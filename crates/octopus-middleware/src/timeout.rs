//! Timeout middleware for request/response timeouts

use async_trait::async_trait;
use bytes::Bytes;
use http::{Request, Response, StatusCode};
use http_body_util::Full;
use octopus_core::{Middleware, Next, Result};
use std::fmt;
use std::time::Duration;
use tokio::time::timeout;

/// Body type alias
pub type Body = Full<Bytes>;

/// Configuration for Timeout middleware
#[derive(Debug, Clone)]
pub struct TimeoutConfig {
    /// Overall request timeout
    pub request_timeout: Duration,
    /// Whether to return a custom error message
    pub custom_error_message: Option<String>,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            request_timeout: Duration::from_secs(30),
            custom_error_message: None,
        }
    }
}

/// Timeout middleware
///
/// Enforces a timeout on requests. If the request takes longer than the configured
/// duration, it will be cancelled and a 504 Gateway Timeout response will be returned.
#[derive(Clone)]
pub struct Timeout {
    config: TimeoutConfig,
}

impl Timeout {
    /// Create a new Timeout middleware with default config (30s)
    pub fn new() -> Self {
        Self::with_config(TimeoutConfig::default())
    }

    /// Create a new Timeout middleware with custom config
    pub fn with_config(config: TimeoutConfig) -> Self {
        Self { config }
    }

    /// Create a new Timeout middleware with a specific duration
    pub fn with_duration(duration: Duration) -> Self {
        Self {
            config: TimeoutConfig {
                request_timeout: duration,
                custom_error_message: None,
            },
        }
    }

    /// Build a timeout error response
    fn timeout_response(&self) -> Response<Body> {
        let message = self
            .config
            .custom_error_message
            .as_deref()
            .unwrap_or("Gateway Timeout");

        Response::builder()
            .status(StatusCode::GATEWAY_TIMEOUT)
            .header("Content-Type", "text/plain")
            .body(Full::new(Bytes::from(message.to_string())))
            .expect("Failed to build timeout response")
    }
}

impl Default for Timeout {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Timeout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Timeout")
            .field("request_timeout", &self.config.request_timeout)
            .finish()
    }
}

#[async_trait]
impl Middleware for Timeout {
    async fn call(&self, req: Request<Body>, next: Next) -> Result<Response<Body>> {
        match timeout(self.config.request_timeout, next.run(req)).await {
            Ok(result) => result,
            Err(_) => {
                // Timeout occurred
                tracing::warn!(
                    timeout_ms = self.config.request_timeout.as_millis(),
                    "Request timeout"
                );
                Ok(self.timeout_response())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[derive(Debug)]
    struct SlowHandler {
        delay: Duration,
    }

    #[async_trait]
    impl Middleware for SlowHandler {
        async fn call(&self, _req: Request<Body>, _next: Next) -> Result<Response<Body>> {
            tokio::time::sleep(self.delay).await;
            Response::builder()
                .status(StatusCode::OK)
                .body(Full::new(Bytes::from("success")))
                .map_err(|e| Error::Internal(e.to_string()))
        }
    }

    #[tokio::test]
    async fn test_timeout_no_timeout() {
        let timeout_middleware = Timeout::with_duration(Duration::from_millis(100));
        let handler = SlowHandler {
            delay: Duration::from_millis(10),
        };

        let stack: std::sync::Arc<[std::sync::Arc<dyn Middleware>]> = std::sync::Arc::new([
            std::sync::Arc::new(timeout_middleware),
            std::sync::Arc::new(handler),
        ]);

        let next = Next::new(stack);

        let req = Request::builder()
            .uri("/test")
            .body(Body::from(""))
            .unwrap();

        let response = next.run(req).await.unwrap();

        // Should succeed
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_timeout_triggers() {
        let timeout_middleware = Timeout::with_duration(Duration::from_millis(50));
        let handler = SlowHandler {
            delay: Duration::from_millis(200),
        };

        let stack: std::sync::Arc<[std::sync::Arc<dyn Middleware>]> = std::sync::Arc::new([
            std::sync::Arc::new(timeout_middleware),
            std::sync::Arc::new(handler),
        ]);

        let next = Next::new(stack);

        let req = Request::builder()
            .uri("/test")
            .body(Body::from(""))
            .unwrap();

        let response = next.run(req).await.unwrap();

        // Should return 504 Gateway Timeout
        assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
    }

    #[tokio::test]
    async fn test_custom_error_message() {
        let config = TimeoutConfig {
            request_timeout: Duration::from_millis(50),
            custom_error_message: Some("Custom timeout message".to_string()),
        };

        let timeout_middleware = Timeout::with_config(config);
        let handler = SlowHandler {
            delay: Duration::from_millis(200),
        };

        let stack: std::sync::Arc<[std::sync::Arc<dyn Middleware>]> = std::sync::Arc::new([
            std::sync::Arc::new(timeout_middleware),
            std::sync::Arc::new(handler),
        ]);

        let next = Next::new(stack);

        let req = Request::builder()
            .uri("/test")
            .body(Body::from(""))
            .unwrap();

        let response = next.run(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);

        // Check custom message
        let body_bytes = http_body_util::BodyExt::collect(response.into_body())
            .await
            .unwrap()
            .to_bytes();
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
        assert_eq!(body_str, "Custom timeout message");
    }
}
