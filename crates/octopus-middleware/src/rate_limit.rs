//! Rate limiting middleware using token bucket algorithm

use crate::auth_gateway::AuthRateLimitKey;
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

/// A shared, unkeyed in-memory governor rate limiter.
type SharedLimiter = Arc<GovernorRateLimiter<NotKeyed, InMemoryState, DefaultClock>>;

/// A map of keys (path or identity) to their dedicated limiters.
type KeyedLimiters = Arc<DashMap<String, SharedLimiter>>;

/// Per-route rate-limit hint attached to a request after route matching.
///
/// The runtime inserts this (from `routes[].rate_limit`) once a route is matched,
/// so the route-aware rate limiter can enforce a per-route window without
/// re-matching. The `key` is the route's path pattern, giving a route-wide cap
/// regardless of the concrete request path (so wildcard routes share one window).
#[derive(Debug, Clone)]
pub struct MatchedRouteRateLimit {
    /// Stable key identifying the route (its path pattern).
    pub key: String,
    /// Maximum requests allowed per window for this route.
    pub requests_per_window: u32,
    /// Window duration.
    pub window_size: Duration,
}

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
    /// Per-identity rate limit (uses AuthRateLimitKey from request extensions)
    Identity,
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
/// Can rate limit globally, per IP, per API key, per path, or per identity.
/// Supports per-route rate limits with different limits for different endpoints.
#[derive(Clone)]
pub struct RateLimit {
    config: RateLimitConfig,
    /// Global rate limiter
    limiter: SharedLimiter,
    /// Per-route rate limiters (path -> limiter)
    route_limiters: KeyedLimiters,
    /// Per-identity rate limiters (identity key -> limiter)
    identity_limiters: KeyedLimiters,
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
            identity_limiters: Arc::new(DashMap::new()),
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
    fn get_limiter_for_request(&self, path: &str) -> (SharedLimiter, Duration, Option<String>) {
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

        // For identity-based rate limiting, use per-identity limiter if available
        let effective_limiter = if self.config.key_extractor == KeyExtractor::Identity {
            if let Some(auth_key) = req.extensions().get::<AuthRateLimitKey>() {
                let key = &auth_key.0;
                self.identity_limiters
                    .entry(key.clone())
                    .or_insert_with(|| {
                        let requests = NonZeroU32::new(self.config.requests_per_window)
                            .unwrap_or_else(|| NonZeroU32::new(1).unwrap());
                        let quota = Quota::with_period(self.config.window_size)
                            .unwrap()
                            .allow_burst(requests);
                        Arc::new(GovernorRateLimiter::direct(quota))
                    })
                    .clone()
            } else {
                // No identity key — fall back to global limiter
                limiter
            }
        } else {
            limiter
        };

        // Check rate limit
        match effective_limiter.check() {
            Ok(_) => {
                // Request allowed, proceed
                next.run(req).await
            }
            Err(_) => {
                // Rate limit exceeded
                let identity_info = if self.config.key_extractor == KeyExtractor::Identity {
                    req.extensions()
                        .get::<AuthRateLimitKey>()
                        .map(|k| k.0.clone())
                        .unwrap_or_default()
                } else {
                    String::new()
                };
                tracing::warn!(
                    uri = %req.uri(),
                    path = %path,
                    identity = %identity_info,
                    "Rate limit exceeded"
                );
                Ok(self.rate_limit_response(window_size, custom_message.as_deref()))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Distributed rate limiting (feature-gated behind "distributed")
// ---------------------------------------------------------------------------

/// Configuration for distributed rate limiting backed by a `StateBackend`.
#[cfg(feature = "distributed")]
#[derive(Debug, Clone)]
pub struct DistributedRateLimitConfig {
    /// Maximum requests allowed per window.
    pub requests_per_window: u32,
    /// Duration of the rate-limit window.
    pub window_size: Duration,
    /// How to derive the rate-limit key from each request.
    pub key_extractor: KeyExtractor,
    /// Header name used when `key_extractor` is `KeyExtractor::Header`.
    pub header_name: Option<String>,
    /// Custom error message returned in 429 responses.
    pub error_message: Option<String>,
    /// Prefix for keys stored in the state backend (default: `"octopus:rl"`).
    pub key_prefix: String,
}

#[cfg(feature = "distributed")]
impl Default for DistributedRateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_window: 1000,
            window_size: Duration::from_secs(60),
            key_extractor: KeyExtractor::Global,
            header_name: None,
            error_message: None,
            key_prefix: "octopus:rl".to_string(),
        }
    }
}

/// Distributed rate-limit middleware that stores counters in a [`StateBackend`].
///
/// Uses a fixed-window algorithm:
/// 1. Compute a window id from the current timestamp.
/// 2. Build a key: `{prefix}:{extractor_value}:{window_id}`.
/// 3. Atomically increment the counter.
/// 4. On the first increment set a TTL so stale windows are cleaned up.
/// 5. If the counter exceeds the limit return **429 Too Many Requests**.
#[cfg(feature = "distributed")]
#[derive(Clone)]
pub struct DistributedRateLimit<B: octopus_state::StateBackend> {
    config: DistributedRateLimitConfig,
    backend: B,
}

#[cfg(feature = "distributed")]
impl<B: octopus_state::StateBackend> fmt::Debug for DistributedRateLimit<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DistributedRateLimit")
            .field("requests_per_window", &self.config.requests_per_window)
            .field("window_size", &self.config.window_size)
            .field("key_extractor", &self.config.key_extractor)
            .field("key_prefix", &self.config.key_prefix)
            .finish()
    }
}

