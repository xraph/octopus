//! Request/Response logging middleware

use async_trait::async_trait;
use bytes::Bytes;
use http::{Request, Response};
use http_body_util::Full;
use octopus_core::{Middleware, Next, Result};
use std::fmt;
use std::time::Instant;
use tracing::{info, warn, Level};

/// Body type alias
pub type Body = Full<Bytes>;

/// Logging configuration
#[derive(Debug, Clone)]
pub struct LoggingConfig {
    /// Log level for requests
    pub log_level: Level,
    /// Whether to log request headers
    pub log_headers: bool,
    /// Whether to log request body
    pub log_body: bool,
    /// Maximum body size to log (bytes)
    pub max_body_size: usize,
    /// Headers to redact (e.g., Authorization, Cookie)
    pub sensitive_headers: Vec<String>,
    /// Whether to log response status
    pub log_response: bool,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            log_level: Level::INFO,
            log_headers: true,
            log_body: false,
            max_body_size: 4096,
            sensitive_headers: vec![
                "authorization".to_string(),
                "cookie".to_string(),
                "set-cookie".to_string(),
                "x-api-key".to_string(),
            ],
            log_response: true,
        }
    }
}

/// Request/Response logging middleware
///
/// Logs requests and responses with structured logging using tracing.
/// Supports redacting sensitive headers and body content.
#[derive(Clone)]
pub struct RequestLogger {
    config: LoggingConfig,
}

impl RequestLogger {
    /// Create a new RequestLogger with default config
    pub fn new() -> Self {
        Self::with_config(LoggingConfig::default())
    }

    /// Create a new RequestLogger with custom config
    pub fn with_config(config: LoggingConfig) -> Self {
        Self { config }
    }

    /// Check if a header should be redacted
    fn should_redact(&self, header_name: &str) -> bool {
        self.config
            .sensitive_headers
            .iter()
            .any(|h| h.eq_ignore_ascii_case(header_name))
    }

    /// Redact a header value
    fn redact_value(&self, header_name: &str, value: &str) -> String {
        if self.should_redact(header_name) {
            "[REDACTED]".to_string()
        } else {
            value.to_string()
        }
    }
}

impl Default for RequestLogger {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for RequestLogger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RequestLogger")
            .field("log_level", &self.config.log_level)
            .field("log_headers", &self.config.log_headers)
            .field("log_body", &self.config.log_body)
            .finish()
    }
}

