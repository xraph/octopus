//! HTTP proxy implementation with zero-copy streaming

use crate::client::{Body, HttpClient};
use crate::pool::ConnectionPool;
use crate::retry::RetryPolicy;
use bytes::Bytes;
use http::{Request, Response, Uri};
use http_body_util::Full;
use hyper::body::Incoming;
use octopus_core::{Error, Result, UpstreamInstance};
use octopus_health::circuit_breaker::{CircuitBreaker, CircuitBreakerConfig};
use std::sync::Arc;
use tracing::{debug, instrument, warn};

/// Proxy configuration
#[derive(Debug, Clone)]
pub struct ProxyConfig {
    /// Whether to preserve the Host header
    pub preserve_host: bool,

    /// Whether to add X-Forwarded-* headers
    pub add_forwarded_headers: bool,

    /// Custom headers to add to upstream requests
    pub upstream_headers: Vec<(String, String)>,

    /// Enable circuit breaker
    pub enable_circuit_breaker: bool,

    /// Enable retry logic
    pub enable_retry: bool,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            preserve_host: false,
            add_forwarded_headers: true,
            upstream_headers: Vec::new(),
            enable_circuit_breaker: true,
            enable_retry: true,
        }
    }
}

/// HTTP proxy with zero-copy body streaming, retry logic, and circuit breaker
#[derive(Clone)]
pub struct HttpProxy {
    client: HttpClient,
    config: ProxyConfig,
    circuit_breaker: Arc<CircuitBreaker>,
    retry_policy: Arc<RetryPolicy>,
}

impl HttpProxy {
    /// Create a new HTTP proxy
    pub fn new(client: HttpClient, config: ProxyConfig) -> Self {
        Self {
            client,
            config,
            circuit_breaker: Arc::new(CircuitBreaker::new(CircuitBreakerConfig::default())),
            retry_policy: Arc::new(RetryPolicy::default()),
        }
    }

    /// Create a new HTTP proxy with default config
    pub fn with_client(client: HttpClient) -> Self {
        Self {
            client,
            config: ProxyConfig::default(),
            circuit_breaker: Arc::new(CircuitBreaker::new(CircuitBreakerConfig::default())),
            retry_policy: Arc::new(RetryPolicy::default()),
        }
    }

    /// Create a new HTTP proxy with connection pool
    pub fn with_pool(pool: Arc<ConnectionPool>, config: ProxyConfig) -> Self {
        let client = HttpClient::with_pool(pool);
        Self {
            client,
            config,
            circuit_breaker: Arc::new(CircuitBreaker::new(CircuitBreakerConfig::default())),
            retry_policy: Arc::new(RetryPolicy::default()),
        }
    }

    /// Create a new HTTP proxy with full configuration
    pub fn with_full_config(
        client: HttpClient,
        config: ProxyConfig,
        circuit_breaker: Arc<CircuitBreaker>,
        retry_policy: Arc<RetryPolicy>,
    ) -> Self {
        Self {
            client,
            config,
            circuit_breaker,
            retry_policy,
        }
    }

    /// Set circuit breaker
    pub fn with_circuit_breaker(mut self, circuit_breaker: Arc<CircuitBreaker>) -> Self {
        self.circuit_breaker = circuit_breaker;
        self
    }

    /// Set retry policy
    pub fn with_retry_policy(mut self, retry_policy: Arc<RetryPolicy>) -> Self {
        self.retry_policy = retry_policy;
        self
    }

