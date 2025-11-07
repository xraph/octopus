//! Circuit breaker pattern implementation

use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

/// Circuit breaker state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    /// Circuit is closed, requests pass through normally
    Closed,
    /// Circuit is open, all requests fail immediately
    Open,
    /// Circuit is half-open, allowing limited test requests
    HalfOpen,
}

impl std::fmt::Display for CircuitState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CircuitState::Closed => write!(f, "closed"),
            CircuitState::Open => write!(f, "open"),
            CircuitState::HalfOpen => write!(f, "half-open"),
        }
    }
}

/// Circuit breaker configuration
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Failure threshold (0.0 to 1.0) to open the circuit
    pub failure_threshold: f64,
    /// Minimum requests before opening circuit
    pub min_requests: u64,
    /// Duration to stay in open state before transitioning to half-open
    pub open_timeout: Duration,
    /// Maximum number of requests allowed in half-open state
    pub half_open_max_requests: u32,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 0.5, // 50% error rate
            min_requests: 10,
            open_timeout: Duration::from_secs(30),
            half_open_max_requests: 5,
        }
    }
}

/// Circuit breaker for a single upstream instance
#[derive(Debug)]
struct CircuitBreakerInstance {
    config: CircuitBreakerConfig,
    state: parking_lot::RwLock<CircuitState>,
    failure_count: AtomicU64,
    success_count: AtomicU64,
    total_count: AtomicU64,
    half_open_requests: AtomicU64,
    state_change_time: parking_lot::Mutex<Instant>,
}

impl CircuitBreakerInstance {
    /// Create a new circuit breaker instance
    fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            config,
            state: parking_lot::RwLock::new(CircuitState::Closed),
            failure_count: AtomicU64::new(0),
            success_count: AtomicU64::new(0),
            total_count: AtomicU64::new(0),
            half_open_requests: AtomicU64::new(0),
            state_change_time: parking_lot::Mutex::new(Instant::now()),
        }
    }

    /// Get the current state
    fn state(&self) -> CircuitState {
        *self.state.read()
    }

    /// Record a successful request
    fn record_success(&self) {
        self.success_count.fetch_add(1, Ordering::Relaxed);
        self.total_count.fetch_add(1, Ordering::Relaxed);

        let current_state = self.state();

        if current_state == CircuitState::HalfOpen {
            let success = self.success_count.load(Ordering::Relaxed);
            let total = self.total_count.load(Ordering::Relaxed);

            // If success rate is good in half-open, close the circuit
            if total >= self.config.half_open_max_requests as u64 {
                let success_rate = success as f64 / total as f64;
                if success_rate >= (1.0 - self.config.failure_threshold) {
                    self.transition_to_closed();
                }
            }
        }

        debug!(
            state = %current_state,
            success = self.success_count.load(Ordering::Relaxed),
            total = self.total_count.load(Ordering::Relaxed),
            "Circuit breaker recorded success"
        );
    }

    /// Record a failed request
    fn record_failure(&self) {
        self.failure_count.fetch_add(1, Ordering::Relaxed);
        self.total_count.fetch_add(1, Ordering::Relaxed);

        let current_state = self.state();
        let total = self.total_count.load(Ordering::Relaxed);

        // Check if we should open the circuit
        if total >= self.config.min_requests {
            let failure_rate = self.failure_count.load(Ordering::Relaxed) as f64 / total as f64;

            if failure_rate >= self.config.failure_threshold {
                match current_state {
                    CircuitState::Closed => {
                        self.transition_to_open();
                    }
                    CircuitState::HalfOpen => {
                        // If failures occur in half-open, go back to open
                        self.transition_to_open();
                    }
                    _ => {}
                }
            }
        }

        warn!(
            state = %current_state,
            failures = self.failure_count.load(Ordering::Relaxed),
            total = self.total_count.load(Ordering::Relaxed),
            "Circuit breaker recorded failure"
        );
    }

    /// Check if a request should be allowed
    fn allow_request(&self) -> bool {
        let current_state = self.state();

        match current_state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                // Check if enough time has passed to transition to half-open
                let time_since_open = self.state_change_time.lock().elapsed();
                if time_since_open >= self.config.open_timeout {
                    self.transition_to_half_open();
                    true
                } else {
                    false
                }
            }
            CircuitState::HalfOpen => {
                // Allow limited requests in half-open state
                let current_requests = self.half_open_requests.fetch_add(1, Ordering::Relaxed);
                current_requests < self.config.half_open_max_requests as u64
            }
        }
    }

    /// Transition to open state
    fn transition_to_open(&self) {
        let mut state = self.state.write();
        if *state != CircuitState::Open {
            *state = CircuitState::Open;
            *self.state_change_time.lock() = Instant::now();
            warn!("Circuit breaker transitioned to OPEN");
        }
    }

    /// Transition to half-open state
    fn transition_to_half_open(&self) {
        let mut state = self.state.write();
        if *state != CircuitState::HalfOpen {
            *state = CircuitState::HalfOpen;
            *self.state_change_time.lock() = Instant::now();
            self.half_open_requests.store(0, Ordering::Relaxed);
            // Reset counters for half-open testing
            self.success_count.store(0, Ordering::Relaxed);
            self.failure_count.store(0, Ordering::Relaxed);
            self.total_count.store(0, Ordering::Relaxed);
            info!("Circuit breaker transitioned to HALF-OPEN");
        }
    }

    /// Transition to closed state
    fn transition_to_closed(&self) {
        let mut state = self.state.write();
        if *state != CircuitState::Closed {
            *state = CircuitState::Closed;
            *self.state_change_time.lock() = Instant::now();
            // Reset counters
            self.success_count.store(0, Ordering::Relaxed);
            self.failure_count.store(0, Ordering::Relaxed);
            self.total_count.store(0, Ordering::Relaxed);
            self.half_open_requests.store(0, Ordering::Relaxed);
            info!("Circuit breaker transitioned to CLOSED");
        }
    }

    /// Reset the circuit breaker
    fn reset(&self) {
        *self.state.write() = CircuitState::Closed;
        *self.state_change_time.lock() = Instant::now();
        self.success_count.store(0, Ordering::Relaxed);
        self.failure_count.store(0, Ordering::Relaxed);
        self.total_count.store(0, Ordering::Relaxed);
        self.half_open_requests.store(0, Ordering::Relaxed);
        info!("Circuit breaker reset");
    }

    /// Get current metrics
    fn metrics(&self) -> CircuitBreakerMetrics {
        CircuitBreakerMetrics {
            state: self.state(),
            success_count: self.success_count.load(Ordering::Relaxed),
            failure_count: self.failure_count.load(Ordering::Relaxed),
            total_count: self.total_count.load(Ordering::Relaxed),
            failure_rate: if self.total_count.load(Ordering::Relaxed) > 0 {
                self.failure_count.load(Ordering::Relaxed) as f64
                    / self.total_count.load(Ordering::Relaxed) as f64
            } else {
                0.0
            },
        }
    }
}

