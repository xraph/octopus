//! Comprehensive metrics for proxy operations

use octopus_metrics::{MetricsCollector, RequestOutcome};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::debug;

/// Proxy-specific metrics collector
#[derive(Clone)]
pub struct ProxyMetrics {
    /// Underlying metrics collector
    collector: Arc<MetricsCollector>,
    
    /// Connection pool metrics
    pool_metrics: Arc<PoolMetrics>,
    
    /// Circuit breaker metrics
    circuit_breaker_metrics: Arc<CircuitBreakerMetrics>,
    
    /// Retry metrics
    retry_metrics: Arc<RetryMetrics>,
    
    /// TLS metrics
    tls_metrics: Arc<TlsMetrics>,
}

impl ProxyMetrics {
    /// Create a new proxy metrics collector
    pub fn new(collector: Arc<MetricsCollector>) -> Self {
        Self {
            collector,
            pool_metrics: Arc::new(PoolMetrics::new()),
            circuit_breaker_metrics: Arc::new(CircuitBreakerMetrics::new()),
            retry_metrics: Arc::new(RetryMetrics::new()),
            tls_metrics: Arc::new(TlsMetrics::new()),
        }
    }

    /// Get the underlying metrics collector
    pub fn collector(&self) -> &Arc<MetricsCollector> {
        &self.collector
    }

    /// Get pool metrics
    pub fn pool_metrics(&self) -> &Arc<PoolMetrics> {
        &self.pool_metrics
    }

    /// Get circuit breaker metrics
    pub fn circuit_breaker_metrics(&self) -> &Arc<CircuitBreakerMetrics> {
        &self.circuit_breaker_metrics
    }

    /// Get retry metrics
    pub fn retry_metrics(&self) -> &Arc<RetryMetrics> {
        &self.retry_metrics
    }

    /// Get TLS metrics
    pub fn tls_metrics(&self) -> &Arc<TlsMetrics> {
        &self.tls_metrics
    }

    /// Record a proxy request
    pub fn record_request(
        &self,
        route: &str,
        duration: Duration,
        outcome: RequestOutcome,
    ) {
        self.collector.record_request(route, duration, outcome);
    }

    /// Start tracking a request
    pub fn start_request(&self) -> RequestTracker {
        RequestTracker::new(self.clone())
    }
}

impl std::fmt::Debug for ProxyMetrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProxyMetrics")
            .field("pool_metrics", &self.pool_metrics)
            .field("circuit_breaker_metrics", &self.circuit_breaker_metrics)
            .field("retry_metrics", &self.retry_metrics)
            .field("tls_metrics", &self.tls_metrics)
            .finish()
    }
}

/// Connection pool metrics
#[derive(Debug)]
pub struct PoolMetrics {
    /// Total connections created
    pub connections_created: AtomicU64,
    
    /// Total connections reused
    pub connections_reused: AtomicU64,
    
    /// Total connections retired
    pub connections_retired: AtomicU64,
    
    /// Connection acquisition failures
    pub acquisition_failures: AtomicU64,
    
    /// Total time spent acquiring connections (microseconds)
    pub acquisition_time_us: AtomicU64,
    
    /// Number of acquisition attempts
    pub acquisition_attempts: AtomicU64,
}

impl PoolMetrics {
    /// Create new pool metrics
    pub fn new() -> Self {
        Self {
            connections_created: AtomicU64::new(0),
            connections_reused: AtomicU64::new(0),
            connections_retired: AtomicU64::new(0),
            acquisition_failures: AtomicU64::new(0),
            acquisition_time_us: AtomicU64::new(0),
            acquisition_attempts: AtomicU64::new(0),
        }
    }