    /// Proxy a request to an upstream instance with resilience (circuit breaker only)
    ///
    /// Note: Retry logic is currently disabled due to request body cloning limitations.
    /// In production, implement request buffering for idempotent retries.
    #[instrument(skip(self, req), fields(upstream = %upstream.id))]
    pub async fn proxy_resilient(
        &self,
        req: Request<Body>,
        upstream: &UpstreamInstance,
    ) -> Result<Response<Incoming>> {
        // Check circuit breaker first
        if self.config.enable_circuit_breaker {
            if !self.circuit_breaker.allow_request(&upstream.id) {
                warn!(upstream = %upstream.id, "Circuit breaker is OPEN, rejecting request");
                return Err(Error::CircuitBreakerOpen(upstream.id.clone()));
            }
        }

        // Execute the request
        let result = self.proxy(req, upstream).await;

        // Update circuit breaker based on result
        if self.config.enable_circuit_breaker {
            match &result {
                Ok(_) => self.circuit_breaker.record_success(&upstream.id),
                Err(_) => self.circuit_breaker.record_failure(&upstream.id),
            }
        }

        result
    }

    /// Proxy a request to an upstream instance (zero-copy streaming, no resilience)
    #[instrument(skip(self, req), fields(upstream = %upstream.id))]
    pub async fn proxy(
        &self,
        mut req: Request<Body>,
        upstream: &UpstreamInstance,
    ) -> Result<Response<Incoming>> {
        // Build upstream URI
        let upstream_uri = self.build_upstream_uri(&req, upstream)?;
        
        debug!(
            method = %req.method(),
            uri = %upstream_uri,
            "Proxying request to upstream"
        );

        // Update request URI
        *req.uri_mut() = upstream_uri;

        // Transform headers
        self.transform_headers(&mut req, upstream)?;

        // Send request and stream response directly (zero-copy)
        let response = self.client.send(req, upstream).await?;

        debug!(
            status = response.status().as_u16(),
            "Received response from upstream"
        );

        Ok(response)
    }

    /// Proxy a request and collect body (for backward compatibility)
    /// 
    /// Note: This buffers the entire response body in memory.
    /// Use `proxy()` for zero-copy streaming whenever possible.
    #[instrument(skip(self, req), fields(upstream = %upstream.id))]
    pub async fn proxy_buffered(
        &self,
        req: Request<Body>,
        upstream: &UpstreamInstance,
    ) -> Result<Response<Full<Bytes>>> {
        use http_body_util::BodyExt;
        
        // Get streaming response
        let response = self.proxy(req, upstream).await?;

        // Collect body into bytes
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
            // X-Forwarded-Proto
            headers.insert(
                http::HeaderName::from_static("x-forwarded-proto"),
                http::HeaderValue::from_static("http"),
            );
            
            // TODO: Add X-Forwarded-For when client IP is available
            // TODO: Add X-Forwarded-Host
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

    /// Get reference to the HTTP client
    pub fn client(&self) -> &HttpClient {
        &self.client
    }

    /// Get proxy configuration
    pub fn config(&self) -> &ProxyConfig {
        &self.config
    }

    /// Get circuit breaker
    pub fn circuit_breaker(&self) -> &Arc<CircuitBreaker> {
        &self.circuit_breaker
    }

    /// Get retry policy
    pub fn retry_policy(&self) -> &Arc<RetryPolicy> {
        &self.retry_policy
    }
}


impl std::fmt::Debug for HttpProxy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpProxy")
            .field("client", &self.client)
            .field("config", &self.config)
            .field("circuit_breaker", &"CircuitBreaker{...}")
            .field("retry_policy", &self.retry_policy)
            .finish()
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

    #[tokio::test]
    async fn test_build_upstream_uri() {
        let client = HttpClient::new();
        let proxy = HttpProxy::new(client, ProxyConfig::default());

        let req = Request::builder()
            .uri("/test?foo=bar")
            .body(Full::new(Bytes::new()))
            .unwrap();

        let upstream = UpstreamInstance::new("test", "localhost", 8080);

        let uri = proxy.build_upstream_uri(&req, &upstream).unwrap();
        assert_eq!(uri.to_string(), "http://localhost:8080/test?foo=bar");
    }

    #[tokio::test]
    async fn test_proxy_creation() {
        let pool = Arc::new(ConnectionPool::new(PoolConfig::default()));
        let proxy = HttpProxy::with_pool(pool, ProxyConfig::default());
        assert!(!proxy.config().preserve_host);
    }
}
