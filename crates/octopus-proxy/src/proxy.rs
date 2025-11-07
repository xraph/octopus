//! HTTP proxy implementation

use crate::client::{Body, HttpClient};
use crate::pool::ConnectionPool;
use bytes::Bytes;
use http::{Request, Response, Uri};
use http_body_util::{BodyExt, Full};
use octopus_core::{Error, Result, UpstreamInstance};
use std::sync::Arc;

/// Proxy configuration
#[derive(Debug, Clone)]
pub struct ProxyConfig {
    /// Whether to preserve the Host header
    pub preserve_host: bool,

    /// Whether to add X-Forwarded-* headers
    pub add_forwarded_headers: bool,

    /// Custom headers to add to upstream requests
    pub upstream_headers: Vec<(String, String)>,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            preserve_host: false,
            add_forwarded_headers: true,
            upstream_headers: Vec::new(),
        }
    }
}

/// HTTP proxy
#[derive(Debug, Clone)]
pub struct HttpProxy {
    client: HttpClient,
    #[allow(dead_code)]
    pool: Arc<ConnectionPool>,
    config: ProxyConfig,
}

impl HttpProxy {
    /// Create a new HTTP proxy
    pub fn new(client: HttpClient, pool: Arc<ConnectionPool>, config: ProxyConfig) -> Self {
        Self {
            client,
            pool,
            config,
        }
    }

    /// Proxy a request to an upstream instance
    pub async fn proxy(
        &self,
        mut req: Request<Full<Bytes>>,
        upstream: &UpstreamInstance,
    ) -> Result<Response<Full<Bytes>>> {
        // Build upstream URI
        let upstream_uri = self.build_upstream_uri(&req, upstream)?;

        // Update request URI
        *req.uri_mut() = upstream_uri;

        // Transform headers
        self.transform_headers(&mut req, upstream)?;

        // Send request
        let response = self.client.send(req, upstream).await?;

        // Convert response body
        // Note: In production, this should stream the body, not collect it all
        let (parts, body) = response.into_parts();
        let body_bytes = body
            .collect()
            .await
            .map_err(|e| Error::UpstreamConnection(e.to_string()))?
            .to_bytes();

        Ok(Response::from_parts(parts, Full::new(body_bytes)))
    }

    /// Build the upstream URI
    fn build_upstream_uri(&self, req: &Request<Body>, upstream: &UpstreamInstance) -> Result<Uri> {
        let path_and_query = req
            .uri()
            .path_and_query()
            .map(|pq| pq.as_str())
            .unwrap_or("/");

        let upstream_uri = format!(
            "http://{}:{}{}",
            upstream.address, upstream.port, path_and_query
        );

        upstream_uri
            .parse()
            .map_err(|e| Error::UpstreamConnection(format!("Invalid upstream URI: {e}")))
    }

    /// Transform request headers
    fn transform_headers(
        &self,
        req: &mut Request<Body>,
        upstream: &UpstreamInstance,
    ) -> Result<()> {
        let headers = req.headers_mut();

        // Update Host header if not preserving
        if !self.config.preserve_host {
            let host = format!("{}:{}", upstream.address, upstream.port);
            headers.insert(
                http::header::HOST,
                host.parse()
                    .map_err(|e| Error::InvalidRequest(format!("Invalid host: {e}")))?,
            );
        }

        // Add X-Forwarded-* headers
        if self.config.add_forwarded_headers {
            // X-Forwarded-For (would need client IP from connection)
            // X-Forwarded-Proto
            headers.insert(
                http::HeaderName::from_static("x-forwarded-proto"),
                http::HeaderValue::from_static("http"),
            );
        }

        // Add custom headers
        for (name, value) in &self.config.upstream_headers {
            headers.insert(
                http::HeaderName::from_bytes(name.as_bytes())
                    .map_err(|e| Error::InvalidRequest(format!("Invalid header name: {e}")))?,
                http::HeaderValue::from_str(value)
                    .map_err(|e| Error::InvalidRequest(format!("Invalid header value: {e}")))?,
            );
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pool::PoolConfig;

    #[test]
    fn test_proxy_config() {
        let config = ProxyConfig::default();
        assert!(!config.preserve_host);
        assert!(config.add_forwarded_headers);
    }

    #[test]
    fn test_build_upstream_uri() {
        let client = HttpClient::new();
        let pool = Arc::new(ConnectionPool::new(PoolConfig::default()));
        let proxy = HttpProxy::new(client, pool, ProxyConfig::default());

        let req = Request::builder()
            .uri("/test?foo=bar")
            .body(Full::new(Bytes::new()))
            .unwrap();

        let upstream = UpstreamInstance::new("test", "localhost", 8080);

        let uri = proxy.build_upstream_uri(&req, &upstream).unwrap();
        assert_eq!(uri.to_string(), "http://localhost:8080/test?foo=bar");
    }
}