    /// Record a connection creation
    pub fn record_created(&self) {
        self.connections_created.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a connection reuse
    pub fn record_reused(&self) {
        self.connections_reused.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a connection retirement
    pub fn record_retired(&self) {
        self.connections_retired.fetch_add(1, Ordering::Relaxed);
    }

    /// Record an acquisition failure
    pub fn record_acquisition_failure(&self) {
        self.acquisition_failures.fetch_add(1, Ordering::Relaxed);
    }

    /// Record connection acquisition time
    pub fn record_acquisition_time(&self, duration: Duration) {
        self.acquisition_time_us
            .fetch_add(duration.as_micros() as u64, Ordering::Relaxed);
        self.acquisition_attempts.fetch_add(1, Ordering::Relaxed);
    }

    /// Get average acquisition time in microseconds
    pub fn avg_acquisition_time_us(&self) -> f64 {
        let total = self.acquisition_time_us.load(Ordering::Relaxed);
        let attempts = self.acquisition_attempts.load(Ordering::Relaxed);
        
        if attempts > 0 {
            total as f64 / attempts as f64
        } else {
            0.0
        }
    }

    /// Get connection reuse rate
    pub fn reuse_rate(&self) -> f64 {
        let created = self.connections_created.load(Ordering::Relaxed);
        let reused = self.connections_reused.load(Ordering::Relaxed);
        let total = created + reused;
        
        if total > 0 {
            reused as f64 / total as f64
        } else {
            0.0
        }
    }
}

impl Default for PoolMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Circuit breaker metrics
#[derive(Debug)]
pub struct CircuitBreakerMetrics {
    /// Number of times circuit opened
    pub circuit_opens: AtomicU64,
    
    /// Number of times circuit closed
    pub circuit_closes: AtomicU64,
    
    /// Number of requests rejected by circuit breaker
    pub requests_rejected: AtomicU64,
    
    /// Number of requests allowed in half-open state
    pub half_open_attempts: AtomicU64,
}

impl CircuitBreakerMetrics {
    /// Create new circuit breaker metrics
    pub fn new() -> Self {
        Self {
            circuit_opens: AtomicU64::new(0),
            circuit_closes: AtomicU64::new(0),
            requests_rejected: AtomicU64::new(0),
            half_open_attempts: AtomicU64::new(0),
        }
    }

    /// Record circuit opened
    pub fn record_circuit_open(&self) {
        self.circuit_opens.fetch_add(1, Ordering::Relaxed);
        debug!("Circuit breaker opened");
    }

    /// Record circuit closed
    pub fn record_circuit_close(&self) {
        self.circuit_closes.fetch_add(1, Ordering::Relaxed);
        debug!("Circuit breaker closed");
    }

    /// Record request rejected
    pub fn record_request_rejected(&self) {
        self.requests_rejected.fetch_add(1, Ordering::Relaxed);
    }

    /// Record half-open attempt
    pub fn record_half_open_attempt(&self) {
        self.half_open_attempts.fetch_add(1, Ordering::Relaxed);
    }
}

impl Default for CircuitBreakerMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Retry metrics
#[derive(Debug)]
pub struct RetryMetrics {
    /// Total number of retries attempted
    pub retries_attempted: AtomicU64,
    
    /// Number of successful retries
    pub retries_succeeded: AtomicU64,
    
    /// Number of failed retries (exhausted)
    pub retries_exhausted: AtomicU64,
    
    /// Total backoff time (milliseconds)
    pub total_backoff_ms: AtomicU64,
}

impl RetryMetrics {
    /// Create new retry metrics
    pub fn new() -> Self {
        Self {
            retries_attempted: AtomicU64::new(0),
            retries_succeeded: AtomicU64::new(0),
            retries_exhausted: AtomicU64::new(0),
            total_backoff_ms: AtomicU64::new(0),
        }
    }

    /// Record a retry attempt
    pub fn record_retry_attempt(&self) {
        self.retries_attempted.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a successful retry
    pub fn record_retry_success(&self) {
        self.retries_succeeded.fetch_add(1, Ordering::Relaxed);
    }

    /// Record exhausted retries
    pub fn record_retry_exhausted(&self) {
        self.retries_exhausted.fetch_add(1, Ordering::Relaxed);
    }

    /// Record backoff time
    pub fn record_backoff(&self, duration: Duration) {
        self.total_backoff_ms
            .fetch_add(duration.as_millis() as u64, Ordering::Relaxed);
    }

    /// Get retry success rate
    pub fn success_rate(&self) -> f64 {
        let attempted = self.retries_attempted.load(Ordering::Relaxed);
        let succeeded = self.retries_succeeded.load(Ordering::Relaxed);
        
        if attempted > 0 {
            succeeded as f64 / attempted as f64
        } else {
            0.0
        }
    }

    /// Get average backoff time in milliseconds
    pub fn avg_backoff_ms(&self) -> f64 {
        let total = self.total_backoff_ms.load(Ordering::Relaxed);
        let attempted = self.retries_attempted.load(Ordering::Relaxed);
        
        if attempted > 0 {
            total as f64 / attempted as f64
        } else {
            0.0
        }
    }
}

impl Default for RetryMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// TLS metrics
#[derive(Debug)]
pub struct TlsMetrics {
    /// Total TLS handshakes attempted
    pub handshakes_attempted: AtomicU64,
    
    /// Successful TLS handshakes
    pub handshakes_succeeded: AtomicU64,
    
    /// Failed TLS handshakes
    pub handshakes_failed: AtomicU64,
    
    /// Certificate verification failures
    pub cert_verification_failures: AtomicU64,
    
    /// Total handshake time (microseconds)
    pub handshake_time_us: AtomicU64,
}

impl TlsMetrics {
    /// Create new TLS metrics
    pub fn new() -> Self {
        Self {
            handshakes_attempted: AtomicU64::new(0),
            handshakes_succeeded: AtomicU64::new(0),
            handshakes_failed: AtomicU64::new(0),
            cert_verification_failures: AtomicU64::new(0),
            handshake_time_us: AtomicU64::new(0),
        }
    }

    /// Record TLS handshake attempt
    pub fn record_handshake_attempt(&self) {
        self.handshakes_attempted.fetch_add(1, Ordering::Relaxed);
    }

    /// Record successful TLS handshake
    pub fn record_handshake_success(&self, duration: Duration) {
        self.handshakes_succeeded.fetch_add(1, Ordering::Relaxed);
        self.handshake_time_us
            .fetch_add(duration.as_micros() as u64, Ordering::Relaxed);
    }

    /// Record failed TLS handshake
    pub fn record_handshake_failure(&self) {
        self.handshakes_failed.fetch_add(1, Ordering::Relaxed);
    }

    /// Record certificate verification failure
    pub fn record_cert_verification_failure(&self) {
        self.cert_verification_failures.fetch_add(1, Ordering::Relaxed);
    }

    /// Get handshake success rate
    pub fn success_rate(&self) -> f64 {
        let attempted = self.handshakes_attempted.load(Ordering::Relaxed);
        let succeeded = self.handshakes_succeeded.load(Ordering::Relaxed);
        
        if attempted > 0 {
            succeeded as f64 / attempted as f64
        } else {
            0.0
        }
    }

    /// Get average handshake time in microseconds
    pub fn avg_handshake_time_us(&self) -> f64 {
        let total = self.handshake_time_us.load(Ordering::Relaxed);
        let succeeded = self.handshakes_succeeded.load(Ordering::Relaxed);
        
        if succeeded > 0 {
            total as f64 / succeeded as f64
        } else {
            0.0
        }
    }
}

impl Default for TlsMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Request tracker for automatic metrics recording
pub struct RequestTracker {
    metrics: ProxyMetrics,
    start_time: Instant,
    route: Option<String>,
}

impl RequestTracker {
    /// Create a new request tracker
    pub fn new(metrics: ProxyMetrics) -> Self {
        Self {
            metrics,
            start_time: Instant::now(),
            route: None,
        }
    }

    /// Set the route for this request
    pub fn with_route(mut self, route: String) -> Self {
        self.route = Some(route);
        self
    }

    /// Complete the request with success
    pub fn complete_success(self) {
        let duration = self.start_time.elapsed();
        if let Some(route) = self.route {
            self.metrics.record_request(&route, duration, RequestOutcome::Success);
        }
    }

    /// Complete the request with error
    pub fn complete_error(self) {
        let duration = self.start_time.elapsed();
        if let Some(route) = self.route {
            self.metrics.record_request(&route, duration, RequestOutcome::Error);
        }
    }

    /// Complete the request with timeout
    pub fn complete_timeout(self) {
        let duration = self.start_time.elapsed();
        if let Some(route) = self.route {
            self.metrics.record_request(&route, duration, RequestOutcome::Timeout);
        }
    }

    /// Get elapsed time
    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_metrics() {
        let metrics = PoolMetrics::new();
        
        metrics.record_created();
        metrics.record_reused();
        metrics.record_reused();
        
        assert_eq!(metrics.connections_created.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.connections_reused.load(Ordering::Relaxed), 2);
        assert_eq!(metrics.reuse_rate(), 2.0 / 3.0);
    }

    #[test]
    fn test_circuit_breaker_metrics() {
        let metrics = CircuitBreakerMetrics::new();
        
        metrics.record_circuit_open();
        metrics.record_request_rejected();
        metrics.record_circuit_close();
        
        assert_eq!(metrics.circuit_opens.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.circuit_closes.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.requests_rejected.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_retry_metrics() {
        let metrics = RetryMetrics::new();
        
        metrics.record_retry_attempt();
        metrics.record_retry_attempt();
        metrics.record_retry_success();
        
        assert_eq!(metrics.retries_attempted.load(Ordering::Relaxed), 2);
        assert_eq!(metrics.retries_succeeded.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.success_rate(), 0.5);
    }

    #[test]
    fn test_tls_metrics() {
        let metrics = TlsMetrics::new();
        
        metrics.record_handshake_attempt();
        metrics.record_handshake_success(Duration::from_millis(10));
        metrics.record_handshake_attempt();
        metrics.record_handshake_failure();
        
        assert_eq!(metrics.handshakes_attempted.load(Ordering::Relaxed), 2);
        assert_eq!(metrics.handshakes_succeeded.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.handshakes_failed.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.success_rate(), 0.5);
    }

    #[test]
    fn test_request_tracker() {
        let collector = Arc::new(MetricsCollector::new());
        let metrics = ProxyMetrics::new(collector.clone());
        
        let tracker = metrics.start_request().with_route("/test".to_string());
        std::thread::sleep(Duration::from_millis(10));
        tracker.complete_success();
        
        assert!(collector.total_requests() > 0);
    }
}
