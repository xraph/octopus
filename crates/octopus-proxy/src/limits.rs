//! Request and response size limits for DoS protection

use octopus_core::{Error, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Proxy limits configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyLimits {
    /// Maximum request body size in bytes (default: 10MB)
    pub max_request_body_size: usize,

    /// Maximum response body size in bytes (default: 100MB)
    pub max_response_body_size: usize,

    /// Maximum header size in bytes (default: 8KB)
    pub max_header_size: usize,

    /// Maximum URI length (default: 8192)
    pub max_uri_length: usize,

    /// Maximum number of headers (default: 100)
    pub max_headers_count: usize,

    /// Maximum connections per upstream (default: 128)
    pub max_connections_per_upstream: usize,

    /// Maximum total connections across all upstreams (default: 1024)
    pub max_total_connections: usize,

    /// Request timeout (default: 30s)
    #[serde(with = "humantime_serde")]
    pub request_timeout: Duration,

    /// Idle timeout (default: 90s)
    #[serde(with = "humantime_serde")]
    pub idle_timeout: Duration,
}

impl Default for ProxyLimits {
    fn default() -> Self {
        Self {
            max_request_body_size: 10 * 1024 * 1024,    // 10MB
            max_response_body_size: 100 * 1024 * 1024,  // 100MB
            max_header_size: 8 * 1024,                   // 8KB
            max_uri_length: 8192,
            max_headers_count: 100,
            max_connections_per_upstream: 128,
            max_total_connections: 1024,
            request_timeout: Duration::from_secs(30),
            idle_timeout: Duration::from_secs(90),
        }
    }
}

impl ProxyLimits {
    /// Create new limits with custom values
    pub fn new() -> Self {
        Self::default()
    }

    /// Set maximum request body size
    pub fn with_max_request_body_size(mut self, size: usize) -> Self {
        self.max_request_body_size = size;
        self
    }

    /// Set maximum response body size
    pub fn with_max_response_body_size(mut self, size: usize) -> Self {
        self.max_response_body_size = size;
        self
    }

    /// Set maximum header size
    pub fn with_max_header_size(mut self, size: usize) -> Self {
        self.max_header_size = size;
        self
    }

    /// Set maximum URI length
    pub fn with_max_uri_length(mut self, length: usize) -> Self {
        self.max_uri_length = length;
        self
    }

    /// Set request timeout
    pub fn with_request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    /// Validate request URI length
    pub fn validate_uri_length(&self, uri: &str) -> Result<()> {
        if uri.len() > self.max_uri_length {
            return Err(Error::InvalidRequest(format!(
                "URI too long: {} bytes (max: {})",
                uri.len(),
                self.max_uri_length
            )));
        }
        Ok(())
    }

    /// Validate headers size
    pub fn validate_headers<B>(&self, req: &http::Request<B>) -> Result<()> {
        // Check header count
        let header_count = req.headers().len();
        if header_count > self.max_headers_count {
            return Err(Error::InvalidRequest(format!(
                "Too many headers: {} (max: {})",
                header_count, self.max_headers_count
            )));
        }

        // Calculate total header size
        let mut total_size = 0;
        for (name, value) in req.headers() {
            total_size += name.as_str().len() + value.len() + 4; // +4 for ": " and "\r\n"
        }

        if total_size > self.max_header_size {
            return Err(Error::InvalidRequest(format!(
                "Headers too large: {} bytes (max: {})",
                total_size, self.max_header_size
            )));
        }

        Ok(())
    }

    /// Validate request
    pub fn validate_request<B>(&self, req: &http::Request<B>) -> Result<()> {
        // Validate URI length
        self.validate_uri_length(req.uri().to_string().as_str())?;

        // Validate headers
        self.validate_headers(req)?;

        Ok(())
    }
}

/// Limited body wrapper for enforcing size limits
pub struct LimitedBody<B> {
    inner: B,
    limit: usize,
    consumed: usize,
}

impl<B> std::fmt::Debug for LimitedBody<B> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LimitedBody")
            .field("limit", &self.limit)
            .field("consumed", &self.consumed)
            .finish()
    }
}

