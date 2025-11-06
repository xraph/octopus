//! Request size limits middleware
//!
//! Prevents resource exhaustion by limiting the size of request components.
//! Protects against:
//! - Large body attacks (memory exhaustion)
//! - Header bombing (CPU exhaustion)
//! - URI length attacks (buffer overflow)

use async_trait::async_trait;
use http::{Request, Response, StatusCode};
use octopus_core::{Body, Middleware, Next, Result};
use serde::{Deserialize, Serialize};

/// Request limits configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestLimitsConfig {
    /// Maximum request body size in bytes
    /// Default: 10MB
    #[serde(default = "default_max_body_size")]
    pub max_body_size: usize,

    /// Maximum total header size in bytes
    /// Default: 8KB
    #[serde(default = "default_max_header_size")]
    pub max_header_size: usize,

    /// Maximum URI length in bytes
    /// Default: 8192 (8KB)
    #[serde(default = "default_max_uri_length")]
    pub max_uri_length: usize,

    /// Custom error message for body size exceeded
    #[serde(default)]
    pub body_size_error_message: Option<String>,

    /// Custom error message for header size exceeded
    #[serde(default)]
    pub header_size_error_message: Option<String>,

    /// Custom error message for URI length exceeded
    #[serde(default)]
    pub uri_length_error_message: Option<String>,
}

fn default_max_body_size() -> usize {
    10 * 1024 * 1024 // 10MB
}

fn default_max_header_size() -> usize {
    8 * 1024 // 8KB
}

fn default_max_uri_length() -> usize {
    8192 // 8KB
}

impl Default for RequestLimitsConfig {
    fn default() -> Self {
        Self {
            max_body_size: default_max_body_size(),
            max_header_size: default_max_header_size(),
            max_uri_length: default_max_uri_length(),
            body_size_error_message: None,
            header_size_error_message: None,
            uri_length_error_message: None,
        }
    }
}

/// Request limits middleware
///
/// Validates request size constraints before processing.
///
/// # Example
///
/// ```
/// use octopus_middleware::{RequestLimits, RequestLimitsConfig};
///
/// // Use defaults (10MB body, 8KB headers, 8KB URI)
/// let limits = RequestLimits::default();
///
/// // Custom limits
/// let config = RequestLimitsConfig {
///     max_body_size: 5 * 1024 * 1024, // 5MB
///     max_header_size: 4 * 1024,      // 4KB
///     max_uri_length: 4096,           // 4KB
///     ..Default::default()
/// };
/// let limits = RequestLimits::with_config(config);
/// ```
#[derive(Debug, Clone)]
pub struct RequestLimits {
    config: RequestLimitsConfig,
}

impl RequestLimits {
    /// Create a new request limits middleware with default configuration
    pub fn new() -> Self {
        Self {
            config: RequestLimitsConfig::default(),
        }
    }

    /// Create a new request limits middleware with custom configuration
    pub fn with_config(config: RequestLimitsConfig) -> Self {
        Self { config }
    }

    /// Create a strict request limits configuration
    /// Recommended for public APIs
    pub fn strict() -> Self {
        Self {
            config: RequestLimitsConfig {
                max_body_size: 1024 * 1024,  // 1MB
                max_header_size: 4 * 1024,   // 4KB
                max_uri_length: 2048,        // 2KB
                body_size_error_message: Some(
                    "Request body too large (max 1MB allowed)".to_string(),
                ),
                header_size_error_message: Some(
                    "Request headers too large (max 4KB allowed)".to_string(),
                ),
                uri_length_error_message: Some(
                    "Request URI too long (max 2KB allowed)".to_string(),
                ),
            },
        }
    }

    /// Create a permissive request limits configuration
    /// Use for internal APIs or file uploads
    pub fn permissive() -> Self {
        Self {
            config: RequestLimitsConfig {
                max_body_size: 100 * 1024 * 1024, // 100MB
                max_header_size: 16 * 1024,       // 16KB
                max_uri_length: 16384,            // 16KB
                body_size_error_message: None,
                header_size_error_message: None,
                uri_length_error_message: None,
            },
        }
    }

    fn calculate_header_size(&self, req: &Request<Body>) -> usize {
        let mut size = 0;
        for (name, value) in req.headers() {
            size += name.as_str().len();
            size += value.len();
            size += 4; // ": " and "\r\n"
        }
        size
    }

    fn error_response(status: StatusCode, message: &str) -> Response<Body> {
        use bytes::Bytes;
        use http_body_util::Full;

        let body = serde_json::json!({
            "error": "request_limit_exceeded",
            "message": message,
            "status": status.as_u16(),
        })
        .to_string();

        Response::builder()
            .status(status)
            .header("content-type", "application/json")
            .body(Full::new(Bytes::from(body)))
            .expect("Failed to build error response")
    }
}

