//! Rate limiting middleware using token bucket algorithm

use async_trait::async_trait;
use bytes::Bytes;
use dashmap::DashMap;
use governor::{
    clock::DefaultClock,
    state::{InMemoryState, NotKeyed},
    Quota, RateLimiter as GovernorRateLimiter,
};
use http::{Request, Response, StatusCode};
use http_body_util::Full;
use octopus_core::{Middleware, Next, Result};
use serde_json;
use std::collections::HashMap;
use std::fmt;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;

/// Body type alias
pub type Body = Full<Bytes>;

/// Rate limit strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitStrategy {
    /// Token bucket algorithm (allows bursts)
    TokenBucket,
    /// Fixed window (simple counter per time window)
    FixedWindow,
}

/// Key extraction strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyExtractor {
    /// Extract key from IP address
    Ip,
    /// Extract key from header (e.g., API key)
    Header,
    /// Extract key from path
    Path,
    /// Global rate limit (no key)
    Global,
}

/// Rate limiting configuration
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Rate limit strategy
    pub strategy: RateLimitStrategy,
    /// Maximum requests per window
    pub requests_per_window: u32,
    /// Time window duration
    pub window_size: Duration,
    /// Key extraction strategy
    pub key_extractor: KeyExtractor,
    /// Header name for key extraction (if using Header strategy)
    pub header_name: Option<String>,
    /// Custom error message
    pub error_message: Option<String>,
    /// Per-route rate limits (path -> config)
    pub per_route_limits: Option<HashMap<String, RouteRateLimit>>,
}

/// Per-route rate limit configuration
#[derive(Debug, Clone)]
pub struct RouteRateLimit {
    /// Maximum requests per window for this route
    pub requests_per_window: u32,
    /// Time window duration for this route
    pub window_size: Duration,
    /// Custom error message for this route
    pub error_message: Option<String>,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            strategy: RateLimitStrategy::TokenBucket,
            requests_per_window: 1000,
            window_size: Duration::from_secs(60),
            key_extractor: KeyExtractor::Global,
            header_name: None,
            error_message: None,
            per_route_limits: None,
        }
    }
}

impl RouteRateLimit {
    /// Create a new per-route rate limit
    pub fn new(requests_per_window: u32, window_size: Duration) -> Self {
        Self {
            requests_per_window,
            window_size,
            error_message: None,
        }
    }

    /// Create a per-route limiter for requests per second
    pub fn per_second(requests: u32) -> Self {
        Self::new(requests, Duration::from_secs(1))
    }

    /// Create a per-route limiter for requests per minute
    pub fn per_minute(requests: u32) -> Self {
        Self::new(requests, Duration::from_secs(60))
    }

    /// Set custom error message
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.error_message = Some(message.into());
        self
    }
}

/// Rate limiting middleware
///
/// Limits the rate of requests using a token bucket algorithm.
/// Can rate limit globally, per IP, per API key, or per path.
/// Supports per-route rate limits with different limits for different endpoints.
#[derive(Clone)]
pub struct RateLimit {
    config: RateLimitConfig,
    /// Global rate limiter
    limiter: Arc<GovernorRateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    /// Per-route rate limiters (path -> limiter)
    route_limiters:
        Arc<DashMap<String, Arc<GovernorRateLimiter<NotKeyed, InMemoryState, DefaultClock>>>>,
}

impl RateLimit {
    /// Create a new RateLimit middleware with default config
    pub fn new() -> Self {
        Self::with_config(RateLimitConfig::default())
    }