#[cfg(feature = "distributed")]
impl<B: octopus_state::StateBackend> DistributedRateLimit<B> {
    /// Create a new distributed rate limiter.
    pub fn new(config: DistributedRateLimitConfig, backend: B) -> Self {
        Self { config, backend }
    }

    /// Extract the rate-limit key component from the request.
    fn extract_key(&self, req: &Request<Body>) -> String {
        match self.config.key_extractor {
            KeyExtractor::Ip => {
                // Try X-Forwarded-For first, fall back to "unknown"
                req.headers()
                    .get("x-forwarded-for")
                    .and_then(|v| v.to_str().ok())
                    .map(|v| v.split(',').next().unwrap_or("unknown").trim().to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            }
            KeyExtractor::Header => {
                if let Some(ref header_name) = self.config.header_name {
                    req.headers()
                        .get(header_name.as_str())
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("unknown")
                        .to_string()
                } else {
                    "unknown".to_string()
                }
            }
            KeyExtractor::Identity => req
                .extensions()
                .get::<AuthRateLimitKey>()
                .map(|k| k.0.clone())
                .unwrap_or_else(|| "anonymous".to_string()),
            KeyExtractor::Path => req.uri().path().to_string(),
            KeyExtractor::Global => "global".to_string(),
        }
    }

    /// Build a rate-limit error response.
    fn rate_limit_response(&self) -> Response<Body> {
        let message = self
            .config
            .error_message
            .as_deref()
            .unwrap_or("Rate limit exceeded");

        Response::builder()
            .status(StatusCode::TOO_MANY_REQUESTS)
            .header("Content-Type", "application/json")
            .header("Retry-After", self.config.window_size.as_secs().to_string())
            .header(
                "X-RateLimit-Limit",
                self.config.requests_per_window.to_string(),
            )
            .header("X-RateLimit-Remaining", "0")
            .header(
                "X-RateLimit-Reset",
                self.config.window_size.as_secs().to_string(),
            )
            .body(Full::new(Bytes::from(
                serde_json::json!({
                    "error": "rate_limit_exceeded",
                    "message": message,
                    "retry_after": self.config.window_size.as_secs()
                })
                .to_string(),
            )))
            .expect("Failed to build rate limit response")
    }
}

#[cfg(feature = "distributed")]
#[async_trait]
impl<B: octopus_state::StateBackend> Middleware for DistributedRateLimit<B> {
    async fn call(&self, req: Request<Body>, next: Next) -> Result<Response<Body>> {
        let window_secs = self.config.window_size.as_secs().max(1);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let window_id = now / window_secs;
        let extractor_value = self.extract_key(&req);
        let key = format!(
            "{}:{}:{}",
            self.config.key_prefix, extractor_value, window_id
        );

        // The TTL should cover the remainder of this window plus a small buffer so
        // that the key outlives the window even if the increment happens at the very
        // start.
        let ttl = self.config.window_size + Duration::from_secs(5);

        // Atomic increment — the backend creates the key if it does not exist.
        let count = self
            .backend
            .increment(&key, 1, Some(ttl))
            .await
            .map_err(|e| octopus_core::Error::Internal(format!("State backend error: {e}")))?;

        if count > self.config.requests_per_window as i64 {
            tracing::warn!(
                key = %key,
                count = count,
                limit = self.config.requests_per_window,
                "Distributed rate limit exceeded"
            );
            return Ok(self.rate_limit_response());
        }

        next.run(req).await
    }
}

/// Per-route distributed rate limiter.
///
/// Reads [`MatchedRouteRateLimit`] from the request (attached by the runtime from
/// `routes[].rate_limit`) and enforces a fixed window per route using a
/// [`octopus_state::StateBackend`], so the limit holds across replicas. Requests
/// for routes without a rate limit pass through untouched.
#[cfg(feature = "distributed")]
#[derive(Clone)]
pub struct RouteRateLimiter<B: octopus_state::StateBackend> {
    backend: B,
    key_prefix: String,
}

#[cfg(feature = "distributed")]
impl<B: octopus_state::StateBackend> RouteRateLimiter<B> {
    /// Create a route rate limiter backed by `backend`.
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            key_prefix: "octopus:rrl".to_string(),
        }
    }

    /// Build the `429 Too Many Requests` response.
    fn limited_response(window: Duration, limit: u32) -> Response<Body> {
        Response::builder()
            .status(StatusCode::TOO_MANY_REQUESTS)
            .header("Content-Type", "application/json")
            .header("Retry-After", window.as_secs().to_string())
            .header("X-RateLimit-Limit", limit.to_string())
            .header("X-RateLimit-Remaining", "0")
            .header("X-RateLimit-Reset", window.as_secs().to_string())
            .body(Full::new(Bytes::from(
                serde_json::json!({
                    "error": "rate_limit_exceeded",
                    "message": "Rate limit exceeded",
                    "retry_after": window.as_secs()
                })
                .to_string(),
            )))
            .expect("Failed to build rate limit response")
    }
}