impl Default for RequestLimits {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Middleware for RequestLimits {
    async fn call(&self, req: Request<Body>, next: Next) -> Result<Response<Body>> {
        // Check URI length
        let uri_str = req.uri().to_string();
        if uri_str.len() > self.config.max_uri_length {
            let message = self
                .config
                .uri_length_error_message
                .as_deref()
                .unwrap_or("Request URI too long");
            
            tracing::warn!(
                uri_length = uri_str.len(),
                max_length = self.config.max_uri_length,
                "Request URI length exceeded"
            );
            
            return Ok(Self::error_response(StatusCode::URI_TOO_LONG, message));
        }

        // Check header size
        let header_size = self.calculate_header_size(&req);
        if header_size > self.config.max_header_size {
            let message = self
                .config
                .header_size_error_message
                .as_deref()
                .unwrap_or("Request headers too large");
            
            tracing::warn!(
                header_size,
                max_size = self.config.max_header_size,
                "Request header size exceeded"
            );
            
            return Ok(Self::error_response(
                StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE,
                message,
            ));
        }

        // Check body size (via Content-Length header)
        if let Some(content_length) = req.headers().get("content-length") {
            if let Ok(length_str) = content_length.to_str() {
                if let Ok(length) = length_str.parse::<usize>() {
                    if length > self.config.max_body_size {
                        let message = self
                            .config
                            .body_size_error_message
                            .as_deref()
                            .unwrap_or("Request body too large");
                        
                        tracing::warn!(
                            body_size = length,
                            max_size = self.config.max_body_size,
                            "Request body size exceeded"
                        );
                        
                        return Ok(Self::error_response(
                            StatusCode::PAYLOAD_TOO_LARGE,
                            message,
                        ));
                    }
                }
            }
        }

        // All checks passed, proceed with request
        next.run(req).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use http_body_util::Full;
    use std::sync::Arc;

    type TestBody = Full<Bytes>;

    // Mock handler for testing
    #[derive(Debug, Clone)]
    struct TestHandler;

    #[async_trait]
    impl Middleware for TestHandler {
        async fn call(&self, _req: Request<TestBody>, _next: Next) -> Result<Response<TestBody>> {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(Full::new(Bytes::from("success")))
                .unwrap())
        }
    }

    #[tokio::test]
    async fn test_default_limits_accept_normal_request() {
        let limits = RequestLimits::default();
        let handler = TestHandler;
        let stack: Arc<[Arc<dyn Middleware>]> =
            Arc::new([Arc::new(limits), Arc::new(handler)]);

        let req = Request::builder()
            .uri("/test")
            .header("content-length", "1024")
            .body(Full::new(Bytes::from("test")))
            .unwrap();

        let next = Next::new(stack);
        let response = next.run(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_reject_large_body() {
        let config = RequestLimitsConfig {
            max_body_size: 1024, // 1KB limit
            ..Default::default()
        };
        let limits = RequestLimits::with_config(config);
        let handler = TestHandler;
        let stack: Arc<[Arc<dyn Middleware>]> =
            Arc::new([Arc::new(limits), Arc::new(handler)]);

        let req = Request::builder()
            .uri("/test")
            .header("content-length", "2048") // 2KB body
            .body(Full::new(Bytes::from("test")))
            .unwrap();

        let next = Next::new(stack);
        let response = next.run(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn test_reject_long_uri() {
        let config = RequestLimitsConfig {
            max_uri_length: 100,
            ..Default::default()
        };
        let limits = RequestLimits::with_config(config);
        let handler = TestHandler;
        let stack: Arc<[Arc<dyn Middleware>]> =
            Arc::new([Arc::new(limits), Arc::new(handler)]);

        let long_path = format!("/test/{}", "a".repeat(200));
        let req = Request::builder()
            .uri(long_path)
            .body(Full::new(Bytes::from("")))
            .unwrap();

        let next = Next::new(stack);
        let response = next.run(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::URI_TOO_LONG);
    }

    #[tokio::test]
    async fn test_reject_large_headers() {
        let config = RequestLimitsConfig {
            max_header_size: 100,
            ..Default::default()
        };
        let limits = RequestLimits::with_config(config);
        let handler = TestHandler;
        let stack: Arc<[Arc<dyn Middleware>]> =
            Arc::new([Arc::new(limits), Arc::new(handler)]);

        let req = Request::builder()
            .uri("/test")
            .header("x-custom-header", "a".repeat(200))
            .body(Full::new(Bytes::from("")))
            .unwrap();

        let next = Next::new(stack);
        let response = next.run(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE);
    }

    #[tokio::test]
    async fn test_strict_limits() {
        let limits = RequestLimits::strict();
        let handler = TestHandler;
        let stack: Arc<[Arc<dyn Middleware>]> =
            Arc::new([Arc::new(limits), Arc::new(handler)]);

        // Should reject 2MB body (strict allows only 1MB)
        let req = Request::builder()
            .uri("/test")
            .header("content-length", "2097152") // 2MB
            .body(Full::new(Bytes::from("test")))
            .unwrap();

        let next = Next::new(stack);
        let response = next.run(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn test_permissive_limits() {
        let limits = RequestLimits::permissive();
        let handler = TestHandler;
        let stack: Arc<[Arc<dyn Middleware>]> =
            Arc::new([Arc::new(limits), Arc::new(handler)]);

        // Should accept 50MB body (permissive allows up to 100MB)
        let req = Request::builder()
            .uri("/test")
            .header("content-length", "52428800") // 50MB
            .body(Full::new(Bytes::from("test")))
            .unwrap();

        let next = Next::new(stack);
        let response = next.run(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_custom_error_messages() {
        let config = RequestLimitsConfig {
            max_body_size: 1024,
            body_size_error_message: Some("Custom error message".to_string()),
            ..Default::default()
        };
        let limits = RequestLimits::with_config(config);
        let handler = TestHandler;
        let stack: Arc<[Arc<dyn Middleware>]> =
            Arc::new([Arc::new(limits), Arc::new(handler)]);

        let req = Request::builder()
            .uri("/test")
            .header("content-length", "2048")
            .body(Full::new(Bytes::from("test")))
            .unwrap();

        let next = Next::new(stack);
        let response = next.run(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        
        use http_body_util::BodyExt;
        let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body_str = String::from_utf8_lossy(&body_bytes);
        assert!(body_str.contains("Custom error message"));
    }
}

