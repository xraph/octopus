//! Retry middleware for re-executing requests on failure
//!
//! Implements exponential backoff retry logic for transient upstream failures.
//! Since `Next` implements `Clone` (backed by `Arc`), we can re-run the
//! downstream middleware chain on each attempt by cloning both the request
//! parts and the `Next` handle.

use async_trait::async_trait;
use bytes::Bytes;
use http::{HeaderValue, Method, Request, Response, StatusCode};
use http_body_util::Full;
use octopus_core::{Middleware, Next, Result};
use std::fmt;
use std::time::Duration;
use tokio::time::sleep;

/// Body type alias
pub type Body = Full<Bytes>;

/// Retry middleware configuration
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of total attempts (including the initial one)
    pub max_attempts: u32,
    /// HTTP status codes that trigger a retry
    pub retryable_status_codes: Vec<u16>,
    /// HTTP methods eligible for retry
    pub retryable_methods: Vec<String>,
    /// Initial backoff duration in milliseconds
    pub initial_backoff_ms: u64,
    /// Maximum backoff duration in milliseconds
    pub max_backoff_ms: u64,
    /// Multiplier applied to backoff after each attempt
    pub backoff_multiplier: f64,
    /// Whether to retry on timeout errors
    pub retry_on_timeout: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            retryable_status_codes: vec![502, 503, 504],
            retryable_methods: vec![
                "GET".to_string(),
                "HEAD".to_string(),
                "PUT".to_string(),
                "DELETE".to_string(),
                "OPTIONS".to_string(),
            ],
            initial_backoff_ms: 100,
            max_backoff_ms: 5000,
            backoff_multiplier: 2.0,
            retry_on_timeout: true,
        }
    }
}

/// Retry middleware
///
/// Re-executes requests when the downstream chain returns a retryable status
/// code. The request body (`Full<Bytes>`) is cheap to clone because it is
/// backed by a reference-counted `Bytes` buffer.
#[derive(Clone)]
pub struct Retry {
    config: RetryConfig,
}

impl Retry {
    /// Create a new retry middleware with default configuration
    pub fn new() -> Self {
        Self::with_config(RetryConfig::default())
    }

    /// Create a new retry middleware with custom configuration
    pub fn with_config(config: RetryConfig) -> Self {
        Self { config }
    }

    /// Check whether the given HTTP method is eligible for retry
    fn is_method_retryable(&self, method: &Method) -> bool {
        self.config
            .retryable_methods
            .iter()
            .any(|m| m.eq_ignore_ascii_case(method.as_str()))
    }

    /// Check whether the given status code should trigger a retry
    fn is_status_retryable(&self, status: StatusCode) -> bool {
        self.config
            .retryable_status_codes
            .contains(&status.as_u16())
    }

    /// Compute the backoff duration for the given attempt (0-indexed)
    fn backoff_for_attempt(&self, attempt: u32) -> Duration {
        let base = self.config.initial_backoff_ms as f64;
        let multiplier = self.config.backoff_multiplier.powi(attempt as i32);
        let ms = (base * multiplier).min(self.config.max_backoff_ms as f64);
        Duration::from_millis(ms as u64)
    }

    /// Clone an `http::Request<Body>`.
    ///
    /// `Request<Full<Bytes>>` is `Clone` because `Full<Bytes>` is `Clone`
    /// (backed by a ref-counted `Bytes` buffer).
    fn clone_request(req: &Request<Body>) -> Request<Body> {
        req.clone()
    }
}

impl Default for Retry {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Retry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Retry")
            .field("max_attempts", &self.config.max_attempts)
            .field(
                "retryable_status_codes",
                &self.config.retryable_status_codes,
            )
            .finish()
    }
}

