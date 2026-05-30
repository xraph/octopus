//! Circuit breaker middleware
//!
//! Implements the Closed -> Open -> Half-Open pattern per upstream.
//! Uses `DashMap` for lock-free concurrent access to per-upstream state.

use crate::auth_gateway::MatchedRouteAuth;
use async_trait::async_trait;
use bytes::Bytes;
use dashmap::DashMap;
use http::{Request, Response, StatusCode};
use http_body_util::Full;
use octopus_core::{Middleware, Next, Result};
use std::fmt;
use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Body type alias
pub type Body = Full<Bytes>;

// State constants
const STATE_CLOSED: u8 = 0;
const STATE_OPEN: u8 = 1;
const STATE_HALF_OPEN: u8 = 2;

/// Circuit breaker configuration
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Number of failures in the rolling window before the circuit opens
    pub failure_threshold: u32,
    /// Number of consecutive successes in Half-Open state to close the circuit
    pub success_threshold: u32,
    /// Duration to stay in Open state before transitioning to Half-Open
    pub open_timeout: Duration,
    /// HTTP status codes considered failures
    pub failure_status_codes: Vec<u16>,
    /// Rolling window for counting failures
    pub window_size: Duration,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            success_threshold: 2,
            open_timeout: Duration::from_secs(30),
            failure_status_codes: (500..=599).collect(),
            window_size: Duration::from_secs(60),
        }
    }
}

/// Per-upstream circuit state
pub struct CircuitState {
    /// Current state: 0=Closed, 1=Open, 2=Half-Open
    pub state: AtomicU8,
    /// Failure count in current window (Closed) or probe failures (Half-Open)
    pub failure_count: AtomicU32,
    /// Consecutive success count in Half-Open state
    pub success_count: AtomicU32,
    /// Timestamp of the last recorded failure
    pub last_failure_time: parking_lot::Mutex<Option<Instant>>,
    /// Timestamp when the circuit was opened
    pub opened_at: parking_lot::Mutex<Option<Instant>>,
    /// Start of the current rolling failure window
    window_start: parking_lot::Mutex<Instant>,
}

impl CircuitState {
    fn new() -> Self {
        Self {
            state: AtomicU8::new(STATE_CLOSED),
            failure_count: AtomicU32::new(0),
            success_count: AtomicU32::new(0),
            last_failure_time: parking_lot::Mutex::new(None),
            opened_at: parking_lot::Mutex::new(None),
            window_start: parking_lot::Mutex::new(Instant::now()),
        }
    }

    fn current_state(&self) -> u8 {
        self.state.load(Ordering::SeqCst)
    }

    /// Record a failure. Returns `true` if the circuit should transition to Open.
    fn record_failure(&self, threshold: u32, window_size: Duration) -> bool {
        let now = Instant::now();
        *self.last_failure_time.lock() = Some(now);

        // Check if the rolling window has expired and reset if necessary
        {
            let mut ws = self.window_start.lock();
            if now.duration_since(*ws) > window_size {
                *ws = now;
                self.failure_count.store(0, Ordering::SeqCst);
            }
        }

        let count = self.failure_count.fetch_add(1, Ordering::SeqCst) + 1;
        count >= threshold
    }

    /// Transition to Open state
    fn open(&self) {
        self.state.store(STATE_OPEN, Ordering::SeqCst);
        *self.opened_at.lock() = Some(Instant::now());
        self.success_count.store(0, Ordering::SeqCst);
        tracing::warn!("Circuit breaker opened");
    }

    /// Try to transition from Open to Half-Open if the timeout has elapsed.
    /// Returns `true` if the transition happened (this caller is the probe).
    fn try_half_open(&self, open_timeout: Duration) -> bool {
        if self.current_state() != STATE_OPEN {
            return false;
        }
        let opened = self.opened_at.lock();
        if let Some(at) = *opened {
            if Instant::now().duration_since(at) >= open_timeout {
                // CAS to ensure only one probe at a time
                let prev = self
                    .state
                    .compare_exchange(STATE_OPEN, STATE_HALF_OPEN, Ordering::SeqCst, Ordering::SeqCst);
                return prev.is_ok();
            }
        }
        false
    }

    /// Record a success in Half-Open state. Returns `true` if the circuit
    /// should transition back to Closed.
    fn record_half_open_success(&self, threshold: u32) -> bool {
        let count = self.success_count.fetch_add(1, Ordering::SeqCst) + 1;
        count >= threshold
    }

    /// Reset to Closed state
    fn close(&self) {
        self.state.store(STATE_CLOSED, Ordering::SeqCst);
        self.failure_count.store(0, Ordering::SeqCst);
        self.success_count.store(0, Ordering::SeqCst);
        *self.window_start.lock() = Instant::now();
        tracing::info!("Circuit breaker closed");
    }
}

