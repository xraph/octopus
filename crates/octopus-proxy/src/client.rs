//! HTTP client for making requests to upstream services using connection pooling

use crate::pool::ConnectionPool;
use bytes::Bytes;
use http::{Request, Response};
use http_body_util::Full;
use hyper::body::Incoming;
use octopus_core::{Error, Result, UpstreamInstance};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, trace};

/// Body type alias
pub type Body = Full<Bytes>;

/// HTTP client for upstream requests with connection pooling
#[derive(Clone)]
pub struct HttpClient {
    pool: Arc<ConnectionPool>,
    h2_pool: Arc<crate::pool::Http2Pool>,
    timeout: Duration,
}

impl HttpClient {
    /// Create a new HTTP client with default pool
    pub fn new() -> Self {
        Self {
            pool: Arc::new(ConnectionPool::default()),
            h2_pool: Arc::new(crate::pool::Http2Pool::default()),
            timeout: Duration::from_secs(30),
        }
    }

    /// Create a new HTTP client with custom pool
    pub fn with_pool(pool: Arc<ConnectionPool>) -> Self {
        Self {
            pool,
            h2_pool: Arc::new(crate::pool::Http2Pool::default()),
            timeout: Duration::from_secs(30),
        }
    }

    /// Create a new HTTP client with custom timeout
    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            pool: Arc::new(ConnectionPool::default()),
            h2_pool: Arc::new(crate::pool::Http2Pool::default()),
            timeout,
        }
    }

    /// Create a new HTTP client with custom pool and timeout
    pub fn new_with_config(pool: Arc<ConnectionPool>, timeout: Duration) -> Self {
        Self {
            pool,
            h2_pool: Arc::new(crate::pool::Http2Pool::default()),
            timeout,
        }
    }

    /// Send a request to an upstream instance using a pooled connection
    pub async fn send(
        &self,
        req: Request<Body>,
        upstream: &UpstreamInstance,
    ) -> Result<Response<Incoming>> {
        trace!(
            upstream = %upstream.id,
            method = %req.method(),
            uri = %req.uri(),
            "Sending request to upstream"
        );

        // Get a pooled connection
        let mut pooled_conn = self.pool.get_connection(upstream).await?;

        // Send request with timeout
        let result = tokio::time::timeout(
            self.timeout,
            pooled_conn.sender().send_request(req),
        )
        .await;

        let response = match result {
            Ok(Ok(resp)) => {
                debug!(
                    upstream = %upstream.id,
                    status = resp.status().as_u16(),
                    "Received response from upstream"
                );
                Ok(resp)
            }
            Ok(Err(e)) => {
                debug!(
                    upstream = %upstream.id,
                    error = %e,
                    "Upstream request failed"
                );
                Err(Error::UpstreamConnection(e.to_string()))
            }
            Err(_) => {
                debug!(
                    upstream = %upstream.id,
                    timeout_secs = self.timeout.as_secs(),
                    "Request timeout"
                );
                Err(Error::UpstreamTimeout)
            }
        };

        // Return connection to pool (only if successful)
        if response.is_ok() {
            self.pool.return_connection(pooled_conn).await;
        }
        // If error, connection is dropped (not returned to pool)

        response
    }

    /// Send a request via HTTP/2 (for gRPC and HTTP/2 upstreams)
    pub async fn send_h2(
        &self,
        req: Request<Body>,
        upstream: &UpstreamInstance,
    ) -> Result<Response<Incoming>> {
        trace!(
            upstream = %upstream.id,
            method = %req.method(),
            uri = %req.uri(),
            "Sending HTTP/2 request to upstream"
        );

        let mut sender = self.h2_pool.get_sender(upstream).await?;

        let result = tokio::time::timeout(self.timeout, sender.send_request(req)).await;

        match result {
            Ok(Ok(resp)) => {
                debug!(
                    upstream = %upstream.id,
                    status = resp.status().as_u16(),
                    "Received HTTP/2 response from upstream"
                );
                Ok(resp)
            }
            Ok(Err(e)) => {
                debug!(
                    upstream = %upstream.id,
                    error = %e,
                    "HTTP/2 upstream request failed"
                );
                Err(Error::UpstreamConnection(e.to_string()))
            }
            Err(_) => {
                debug!(
                    upstream = %upstream.id,
                    timeout_secs = self.timeout.as_secs(),
                    "HTTP/2 request timeout"
                );
                Err(Error::UpstreamTimeout)
            }
        }
    }

    /// Get the configured timeout
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Get reference to the connection pool
    pub fn pool(&self) -> &Arc<ConnectionPool> {
        &self.pool
    }

    /// Get pool statistics for an upstream
    pub fn get_pool_stats(&self, upstream: &UpstreamInstance) -> Option<crate::pool::PoolStats> {
        let key = crate::pool::UpstreamKey::from_instance(upstream);
        self.pool.get_pool_stats(&key)
    }
}

impl Default for HttpClient {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for HttpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpClient")
            .field("timeout", &self.timeout)
            .field("pool_count", &self.pool.pool_count())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_http_client_new() {
        let client = HttpClient::new();
        assert_eq!(client.timeout(), Duration::from_secs(30));
        assert_eq!(client.pool().pool_count(), 0);
    }

    #[tokio::test]
    async fn test_http_client_with_timeout() {
        let client = HttpClient::with_timeout(Duration::from_secs(10));
        assert_eq!(client.timeout(), Duration::from_secs(10));
    }

    #[tokio::test]
    async fn test_http_client_clone() {
        let client = HttpClient::new();
        let cloned = client.clone();
        assert_eq!(client.timeout(), cloned.timeout());
    }
}