/// Circuit breaker metrics
#[derive(Debug, Clone)]
pub struct CircuitBreakerMetrics {
    /// Current state
    pub state: CircuitState,
    /// Number of successful requests
    pub success_count: u64,
    /// Number of failed requests
    pub failure_count: u64,
    /// Total number of requests
    pub total_count: u64,
    /// Failure rate (0.0 to 1.0)
    pub failure_rate: f64,
}

/// Circuit breaker manager for multiple upstream instances
#[derive(Debug, Clone)]
pub struct CircuitBreaker {
    config: CircuitBreakerConfig,
    instances: Arc<DashMap<String, Arc<CircuitBreakerInstance>>>,
}

impl CircuitBreaker {
    /// Create a new circuit breaker manager
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            config,
            instances: Arc::new(DashMap::new()),
        }
    }

    /// Create with default configuration
    pub fn default_config() -> Self {
        Self::new(CircuitBreakerConfig::default())
    }

    /// Get or create circuit breaker for an instance
    fn get_or_create(&self, instance_id: &str) -> Arc<CircuitBreakerInstance> {
        self.instances
            .entry(instance_id.to_string())
            .or_insert_with(|| Arc::new(CircuitBreakerInstance::new(self.config.clone())))
            .clone()
    }

    /// Check if a request to an instance should be allowed
    pub fn allow_request(&self, instance_id: &str) -> bool {
        let instance = self.get_or_create(instance_id);
        instance.allow_request()
    }

    /// Record a successful request
    pub fn record_success(&self, instance_id: &str) {
        let instance = self.get_or_create(instance_id);
        instance.record_success();
    }

    /// Record a failed request
    pub fn record_failure(&self, instance_id: &str) {
        let instance = self.get_or_create(instance_id);
        instance.record_failure();
    }

    /// Get the state of a circuit breaker
    pub fn get_state(&self, instance_id: &str) -> CircuitState {
        self.instances
            .get(instance_id)
            .map(|inst| inst.state())
            .unwrap_or(CircuitState::Closed)
    }

    /// Get metrics for an instance
    pub fn get_metrics(&self, instance_id: &str) -> Option<CircuitBreakerMetrics> {
        self.instances.get(instance_id).map(|inst| inst.metrics())
    }

    /// Get metrics for all instances
    pub fn get_all_metrics(&self) -> Vec<(String, CircuitBreakerMetrics)> {
        self.instances
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().metrics()))
            .collect()
    }

    /// Reset a circuit breaker
    pub fn reset(&self, instance_id: &str) {
        if let Some(instance) = self.instances.get(instance_id) {
            instance.reset();
        }
    }

    /// Reset all circuit breakers
    pub fn reset_all(&self) {
        for entry in self.instances.iter() {
            entry.value().reset();
        }
        info!("All circuit breakers reset");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn test_circuit_breaker_closed_to_open() {
        let config = CircuitBreakerConfig {
            failure_threshold: 0.5,
            min_requests: 5,
            open_timeout: Duration::from_secs(1),
            half_open_max_requests: 3,
        };

        let breaker = CircuitBreaker::new(config);
        let instance_id = "test-instance";

        // Initially closed
        assert_eq!(breaker.get_state(instance_id), CircuitState::Closed);
        assert!(breaker.allow_request(instance_id));

        // Record requests to reach threshold
        breaker.record_success(instance_id);
        breaker.record_success(instance_id);
        breaker.record_failure(instance_id);
        breaker.record_failure(instance_id);
        breaker.record_failure(instance_id);

        // Should transition to open (3/5 = 60% failure rate > 50% threshold)
        assert_eq!(breaker.get_state(instance_id), CircuitState::Open);
        assert!(!breaker.allow_request(instance_id));
    }

    #[test]
    fn test_circuit_breaker_open_to_half_open() {
        let config = CircuitBreakerConfig {
            failure_threshold: 0.5,
            min_requests: 5,
            open_timeout: Duration::from_millis(100),
            half_open_max_requests: 3,
        };

        let breaker = CircuitBreaker::new(config);
        let instance_id = "test-instance";

        // Force open state
        for _ in 0..5 {
            breaker.record_failure(instance_id);
        }
        assert_eq!(breaker.get_state(instance_id), CircuitState::Open);

        // Wait for open timeout
        sleep(Duration::from_millis(150));

        // Should allow request and transition to half-open
        assert!(breaker.allow_request(instance_id));
        assert_eq!(breaker.get_state(instance_id), CircuitState::HalfOpen);
    }

    #[test]
    fn test_circuit_breaker_half_open_to_closed() {
        let config = CircuitBreakerConfig {
            failure_threshold: 0.5,
            min_requests: 3,
            open_timeout: Duration::from_millis(100),
            half_open_max_requests: 3,
        };

        let breaker = CircuitBreaker::new(config);
        let instance_id = "test-instance";

        // Force open state
        for _ in 0..5 {
            breaker.record_failure(instance_id);
        }

        // Wait and transition to half-open
        sleep(Duration::from_millis(150));
        breaker.allow_request(instance_id);
        assert_eq!(breaker.get_state(instance_id), CircuitState::HalfOpen);

        // Record successes in half-open
        for _ in 0..3 {
            breaker.record_success(instance_id);
        }

        // Should transition back to closed
        assert_eq!(breaker.get_state(instance_id), CircuitState::Closed);
    }

    #[test]
    fn test_circuit_breaker_metrics() {
        let breaker = CircuitBreaker::default_config();
        let instance_id = "test-instance";

        breaker.record_success(instance_id);
        breaker.record_success(instance_id);
        breaker.record_failure(instance_id);

        let metrics = breaker.get_metrics(instance_id).unwrap();
        assert_eq!(metrics.success_count, 2);
        assert_eq!(metrics.failure_count, 1);
        assert_eq!(metrics.total_count, 3);
        assert_eq!(metrics.failure_rate, 1.0 / 3.0);
    }

    #[test]
    fn test_circuit_breaker_reset() {
        let breaker = CircuitBreaker::default_config();
        let instance_id = "test-instance";

        breaker.record_failure(instance_id);
        breaker.record_failure(instance_id);

        breaker.reset(instance_id);

        let metrics = breaker.get_metrics(instance_id).unwrap();
        assert_eq!(metrics.total_count, 0);
        assert_eq!(breaker.get_state(instance_id), CircuitState::Closed);
    }

    #[test]
    fn test_circuit_state_display() {
        assert_eq!(format!("{}", CircuitState::Closed), "closed");
        assert_eq!(format!("{}", CircuitState::Open), "open");
        assert_eq!(format!("{}", CircuitState::HalfOpen), "half-open");
    }
}