impl fmt::Debug for CircuitState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state_name = match self.current_state() {
            STATE_CLOSED => "Closed",
            STATE_OPEN => "Open",
            STATE_HALF_OPEN => "HalfOpen",
            _ => "Unknown",
        };
        f.debug_struct("CircuitState")
            .field("state", &state_name)
            .field("failure_count", &self.failure_count.load(Ordering::Relaxed))
            .field("success_count", &self.success_count.load(Ordering::Relaxed))
            .finish()
    }
}

/// Circuit breaker middleware
///
/// Tracks per-upstream failure rates and short-circuits requests when an
/// upstream is unhealthy, giving it time to recover.
#[derive(Clone)]
pub struct CircuitBreaker {
    config: CircuitBreakerConfig,
    states: Arc<DashMap<String, Arc<CircuitState>>>,
}

impl CircuitBreaker {
    /// Create a new circuit breaker middleware with default configuration
    pub fn new() -> Self {
        Self::with_config(CircuitBreakerConfig::default())
    }

    /// Create a new circuit breaker middleware with custom configuration
    pub fn with_config(config: CircuitBreakerConfig) -> Self {
        Self {
            config,
            states: Arc::new(DashMap::new()),
        }
    }

    /// Get or create the circuit state for a given upstream
    fn get_state(&self, upstream: &str) -> Arc<CircuitState> {
        self.states
            .entry(upstream.to_string())
            .or_insert_with(|| Arc::new(CircuitState::new()))
            .value()
            .clone()
    }

    /// Check if a response status code counts as a failure
    fn is_failure_status(&self, status: StatusCode) -> bool {
        self.config
            .failure_status_codes
            .contains(&status.as_u16())
    }

    /// Build a 503 Service Unavailable response for an open circuit
    fn open_circuit_response(upstream: &str) -> Response<Body> {
        let body = serde_json::json!({
            "error": "circuit_breaker_open",
            "message": format!("Circuit breaker is open for upstream '{}'", upstream),
            "upstream": upstream
        });

        Response::builder()
            .status(StatusCode::SERVICE_UNAVAILABLE)
            .header("Content-Type", "application/json")
            .header("X-Circuit-State", "open")
            .body(Full::new(Bytes::from(body.to_string())))
            .expect("failed to build circuit breaker response")
    }
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for CircuitBreaker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CircuitBreaker")
            .field("failure_threshold", &self.config.failure_threshold)
            .field("success_threshold", &self.config.success_threshold)
            .field("open_timeout", &self.config.open_timeout)
            .finish()
    }
}