impl<B> LimitedBody<B> {
    /// Create a new limited body
    pub fn new(inner: B, limit: usize) -> Self {
        Self {
            inner,
            limit,
            consumed: 0,
        }
    }

    /// Get the inner body
    pub fn into_inner(self) -> B {
        self.inner
    }

    /// Get bytes consumed so far
    pub fn consumed(&self) -> usize {
        self.consumed
    }

    /// Get the limit
    pub fn limit(&self) -> usize {
        self.limit
    }
}

impl<B> http_body::Body for LimitedBody<B>
where
    B: http_body::Body + Unpin,
    B::Error: std::fmt::Display,
{
    type Data = B::Data;
    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn poll_frame(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        use bytes::Buf;

        // Get mutable reference to inner body
        let inner = std::pin::Pin::new(&mut self.inner);

        match inner.poll_frame(cx) {
            std::task::Poll::Ready(Some(Ok(frame))) => {
                if let Some(data) = frame.data_ref() {
                    let size = data.remaining();
                    let new_consumed = self.consumed + size;

                    if new_consumed > self.limit {
                        return std::task::Poll::Ready(Some(Err(Box::new(
                            std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                format!(
                                    "Body size limit exceeded: {} bytes (max: {})",
                                    new_consumed, self.limit
                                ),
                            ),
                        ))));
                    }

                    self.consumed = new_consumed;
                }

                std::task::Poll::Ready(Some(Ok(frame)))
            }
            std::task::Poll::Ready(Some(Err(e))) => {
                std::task::Poll::Ready(Some(Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                )))))
            }
            std::task::Poll::Ready(None) => std::task::Poll::Ready(None),
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> http_body::SizeHint {
        self.inner.size_hint()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_limits() {
        let limits = ProxyLimits::default();
        assert_eq!(limits.max_request_body_size, 10 * 1024 * 1024);
        assert_eq!(limits.max_response_body_size, 100 * 1024 * 1024);
        assert_eq!(limits.max_header_size, 8 * 1024);
        assert_eq!(limits.max_uri_length, 8192);
        assert_eq!(limits.max_headers_count, 100);
    }

    #[test]
    fn test_builder_pattern() {
        let limits = ProxyLimits::new()
            .with_max_request_body_size(5 * 1024 * 1024)
            .with_max_uri_length(4096);

        assert_eq!(limits.max_request_body_size, 5 * 1024 * 1024);
        assert_eq!(limits.max_uri_length, 4096);
    }

    #[test]
    fn test_validate_uri_length() {
        let limits = ProxyLimits::default();

        // Valid URI
        assert!(limits.validate_uri_length("/api/test").is_ok());

        // Too long URI
        let long_uri = "/".to_string() + &"a".repeat(10000);
        assert!(limits.validate_uri_length(&long_uri).is_err());
    }

    #[test]
    fn test_validate_headers() {
        let limits = ProxyLimits::default();

        // Valid request
        let req = http::Request::builder()
            .header("content-type", "application/json")
            .body(())
            .unwrap();

        assert!(limits.validate_headers(&req).is_ok());

        // Too many headers
        let mut builder = http::Request::builder();
        for i in 0..150 {
            builder = builder.header(format!("x-header-{}", i), "value");
        }
        let req = builder.body(()).unwrap();

        assert!(limits.validate_headers(&req).is_err());
    }

    #[test]
    fn test_validate_request() {
        let limits = ProxyLimits::default();

        let req = http::Request::builder()
            .uri("/api/test")
            .header("content-type", "application/json")
            .body(())
            .unwrap();

        assert!(limits.validate_request(&req).is_ok());
    }

    #[test]
    fn test_limited_body() {
        use bytes::Bytes;
        use http_body_util::Full;

        let body = Full::new(Bytes::from("test data"));
        let limited = LimitedBody::new(body, 100);

        assert_eq!(limited.limit(), 100);
        assert_eq!(limited.consumed(), 0);
    }
}