#[cfg(feature = "distributed")]
impl<B: octopus_state::StateBackend> fmt::Debug for RouteRateLimiter<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RouteRateLimiter")
            .field("key_prefix", &self.key_prefix)
            .finish()
    }
}

#[cfg(feature = "distributed")]
#[async_trait]
impl<B: octopus_state::StateBackend> Middleware for RouteRateLimiter<B> {
    async fn call(&self, req: Request<Body>, next: Next) -> Result<Response<Body>> {
        if let Some(rl) = req.extensions().get::<MatchedRouteRateLimit>().cloned() {
            let window_secs = rl.window_size.as_secs().max(1);
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let window_id = now / window_secs;
            let key = format!("{}:{}:{}", self.key_prefix, rl.key, window_id);
            let ttl = rl.window_size + Duration::from_secs(5);

            let count = self
                .backend
                .increment(&key, 1, Some(ttl))
                .await
                .map_err(|e| octopus_core::Error::Internal(format!("State backend error: {e}")))?;

            if count > rl.requests_per_window as i64 {
                tracing::warn!(
                    route = %rl.key,
                    count,
                    limit = rl.requests_per_window,
                    "Per-route rate limit exceeded"
                );
                return Ok(Self::limited_response(
                    rl.window_size,
                    rl.requests_per_window,
                ));
            }
        }
        next.run(req).await
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

    // -----------------------------------------------------------------------
    // Distributed rate limit tests (use in-memory backend)
    // -----------------------------------------------------------------------

    #[cfg(feature = "distributed")]
    mod distributed_tests {
        use super::*;
        use crate::rate_limit::{DistributedRateLimit, DistributedRateLimitConfig};
        use octopus_state::InMemoryBackend;

        #[tokio::test]
        async fn test_distributed_rate_limit_allows_within_window() {
            let backend = InMemoryBackend::new();
            let config = DistributedRateLimitConfig {
                requests_per_window: 5,
                window_size: Duration::from_secs(60),
                key_extractor: KeyExtractor::Global,
                key_prefix: "test:rl".to_string(),
                ..Default::default()
            };

            let rl = DistributedRateLimit::new(config, backend);
            let handler = TestHandler;

            let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([Arc::new(rl), Arc::new(handler)]);

            // First 5 requests should succeed
            for _ in 0..5 {
                let next = Next::new(stack.clone());
                let req = Request::builder()
                    .uri("/test")
                    .body(Body::from(""))
                    .unwrap();
                let response = next.run(req).await.unwrap();
                assert_eq!(response.status(), StatusCode::OK);
            }

            // 6th request should be rate-limited
            let next = Next::new(stack.clone());
            let req = Request::builder()
                .uri("/test")
                .body(Body::from(""))
                .unwrap();
            let response = next.run(req).await.unwrap();
            assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        }

        #[tokio::test]
        async fn test_distributed_rate_limit_per_ip() {
            let backend = InMemoryBackend::new();
            let config = DistributedRateLimitConfig {
                requests_per_window: 2,
                window_size: Duration::from_secs(60),
                key_extractor: KeyExtractor::Ip,
                key_prefix: "test:rl:ip".to_string(),
                ..Default::default()
            };

            let rl = DistributedRateLimit::new(config, backend);
            let handler = TestHandler;

            let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([Arc::new(rl), Arc::new(handler)]);

            // 2 requests from IP-A should be fine
            for _ in 0..2 {
                let next = Next::new(stack.clone());
                let req = Request::builder()
                    .uri("/test")
                    .header("x-forwarded-for", "10.0.0.1")
                    .body(Body::from(""))
                    .unwrap();
                let response = next.run(req).await.unwrap();
                assert_eq!(response.status(), StatusCode::OK);
            }

            // 3rd request from IP-A should fail
            let next = Next::new(stack.clone());
            let req = Request::builder()
                .uri("/test")
                .header("x-forwarded-for", "10.0.0.1")
                .body(Body::from(""))
                .unwrap();
            let response = next.run(req).await.unwrap();
            assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);

            // But IP-B should still be allowed
            let next = Next::new(stack.clone());
            let req = Request::builder()
                .uri("/test")
                .header("x-forwarded-for", "10.0.0.2")
                .body(Body::from(""))
                .unwrap();
            let response = next.run(req).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }

        #[tokio::test]
        async fn test_distributed_rate_limit_custom_error() {
            let backend = InMemoryBackend::new();
            let config = DistributedRateLimitConfig {
                requests_per_window: 1,
                window_size: Duration::from_secs(60),
                key_extractor: KeyExtractor::Global,
                key_prefix: "test:rl:err".to_string(),
                error_message: Some("Custom limit hit".to_string()),
                ..Default::default()
            };

            let rl = DistributedRateLimit::new(config, backend);
            let handler = TestHandler;

            let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([Arc::new(rl), Arc::new(handler)]);

            // Exhaust the limit
            let next = Next::new(stack.clone());
            let req = Request::builder()
                .uri("/test")
                .body(Body::from(""))
                .unwrap();
            next.run(req).await.unwrap();

            // Next should return 429 with custom message
            let next = Next::new(stack.clone());
            let req = Request::builder()
                .uri("/test")
                .body(Body::from(""))
                .unwrap();
            let response = next.run(req).await.unwrap();
            assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);

            let body = response.into_body().collect().await.unwrap().to_bytes();
            let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(json["message"], "Custom limit hit");
        }