#[async_trait]
impl Middleware for CircuitBreaker {
    async fn call(&self, req: Request<Body>, next: Next) -> Result<Response<Body>> {
        // Determine upstream name from request extensions
        let upstream = req
            .extensions()
            .get::<MatchedRouteAuth>()
            .map(|m| m.upstream.clone())
            .unwrap_or_else(|| "default".to_string());

        let state = self.get_state(&upstream);

        match state.current_state() {
            STATE_CLOSED => {
                // Normal operation -- forward the request
                let response = next.run(req).await?;

                if self.is_failure_status(response.status()) {
                    let should_open = state.record_failure(
                        self.config.failure_threshold,
                        self.config.window_size,
                    );
                    if should_open {
                        tracing::warn!(upstream = %upstream, "Failure threshold reached, opening circuit");
                        state.open();
                    }
                }

                Ok(response)
            }
            STATE_OPEN => {
                // Check if open_timeout elapsed -> try Half-Open
                if state.try_half_open(self.config.open_timeout) {
                    tracing::info!(upstream = %upstream, "Circuit transitioning to half-open, sending probe");
                    let response = next.run(req).await?;

                    if self.is_failure_status(response.status()) {
                        tracing::warn!(upstream = %upstream, "Half-open probe failed, re-opening circuit");
                        state.open();
                    } else if state.record_half_open_success(self.config.success_threshold) {
                        tracing::info!(upstream = %upstream, "Half-open success threshold reached, closing circuit");
                        state.close();
                    }

                    Ok(response)
                } else {
                    // Still in Open state -- reject immediately
                    tracing::debug!(upstream = %upstream, "Circuit open, rejecting request");
                    Ok(Self::open_circuit_response(&upstream))
                }
            }
            STATE_HALF_OPEN => {
                // Allow the probe request through
                let response = next.run(req).await?;

                if self.is_failure_status(response.status()) {
                    tracing::warn!(upstream = %upstream, "Half-open request failed, re-opening circuit");
                    state.open();
                } else if state.record_half_open_success(self.config.success_threshold) {
                    tracing::info!(upstream = %upstream, "Half-open success threshold reached, closing circuit");
                    state.close();
                }

                Ok(response)
            }
            _ => {
                // Unknown state -- treat as closed
                next.run(req).await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use octopus_core::Error;
    use std::collections::HashMap;
    use std::sync::atomic::AtomicU32;

    #[derive(Debug)]
    struct StatusHandler(StatusCode);

    #[async_trait]
    impl Middleware for StatusHandler {
        async fn call(&self, _req: Request<Body>, _next: Next) -> Result<Response<Body>> {
            Response::builder()
                .status(self.0)
                .body(Full::new(Bytes::from("response")))
                .map_err(|e| Error::Internal(e.to_string()))
        }
    }

    /// Handler that returns a configurable status for the first N calls, then
    /// switches to a different status.
    #[derive(Debug)]
    struct ToggleHandler {
        initial_status: StatusCode,
        then_status: StatusCode,
        switch_after: u32,
        counter: Arc<AtomicU32>,
    }

    #[async_trait]
    impl Middleware for ToggleHandler {
        async fn call(&self, _req: Request<Body>, _next: Next) -> Result<Response<Body>> {
            let n = self.counter.fetch_add(1, Ordering::SeqCst);
            let status = if n < self.switch_after {
                self.initial_status
            } else {
                self.then_status
            };
            Response::builder()
                .status(status)
                .body(Full::new(Bytes::from("response")))
                .map_err(|e| Error::Internal(e.to_string()))
        }
    }

    fn make_request(upstream: &str) -> Request<Body> {
        let mut req = Request::builder()
            .uri("/test")
            .body(Body::from(""))
            .unwrap();
        req.extensions_mut().insert(MatchedRouteAuth {
            auth_provider: None,
            skip_auth: true,
            require_roles: vec![],
            require_scopes: vec![],
            authz_rule: None,
            upstream: upstream.to_string(),
            metadata: HashMap::new(),
        });
        req
    }

    #[tokio::test]
    async fn test_circuit_opens_after_failure_threshold() {
        let config = CircuitBreakerConfig {
            failure_threshold: 3,
            success_threshold: 1,
            open_timeout: Duration::from_secs(60),
            failure_status_codes: vec![500],
            window_size: Duration::from_secs(60),
        };
        let cb = CircuitBreaker::with_config(config);
        let handler = StatusHandler(StatusCode::INTERNAL_SERVER_ERROR);

        let stack: Arc<[Arc<dyn Middleware>]> =
            Arc::new([Arc::new(cb.clone()), Arc::new(handler)]);

        // Send failure_threshold requests to trigger the circuit open
        for _ in 0..3 {
            let next = Next::new(stack.clone());
            let req = make_request("svc-a");
            let resp = next.run(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        }

        // The circuit should now be Open; next request gets 503
        let next = Next::new(stack.clone());
        let req = make_request("svc-a");
        let resp = next.run(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            resp.headers().get("X-Circuit-State").unwrap(),
            "open"
        );
    }

    #[tokio::test]
    async fn test_circuit_recovers_through_half_open() {
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            success_threshold: 1,
            open_timeout: Duration::from_millis(50), // very short for testing
            failure_status_codes: vec![500],
            window_size: Duration::from_secs(60),
        };
        let cb = CircuitBreaker::with_config(config);

        let counter = Arc::new(AtomicU32::new(0));
        let handler = ToggleHandler {
            initial_status: StatusCode::INTERNAL_SERVER_ERROR,
            then_status: StatusCode::OK,
            switch_after: 2, // first 2 calls fail, then succeed
            counter: counter.clone(),
        };

        let stack: Arc<[Arc<dyn Middleware>]> =
            Arc::new([Arc::new(cb.clone()), Arc::new(handler)]);

        // Trigger failures to open the circuit
        for _ in 0..2 {
            let next = Next::new(stack.clone());
            let req = make_request("svc-b");
            next.run(req).await.unwrap();
        }

        // Circuit is open; request should be rejected
        let next = Next::new(stack.clone());
        let req = make_request("svc-b");
        let resp = next.run(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);

        // Wait for open_timeout to elapse
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Next request should be the half-open probe, which succeeds
        let next = Next::new(stack.clone());
        let req = make_request("svc-b");
        let resp = next.run(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Circuit should now be Closed; confirm state
        let state = cb.get_state("svc-b");
        assert_eq!(state.current_state(), STATE_CLOSED);
    }

    #[tokio::test]
    async fn test_different_upstreams_have_independent_circuits() {
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            failure_status_codes: vec![500],
            ..Default::default()
        };
        let cb = CircuitBreaker::with_config(config);
        let handler = StatusHandler(StatusCode::INTERNAL_SERVER_ERROR);

        let stack: Arc<[Arc<dyn Middleware>]> =
            Arc::new([Arc::new(cb.clone()), Arc::new(handler)]);

        // Trip the circuit for upstream-1
        for _ in 0..2 {
            let next = Next::new(stack.clone());
            let req = make_request("upstream-1");
            next.run(req).await.unwrap();
        }

        // upstream-1 is open
        let next = Next::new(stack.clone());
        let req = make_request("upstream-1");
        let resp = next.run(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);

        // upstream-2 is still closed (failures go through)
        let next = Next::new(stack.clone());
        let req = make_request("upstream-2");
        let resp = next.run(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