#[async_trait]
impl Middleware for Retry {
    async fn call(&self, req: Request<Body>, next: Next) -> Result<Response<Body>> {
        // Non-retryable methods pass through immediately
        if !self.is_method_retryable(req.method()) {
            return next.run(req).await;
        }

        let max = self.config.max_attempts.max(1);

        // We need to keep re-usable copies of the request and the Next handle
        // across attempts. Both are cheap to clone.
        let mut last_response: Option<Response<Body>> = None;

        for attempt in 0..max {
            let cloned_req = if attempt == max - 1 {
                // Last attempt -- we can consume the original request indirectly
                // but we already clone every time for simplicity.
                Self::clone_request(&req)
            } else {
                Self::clone_request(&req)
            };

            let cloned_next = next.clone();
            let result = cloned_next.run(cloned_req).await;

            match result {
                Ok(mut response) => {
                    if attempt > 0 {
                        // Tag the response with retry metadata
                        response.headers_mut().insert(
                            "X-Retry-Attempts",
                            HeaderValue::from_str(&attempt.to_string())
                                .unwrap_or_else(|_| HeaderValue::from_static("0")),
                        );
                    }

                    if self.is_status_retryable(response.status()) && attempt < max - 1 {
                        tracing::warn!(
                            attempt = attempt + 1,
                            max_attempts = max,
                            status = %response.status(),
                            uri = %req.uri(),
                            "Retryable status code, will retry after backoff"
                        );
                        last_response = Some(response);
                        sleep(self.backoff_for_attempt(attempt)).await;
                        continue;
                    }

                    return Ok(response);
                }
                Err(err) => {
                    let should_retry = match &err {
                        octopus_core::Error::UpstreamTimeout if self.config.retry_on_timeout => {
                            true
                        }
                        octopus_core::Error::UpstreamConnection(_) => true,
                        _ => false,
                    };

                    if should_retry && attempt < max - 1 {
                        tracing::warn!(
                            attempt = attempt + 1,
                            max_attempts = max,
                            error = %err,
                            uri = %req.uri(),
                            "Retryable error, will retry after backoff"
                        );
                        sleep(self.backoff_for_attempt(attempt)).await;
                        continue;
                    }

                    return Err(err);
                }
            }
        }

        // Should not be reached, but return last response if somehow we exit the loop
        match last_response {
            Some(resp) => Ok(resp),
            None => Err(octopus_core::Error::Internal(
                "Retry loop exhausted without result".to_string(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use octopus_core::Error;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    /// A test handler that returns a configurable status code for the first N
    /// calls, then succeeds.
    #[derive(Debug)]
    struct FailThenSucceedHandler {
        fail_count: u32,
        fail_status: StatusCode,
        call_counter: Arc<AtomicU32>,
    }

    #[async_trait]
    impl Middleware for FailThenSucceedHandler {
        async fn call(&self, _req: Request<Body>, _next: Next) -> Result<Response<Body>> {
            let n = self.call_counter.fetch_add(1, Ordering::SeqCst);
            if n < self.fail_count {
                Response::builder()
                    .status(self.fail_status)
                    .body(Full::new(Bytes::from("error")))
                    .map_err(|e| Error::Internal(e.to_string()))
            } else {
                Response::builder()
                    .status(StatusCode::OK)
                    .body(Full::new(Bytes::from("success")))
                    .map_err(|e| Error::Internal(e.to_string()))
            }
        }
    }

    #[derive(Debug)]
    struct AlwaysOkHandler;

    #[async_trait]
    impl Middleware for AlwaysOkHandler {
        async fn call(&self, _req: Request<Body>, _next: Next) -> Result<Response<Body>> {
            Response::builder()
                .status(StatusCode::OK)
                .body(Full::new(Bytes::from("ok")))
                .map_err(|e| Error::Internal(e.to_string()))
        }
    }

    #[tokio::test]
    async fn test_retry_succeeds_after_transient_failure() {
        let counter = Arc::new(AtomicU32::new(0));
        let handler = FailThenSucceedHandler {
            fail_count: 2,
            fail_status: StatusCode::BAD_GATEWAY,
            call_counter: counter.clone(),
        };

        let config = RetryConfig {
            max_attempts: 3,
            initial_backoff_ms: 1, // fast for tests
            max_backoff_ms: 10,
            ..Default::default()
        };
        let retry = Retry::with_config(config);

        let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([Arc::new(retry), Arc::new(handler)]);

        let next = Next::new(stack);
        let req = Request::builder()
            .method(Method::GET)
            .uri("/test")
            .body(Body::from(""))
            .unwrap();

        let response = next.run(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        // The handler was called 3 times (2 failures + 1 success)
        assert_eq!(counter.load(Ordering::SeqCst), 3);
        // Retry header should be present
        assert_eq!(response.headers().get("X-Retry-Attempts").unwrap(), "2");
    }

    #[tokio::test]
    async fn test_retry_exhausts_all_attempts() {
        let counter = Arc::new(AtomicU32::new(0));
        let handler = FailThenSucceedHandler {
            fail_count: 10, // always fail
            fail_status: StatusCode::SERVICE_UNAVAILABLE,
            call_counter: counter.clone(),
        };

        let config = RetryConfig {
            max_attempts: 3,
            initial_backoff_ms: 1,
            max_backoff_ms: 5,
            ..Default::default()
        };
        let retry = Retry::with_config(config);

        let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([Arc::new(retry), Arc::new(handler)]);

        let next = Next::new(stack);
        let req = Request::builder()
            .method(Method::GET)
            .uri("/test")
            .body(Body::from(""))
            .unwrap();

        let response = next.run(req).await.unwrap();
        // After exhausting retries, the last retryable response is returned
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_retry_skips_non_retryable_method() {
        let counter = Arc::new(AtomicU32::new(0));
        let handler = FailThenSucceedHandler {
            fail_count: 10,
            fail_status: StatusCode::BAD_GATEWAY,
            call_counter: counter.clone(),
        };

        let config = RetryConfig {
            max_attempts: 3,
            initial_backoff_ms: 1,
            max_backoff_ms: 5,
            ..Default::default()
        };
        let retry = Retry::with_config(config);

        let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([Arc::new(retry), Arc::new(handler)]);

        let next = Next::new(stack);
        // POST is not in the default retryable methods list
        let req = Request::builder()
            .method(Method::POST)
            .uri("/test")
            .body(Body::from(""))
            .unwrap();

        let response = next.run(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        // Only called once -- no retry for POST
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_retry_no_retry_on_success() {
        let retry = Retry::new();
        let handler = AlwaysOkHandler;

        let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([Arc::new(retry), Arc::new(handler)]);

        let next = Next::new(stack);
        let req = Request::builder()
            .method(Method::GET)
            .uri("/ok")
            .body(Body::from(""))
            .unwrap();

        let response = next.run(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        // No retry header on first-attempt success
        assert!(response.headers().get("X-Retry-Attempts").is_none());
    }

    #[tokio::test]
    async fn test_backoff_calculation() {
        let config = RetryConfig {
            initial_backoff_ms: 100,
            max_backoff_ms: 5000,
            backoff_multiplier: 2.0,
            ..Default::default()
        };
        let retry = Retry::with_config(config);

        assert_eq!(retry.backoff_for_attempt(0), Duration::from_millis(100));
        assert_eq!(retry.backoff_for_attempt(1), Duration::from_millis(200));
        assert_eq!(retry.backoff_for_attempt(2), Duration::from_millis(400));
        assert_eq!(retry.backoff_for_attempt(3), Duration::from_millis(800));
        // Should be capped at max_backoff_ms
        assert_eq!(retry.backoff_for_attempt(10), Duration::from_millis(5000));
    }
}