        use crate::rate_limit::{MatchedRouteRateLimit, RouteRateLimiter};

        fn rl_ext(key: &str, limit: u32, window: Duration) -> MatchedRouteRateLimit {
            MatchedRouteRateLimit {
                key: key.to_string(),
                requests_per_window: limit,
                window_size: window,
            }
        }

        #[tokio::test]
        async fn test_route_rate_limit_enforces_per_route_window() {
            let rl = RouteRateLimiter::new(InMemoryBackend::new());
            let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([Arc::new(rl), Arc::new(TestHandler)]);

            // limit 2/60s for route "/api/*"; concrete paths share the route window.
            for path in ["/api/users/1", "/api/users/2"] {
                let next = Next::new(stack.clone());
                let mut req = Request::builder().uri(path).body(Body::from("")).unwrap();
                req.extensions_mut()
                    .insert(rl_ext("/api/*", 2, Duration::from_secs(60)));
                assert_eq!(next.run(req).await.unwrap().status(), StatusCode::OK);
            }

            let next = Next::new(stack.clone());
            let mut req = Request::builder()
                .uri("/api/users/3")
                .body(Body::from(""))
                .unwrap();
            req.extensions_mut()
                .insert(rl_ext("/api/*", 2, Duration::from_secs(60)));
            let resp = next.run(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
            assert!(resp.headers().contains_key("Retry-After"));
        }

        #[tokio::test]
        async fn test_route_rate_limit_passes_through_without_extension() {
            let rl = RouteRateLimiter::new(InMemoryBackend::new());
            let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([Arc::new(rl), Arc::new(TestHandler)]);

            for _ in 0..10 {
                let next = Next::new(stack.clone());
                let req = Request::builder().uri("/x").body(Body::from("")).unwrap();
                assert_eq!(next.run(req).await.unwrap().status(), StatusCode::OK);
            }
        }

        #[tokio::test]
        async fn test_route_rate_limit_independent_per_route_key() {
            let rl = RouteRateLimiter::new(InMemoryBackend::new());
            let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([Arc::new(rl), Arc::new(TestHandler)]);

            // Exhaust /a (limit 1).
            for expected in [StatusCode::OK, StatusCode::TOO_MANY_REQUESTS] {
                let next = Next::new(stack.clone());
                let mut req = Request::builder().uri("/a").body(Body::from("")).unwrap();
                req.extensions_mut()
                    .insert(rl_ext("/a", 1, Duration::from_secs(60)));
                assert_eq!(next.run(req).await.unwrap().status(), expected);
            }

            // /b has its own window.
            let next = Next::new(stack.clone());
            let mut req = Request::builder().uri("/b").body(Body::from("")).unwrap();
            req.extensions_mut()
                .insert(rl_ext("/b", 1, Duration::from_secs(60)));
            assert_eq!(next.run(req).await.unwrap().status(), StatusCode::OK);
        }
    }
}