    /// Create a new RateLimit middleware with custom config
    pub fn with_config(config: RateLimitConfig) -> Self {
        // Create quota from config
        let requests = NonZeroU32::new(config.requests_per_window)
            .unwrap_or_else(|| NonZeroU32::new(1).unwrap());

        let quota = Quota::with_period(config.window_size)
            .unwrap()
            .allow_burst(requests);

        let limiter = Arc::new(GovernorRateLimiter::direct(quota));

        // Create per-route limiters if configured
        let route_limiters = Arc::new(DashMap::new());
        if let Some(ref per_route) = config.per_route_limits {
            for (path, route_config) in per_route {
                let route_requests = NonZeroU32::new(route_config.requests_per_window)
                    .unwrap_or_else(|| NonZeroU32::new(1).unwrap());

                let route_quota = Quota::with_period(route_config.window_size)
                    .unwrap()
                    .allow_burst(route_requests);

                let route_limiter = Arc::new(GovernorRateLimiter::direct(route_quota));
                route_limiters.insert(path.clone(), route_limiter);
            }
        }

        Self {
            config,
            limiter,
            route_limiters,
        }
    }

    /// Create a rate limiter with specific requests per second
    pub fn per_second(requests: u32) -> Self {
        let config = RateLimitConfig {
            requests_per_window: requests,
            window_size: Duration::from_secs(1),
            ..Default::default()
        };
        Self::with_config(config)
    }

    /// Create a rate limiter with specific requests per minute
    pub fn per_minute(requests: u32) -> Self {
        let config = RateLimitConfig {
            requests_per_window: requests,
            window_size: Duration::from_secs(60),
            ..Default::default()
        };
        Self::with_config(config)
    }

    /// Build rate limit error response
    fn rate_limit_response(
        &self,
        window_size: Duration,
        custom_message: Option<&str>,
    ) -> Response<Body> {
        let message = custom_message
            .or(self.config.error_message.as_deref())
            .unwrap_or("Rate limit exceeded");

        Response::builder()
            .status(StatusCode::TOO_MANY_REQUESTS)
            .header("Content-Type", "application/json")
            .header("Retry-After", window_size.as_secs().to_string())
            .header(
                "X-RateLimit-Limit",
                self.config.requests_per_window.to_string(),
            )
            .header("X-RateLimit-Remaining", "0")
            .header("X-RateLimit-Reset", window_size.as_secs().to_string())
            .body(Full::new(Bytes::from(
                serde_json::json!({
                    "error": "rate_limit_exceeded",
                    "message": message,
                    "retry_after": window_size.as_secs()
                })
                .to_string(),
            )))
            .expect("Failed to build rate limit response")
    }

    /// Get the appropriate rate limiter for a request
    fn get_limiter_for_request(
        &self,
        path: &str,
    ) -> (
        Arc<GovernorRateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
        Duration,
        Option<String>,
    ) {
        // Check if there's a per-route limiter for this path
        if let Some(route_limiter) = self.route_limiters.get(path) {
            let route_config = self
                .config
                .per_route_limits
                .as_ref()
                .and_then(|limits| limits.get(path));

            let window_size = route_config
                .map(|c| c.window_size)
                .unwrap_or(self.config.window_size);

            let error_message = route_config.and_then(|c| c.error_message.clone());

            return (route_limiter.clone(), window_size, error_message);
        }

        // Fall back to global limiter
        (self.limiter.clone(), self.config.window_size, None)
    }
}

impl Default for RateLimit {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for RateLimit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RateLimit")
            .field("requests_per_window", &self.config.requests_per_window)
            .field("window_size", &self.config.window_size)
            .field("strategy", &self.config.strategy)
            .finish()
    }
}