#[async_trait]
impl Middleware for RequestLogger {
    async fn call(&self, req: Request<Body>, next: Next) -> Result<Response<Body>> {
        let method = req.method().clone();
        let uri = req.uri().clone();
        let version = req.version();

        // Log request
        if self.config.log_headers {
            let headers: Vec<String> = req
                .headers()
                .iter()
                .map(|(name, value)| {
                    let value_str = value.to_str().unwrap_or("[invalid UTF-8]");
                    let redacted = self.redact_value(name.as_str(), value_str);
                    format!("{}: {}", name, redacted)
                })
                .collect();

            match self.config.log_level {
                Level::TRACE => tracing::trace!(
                    method = %method,
                    uri = %uri,
                    version = ?version,
                    headers = ?headers,
                    "Incoming request"
                ),
                Level::DEBUG => tracing::debug!(
                    method = %method,
                    uri = %uri,
                    version = ?version,
                    headers = ?headers,
                    "Incoming request"
                ),
                Level::INFO => tracing::info!(
                    method = %method,
                    uri = %uri,
                    version = ?version,
                    headers = ?headers,
                    "Incoming request"
                ),
                Level::WARN => tracing::warn!(
                    method = %method,
                    uri = %uri,
                    version = ?version,
                    headers = ?headers,
                    "Incoming request"
                ),
                Level::ERROR => tracing::error!(
                    method = %method,
                    uri = %uri,
                    version = ?version,
                    headers = ?headers,
                    "Incoming request"
                ),
            }
        } else {
            match self.config.log_level {
                Level::TRACE => tracing::trace!(
                    method = %method,
                    uri = %uri,
                    version = ?version,
                    "Incoming request"
                ),
                Level::DEBUG => tracing::debug!(
                    method = %method,
                    uri = %uri,
                    version = ?version,
                    "Incoming request"
                ),
                Level::INFO => tracing::info!(
                    method = %method,
                    uri = %uri,
                    version = ?version,
                    "Incoming request"
                ),
                Level::WARN => tracing::warn!(
                    method = %method,
                    uri = %uri,
                    version = ?version,
                    "Incoming request"
                ),
                Level::ERROR => tracing::error!(
                    method = %method,
                    uri = %uri,
                    version = ?version,
                    "Incoming request"
                ),
            }
        }

        // Start timer
        let start = Instant::now();

        // Call next middleware
        let response = next.run(req).await;

        // Calculate duration
        let duration = start.elapsed();

        // Log response
        match &response {
            Ok(resp) => {
                if self.config.log_response {
                    info!(
                        method = %method,
                        uri = %uri,
                        status = resp.status().as_u16(),
                        duration_ms = duration.as_millis(),
                        "Request completed"
                    );
                }
            }
            Err(e) => {
                warn!(
                    method = %method,
                    uri = %uri,
                    error = %e,
                    duration_ms = duration.as_millis(),
                    "Request failed"
                );
            }
        }

        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::StatusCode;
    use octopus_core::Error;

    #[derive(Debug)]
    struct TestHandler {
        status: StatusCode,
    }

    #[async_trait]
    impl Middleware for TestHandler {
        async fn call(&self, _req: Request<Body>, _next: Next) -> Result<Response<Body>> {
            Response::builder()
                .status(self.status)
                .body(Full::new(Bytes::from("test response")))
                .map_err(|e| Error::Internal(e.to_string()))
        }
    }

    #[tokio::test]
    async fn test_request_logging() {
        let logger = RequestLogger::new();
        let handler = TestHandler {
            status: StatusCode::OK,
        };

        let stack: std::sync::Arc<[std::sync::Arc<dyn Middleware>]> =
            std::sync::Arc::new([std::sync::Arc::new(logger), std::sync::Arc::new(handler)]);

        let next = Next::new(stack);

        let req = Request::builder()
            .uri("/test")
            .header("Content-Type", "application/json")
            .body(Body::from(""))
            .unwrap();

        let response = next.run(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_sensitive_header_redaction() {
        let logger = RequestLogger::new();

        // Test redaction
        assert_eq!(
            logger.redact_value("Authorization", "Bearer token123"),
            "[REDACTED]"
        );
        assert_eq!(logger.redact_value("Cookie", "session=abc"), "[REDACTED]");
        assert_eq!(
            logger.redact_value("Content-Type", "application/json"),
            "application/json"
        );
    }

    #[tokio::test]
    async fn test_custom_logging_config() {
        let config = LoggingConfig {
            log_level: Level::DEBUG,
            log_headers: false,
            log_body: false,
            max_body_size: 1024,
            sensitive_headers: vec!["X-Custom-Token".to_string()],
            log_response: true,
        };

        let logger = RequestLogger::with_config(config.clone());
        let handler = TestHandler {
            status: StatusCode::OK,
        };

        let stack: std::sync::Arc<[std::sync::Arc<dyn Middleware>]> =
            std::sync::Arc::new([std::sync::Arc::new(logger), std::sync::Arc::new(handler)]);

        let next = Next::new(stack);

        let req = Request::builder()
            .uri("/test")
            .header("X-Custom-Token", "secret123")
            .body(Body::from(""))
            .unwrap();

        let response = next.run(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_error_logging() {
        let logger = RequestLogger::new();

        #[derive(Debug)]
        struct ErrorHandler;

        #[async_trait]
        impl Middleware for ErrorHandler {
            async fn call(&self, _req: Request<Body>, _next: Next) -> Result<Response<Body>> {
                Err(Error::Internal("Test error".to_string()))
            }
        }

        let handler = ErrorHandler;

        let stack: std::sync::Arc<[std::sync::Arc<dyn Middleware>]> =
            std::sync::Arc::new([std::sync::Arc::new(logger), std::sync::Arc::new(handler)]);

        let next = Next::new(stack);

        let req = Request::builder()
            .uri("/test")
            .body(Body::from(""))
            .unwrap();

        let result = next.run(req).await;

        assert!(result.is_err());
    }
}