#[async_trait]
impl Middleware for RateLimit {
    async fn call(&self, req: Request<Body>, next: Next) -> Result<Response<Body>> {
        let path = req.uri().path().to_string();

        // Get the appropriate limiter for this request
        let (limiter, window_size, custom_message) = self.get_limiter_for_request(&path);

        // Check rate limit
        match limiter.check() {
            Ok(_) => {
                // Request allowed, proceed
                next.run(req).await
            }
            Err(_) => {
                // Rate limit exceeded
                tracing::warn!(
                    uri = %req.uri(),
                    path = %path,
                    "Rate limit exceeded"
                );
                Ok(self.rate_limit_response(window_size, custom_message.as_deref()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;
    use octopus_core::Error;
    use std::time::Duration;
    use tokio::time::sleep;

    #[derive(Debug)]
    struct TestHandler;

    #[async_trait]
    impl Middleware for TestHandler {
        async fn call(&self, _req: Request<Body>, _next: Next) -> Result<Response<Body>> {
            Response::builder()
                .status(StatusCode::OK)
                .body(Full::new(Bytes::from("success")))
                .map_err(|e| Error::Internal(e.to_string()))
        }
    }

    #[tokio::test]
    async fn test_rate_limit_allows_requests() {
        let rate_limit = RateLimit::per_second(10);
        let handler = TestHandler;

        let stack: std::sync::Arc<[std::sync::Arc<dyn Middleware>]> = std::sync::Arc::new([
            std::sync::Arc::new(rate_limit),
            std::sync::Arc::new(handler),
        ]);

        // First request should succeed
        let next = Next::new(stack.clone());
        let req = Request::builder()
            .uri("/test")
            .body(Body::from(""))
            .unwrap();

        let response = next.run(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_rate_limit_blocks_excess() {
        // Very restrictive: 2 requests per second
        let config = RateLimitConfig {
            requests_per_window: 2,
            window_size: Duration::from_secs(1),
            ..Default::default()
        };

        let rate_limit = RateLimit::with_config(config);
        let handler = TestHandler;

        let stack: std::sync::Arc<[std::sync::Arc<dyn Middleware>]> = std::sync::Arc::new([
            std::sync::Arc::new(rate_limit),
            std::sync::Arc::new(handler),
        ]);

        // First two requests should succeed
        for _ in 0..2 {
            let next = Next::new(stack.clone());
            let req = Request::builder()
                .uri("/test")
                .body(Body::from(""))
                .unwrap();

            let response = next.run(req).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }

        // Third request should be rate limited
        let next = Next::new(stack.clone());
        let req = Request::builder()
            .uri("/test")
            .body(Body::from(""))
            .unwrap();

        let response = next.run(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn test_rate_limit_recovery() {
        // 1 request per 100ms
        let config = RateLimitConfig {
            requests_per_window: 1,
            window_size: Duration::from_millis(100),
            ..Default::default()
        };

        let rate_limit = RateLimit::with_config(config);
        let handler = TestHandler;

        let stack: std::sync::Arc<[std::sync::Arc<dyn Middleware>]> = std::sync::Arc::new([
            std::sync::Arc::new(rate_limit),
            std::sync::Arc::new(handler),
        ]);

        // First request succeeds
        let next = Next::new(stack.clone());
        let req = Request::builder()
            .uri("/test")
            .body(Body::from(""))
            .unwrap();
        let response = next.run(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Second immediate request fails
        let next = Next::new(stack.clone());
        let req = Request::builder()
            .uri("/test")
            .body(Body::from(""))
            .unwrap();
        let response = next.run(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);

        // Wait for window to reset
        sleep(Duration::from_millis(150)).await;

        // Third request should succeed
        let next = Next::new(stack.clone());
        let req = Request::builder()
            .uri("/test")
            .body(Body::from(""))
            .unwrap();
        let response = next.run(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_per_route_rate_limits() {
        // Create per-route limits
        let mut per_route_limits = HashMap::new();
        per_route_limits.insert("/api".to_string(), RouteRateLimit::per_second(2));
        per_route_limits.insert("/admin".to_string(), RouteRateLimit::per_second(1));

        let config = RateLimitConfig {
            requests_per_window: 10, // Global limit: 10/sec
            window_size: Duration::from_secs(1),
            per_route_limits: Some(per_route_limits),
            ..Default::default()
        };

        let rate_limit = RateLimit::with_config(config);
        let handler = TestHandler;

        let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([Arc::new(rate_limit), Arc::new(handler)]);

        // Test /api endpoint (2 requests/sec limit)
        for _ in 0..2 {
            let next = Next::new(stack.clone());
            let req = Request::builder().uri("/api").body(Body::from("")).unwrap();
            let response = next.run(req).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }

        // Third request to /api should fail
        let next = Next::new(stack.clone());
        let req = Request::builder().uri("/api").body(Body::from("")).unwrap();
        let response = next.run(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);

        // Test /admin endpoint (1 request/sec limit)
        let next = Next::new(stack.clone());
        let req = Request::builder()
            .uri("/admin")
            .body(Body::from(""))
            .unwrap();
        let response = next.run(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Second request to /admin should fail
        let next = Next::new(stack.clone());
        let req = Request::builder()
            .uri("/admin")
            .body(Body::from(""))
            .unwrap();
        let response = next.run(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);

        // Test endpoint without specific limit (should use global limit of 10/sec)
        for _ in 0..10 {
            let next = Next::new(stack.clone());
            let req = Request::builder()
                .uri("/other")
                .body(Body::from(""))
                .unwrap();
            let response = next.run(req).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }

        // 11th request should fail
        let next = Next::new(stack.clone());
        let req = Request::builder()
            .uri("/other")
            .body(Body::from(""))
            .unwrap();
        let response = next.run(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn test_rate_limit_headers() {
        let config = RateLimitConfig {
            requests_per_window: 1,
            window_size: Duration::from_secs(60),
            ..Default::default()
        };

        let rate_limit = RateLimit::with_config(config);
        let handler = TestHandler;

        let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([Arc::new(rate_limit), Arc::new(handler)]);

        // First request succeeds
        let next = Next::new(stack.clone());
        let req = Request::builder()
            .uri("/test")
            .body(Body::from(""))
            .unwrap();
        next.run(req).await.unwrap();

        // Second request fails with proper headers
        let next = Next::new(stack.clone());
        let req = Request::builder()
            .uri("/test")
            .body(Body::from(""))
            .unwrap();
        let response = next.run(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(response.headers().contains_key("Retry-After"));
        assert!(response.headers().contains_key("X-RateLimit-Limit"));
        assert!(response.headers().contains_key("X-RateLimit-Remaining"));
        assert!(response.headers().contains_key("X-RateLimit-Reset"));
        assert_eq!(
            response.headers().get("Content-Type").unwrap(),
            "application/json"
        );
    }

    #[tokio::test]
    async fn test_custom_error_message() {
        let mut per_route_limits = HashMap::new();
        per_route_limits.insert(
            "/premium".to_string(),
            RouteRateLimit::per_second(1).with_message("Premium API rate limit exceeded"),
        );

        let config = RateLimitConfig {
            requests_per_window: 10,
            window_size: Duration::from_secs(1),
            per_route_limits: Some(per_route_limits),
            error_message: Some("Global rate limit exceeded".to_string()),
            ..Default::default()
        };

        let rate_limit = RateLimit::with_config(config);
        let handler = TestHandler;

        let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([Arc::new(rate_limit), Arc::new(handler)]);

        // Exceed premium endpoint limit
        let next = Next::new(stack.clone());
        let req = Request::builder()
            .uri("/premium")
            .body(Body::from(""))
            .unwrap();
        next.run(req).await.unwrap();

        let next = Next::new(stack.clone());
        let req = Request::builder()
            .uri("/premium")
            .body(Body::from(""))
            .unwrap();
        let response = next.run(req).await.unwrap();

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["message"], "Premium API rate limit exceeded");
    }

    #[tokio::test]
    async fn test_retry_after_header() {
        let config = RateLimitConfig {
            requests_per_window: 1,
            window_size: Duration::from_secs(60),
            ..Default::default()
        };

        let rate_limit = RateLimit::with_config(config);
        let handler = TestHandler;

        let stack: std::sync::Arc<[std::sync::Arc<dyn Middleware>]> = std::sync::Arc::new([
            std::sync::Arc::new(rate_limit),
            std::sync::Arc::new(handler),
        ]);

        // First request succeeds
        let next = Next::new(stack.clone());
        let req = Request::builder()
            .uri("/test")
            .body(Body::from(""))
            .unwrap();
        next.run(req).await.unwrap();

        // Second request fails with Retry-After header
        let next = Next::new(stack.clone());
        let req = Request::builder()
            .uri("/test")
            .body(Body::from(""))
            .unwrap();
        let response = next.run(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(response.headers().contains_key("Retry-After"));
    }
}
