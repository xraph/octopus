//! Passive health tracking based on request success/failure

use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

/// Health metrics for an upstream instance
#[derive(Debug)]
pub struct HealthMetrics {
    /// Total number of requests
    pub total_requests: AtomicU64,
    /// Number of successful requests
    pub successful_requests: AtomicU64,
    /// Number of failed requests
    pub failed_requests: AtomicU64,
    /// Sum of latencies (for calculating average)
    total_latency_ms: AtomicU64,
    /// Last request timestamp
    last_request_time: parking_lot::Mutex<Option<Instant>>,
}

impl Default for HealthMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl HealthMetrics {
    /// Create new health metrics
    pub fn new() -> Self {
        Self {
            total_requests: AtomicU64::new(0),
            successful_requests: AtomicU64::new(0),
            failed_requests: AtomicU64::new(0),
            total_latency_ms: AtomicU64::new(0),
            last_request_time: parking_lot::Mutex::new(None),
        }
    }

    /// Record a successful request
    pub fn record_success(&self, latency: Duration) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        self.successful_requests.fetch_add(1, Ordering::Relaxed);
        self.total_latency_ms
            .fetch_add(latency.as_millis() as u64, Ordering::Relaxed);
        *self.last_request_time.lock() = Some(Instant::now());

        debug!(
            total = self.total_requests.load(Ordering::Relaxed),
            success = self.successful_requests.load(Ordering::Relaxed),
            latency_ms = latency.as_millis(),
            "Recorded successful request"
        );
    }

    /// Record a failed request
    pub fn record_failure(&self, latency: Duration) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        self.failed_requests.fetch_add(1, Ordering::Relaxed);
        self.total_latency_ms
            .fetch_add(latency.as_millis() as u64, Ordering::Relaxed);
        *self.last_request_time.lock() = Some(Instant::now());

        warn!(
            total = self.total_requests.load(Ordering::Relaxed),
            failed = self.failed_requests.load(Ordering::Relaxed),
            latency_ms = latency.as_millis(),
            "Recorded failed request"
        );
    }

    /// Get the error rate (0.0 to 1.0)
    pub fn error_rate(&self) -> f64 {
        let total = self.total_requests.load(Ordering::Relaxed);
        if total == 0 {
            return 0.0;
        }
        let failed = self.failed_requests.load(Ordering::Relaxed);
        failed as f64 / total as f64
    }

    /// Get the success rate (0.0 to 1.0)
    pub fn success_rate(&self) -> f64 {
        1.0 - self.error_rate()
    }

    /// Get the average latency
    pub fn avg_latency(&self) -> Duration {
        let total = self.total_requests.load(Ordering::Relaxed);
        if total == 0 {
            return Duration::from_secs(0);
        }
        let total_latency = self.total_latency_ms.load(Ordering::Relaxed);
        Duration::from_millis(total_latency / total)
    }

    /// Get the last request time
    pub fn last_request_time(&self) -> Option<Instant> {
        *self.last_request_time.lock()
    }

    /// Reset all metrics
    pub fn reset(&self) {
        self.total_requests.store(0, Ordering::Relaxed);
        self.successful_requests.store(0, Ordering::Relaxed);
        self.failed_requests.store(0, Ordering::Relaxed);
        self.total_latency_ms.store(0, Ordering::Relaxed);
        *self.last_request_time.lock() = None;

        info!("Health metrics reset");
    }

    /// Get a snapshot of current metrics
    pub fn snapshot(&self) -> HealthSnapshot {
        HealthSnapshot {
            total_requests: self.total_requests.load(Ordering::Relaxed),
            successful_requests: self.successful_requests.load(Ordering::Relaxed),
            failed_requests: self.failed_requests.load(Ordering::Relaxed),
            error_rate: self.error_rate(),
            success_rate: self.success_rate(),
            avg_latency: self.avg_latency(),
            last_request_time: self.last_request_time(),
        }
    }
}

/// Snapshot of health metrics at a point in time
#[derive(Debug, Clone)]
pub struct HealthSnapshot {
    /// Total number of requests
    pub total_requests: u64,
    /// Number of successful requests
    pub successful_requests: u64,
    /// Number of failed requests
    pub failed_requests: u64,
    /// Error rate (0.0 to 1.0)
    pub error_rate: f64,
    /// Success rate (0.0 to 1.0)
    pub success_rate: f64,
    /// Average latency
    pub avg_latency: Duration,
    /// Last request time
    pub last_request_time: Option<Instant>,
}

/// Health tracker configuration
#[derive(Debug, Clone)]
pub struct HealthTrackerConfig {
    /// Time window for calculating metrics
    pub window_duration: Duration,
    /// Interval for cleaning up old metrics
    pub cleanup_interval: Duration,
}

impl Default for HealthTrackerConfig {
    fn default() -> Self {
        Self {
            window_duration: Duration::from_secs(60),
            cleanup_interval: Duration::from_secs(300),
        }
    }
}

/// Health tracker that monitors passive health of upstream instances
#[derive(Debug, Clone)]
pub struct HealthTracker {
    config: HealthTrackerConfig,
    metrics: Arc<DashMap<String, Arc<HealthMetrics>>>,
}

impl HealthTracker {
    /// Create a new health tracker
    pub fn new(config: HealthTrackerConfig) -> Self {
        Self {
            config,
            metrics: Arc::new(DashMap::new()),
        }
    }

    /// Create a health tracker with default config
    pub fn default_config() -> Self {
        Self::new(HealthTrackerConfig::default())
    }

    /// Get or create metrics for an instance
    fn get_or_create_metrics(&self, instance_id: &str) -> Arc<HealthMetrics> {
        self.metrics
            .entry(instance_id.to_string())
            .or_insert_with(|| Arc::new(HealthMetrics::new()))
            .clone()
    }

    /// Record a successful request
    pub fn record_success(&self, instance_id: &str, latency: Duration) {
        let metrics = self.get_or_create_metrics(instance_id);
        metrics.record_success(latency);
    }

    /// Record a failed request
    pub fn record_failure(&self, instance_id: &str, latency: Duration) {
        let metrics = self.get_or_create_metrics(instance_id);
        metrics.record_failure(latency);
    }

    /// Get metrics for an instance
    pub fn get_metrics(&self, instance_id: &str) -> Option<Arc<HealthMetrics>> {
        self.metrics.get(instance_id).map(|entry| entry.clone())
    }

    /// Get a snapshot of metrics for an instance
    pub fn get_snapshot(&self, instance_id: &str) -> Option<HealthSnapshot> {
        self.get_metrics(instance_id).map(|m| m.snapshot())
    }

    /// Get snapshots for all instances
    pub fn get_all_snapshots(&self) -> Vec<(String, HealthSnapshot)> {
        self.metrics
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().snapshot()))
            .collect()
    }

    /// Check if an instance is healthy based on error rate
    pub fn is_healthy(&self, instance_id: &str, threshold: f64) -> bool {
        if let Some(metrics) = self.get_metrics(instance_id) {
            let total = metrics.total_requests.load(Ordering::Relaxed);
            // Need minimum requests before making judgment
            if total < 10 {
                return true; // Assume healthy until proven otherwise
            }
            metrics.error_rate() < threshold
        } else {
            true // No data, assume healthy
        }
    }

    /// Reset metrics for an instance
    pub fn reset_instance(&self, instance_id: &str) {
        if let Some(metrics) = self.get_metrics(instance_id) {
            metrics.reset();
        }
    }

    /// Reset all metrics
    pub fn reset_all(&self) {
        for entry in self.metrics.iter() {
            entry.value().reset();
        }
        info!("All health metrics reset");
    }

    /// Start background cleanup task
    pub fn start_cleanup_task(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(self.config.cleanup_interval);
            loop {
                interval.tick().await;
                self.cleanup_old_metrics();
            }
        });
    }

    /// Clean up metrics for instances that haven't been accessed recently
    fn cleanup_old_metrics(&self) {
        let cutoff = Instant::now() - self.config.window_duration * 2;
        let mut removed = 0;

        self.metrics.retain(|_id, metrics| {
            if let Some(last_request) = metrics.last_request_time() {
                if last_request < cutoff {
                    removed += 1;
                    return false;
                }
            }
            true
        });

        if removed > 0 {
            info!(removed = removed, "Cleaned up old health metrics");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_health_metrics() {
        let metrics = HealthMetrics::new();

        assert_eq!(metrics.total_requests.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.error_rate(), 0.0);
        assert_eq!(metrics.success_rate(), 1.0);

        // Record successes
        metrics.record_success(Duration::from_millis(10));
        metrics.record_success(Duration::from_millis(20));

        assert_eq!(metrics.total_requests.load(Ordering::Relaxed), 2);
        assert_eq!(metrics.successful_requests.load(Ordering::Relaxed), 2);
        assert_eq!(metrics.error_rate(), 0.0);
        assert_eq!(metrics.success_rate(), 1.0);
        assert_eq!(metrics.avg_latency(), Duration::from_millis(15));

        // Record failures
        metrics.record_failure(Duration::from_millis(30));

        assert_eq!(metrics.total_requests.load(Ordering::Relaxed), 3);
        assert_eq!(metrics.failed_requests.load(Ordering::Relaxed), 1);
        assert!((metrics.error_rate() - 1.0 / 3.0).abs() < 1e-10); // Use epsilon for float comparison
        assert!((metrics.success_rate() - 2.0 / 3.0).abs() < 1e-10);

        // Reset
        metrics.reset();
        assert_eq!(metrics.total_requests.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.error_rate(), 0.0);
    }

    #[test]
    fn test_health_tracker() {
        let tracker = HealthTracker::default_config();

        // Record enough requests to pass minimum threshold (10)
        for _ in 0..7 {
            tracker.record_success("instance1", Duration::from_millis(10));
        }
        for _ in 0..3 {
            tracker.record_failure("instance1", Duration::from_millis(30));
        }

        let snapshot = tracker.get_snapshot("instance1").unwrap();
        assert_eq!(snapshot.total_requests, 10);
        assert_eq!(snapshot.successful_requests, 7);
        assert_eq!(snapshot.failed_requests, 3);
        assert!((snapshot.error_rate - 0.3).abs() < 1e-10); // 3/10 = 0.3

        // Test is_healthy with threshold
        assert!(tracker.is_healthy("instance1", 0.5)); // Error rate (30%) < 50%
        assert!(!tracker.is_healthy("instance1", 0.2)); // Error rate (30%) > 20%
    }

    #[test]
    fn test_health_tracker_multiple_instances() {
        let tracker = HealthTracker::default_config();

        tracker.record_success("instance1", Duration::from_millis(10));
        tracker.record_success("instance2", Duration::from_millis(20));
        tracker.record_failure("instance3", Duration::from_millis(30));

        let snapshots = tracker.get_all_snapshots();
        assert_eq!(snapshots.len(), 3);

        assert!(tracker.get_snapshot("instance1").is_some());
        assert!(tracker.get_snapshot("instance2").is_some());
        assert!(tracker.get_snapshot("instance3").is_some());
        assert!(tracker.get_snapshot("instance4").is_none());
    }

    #[test]
    fn test_health_snapshot() {
        let metrics = HealthMetrics::new();
        metrics.record_success(Duration::from_millis(10));
        metrics.record_failure(Duration::from_millis(20));

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.total_requests, 2);
        assert_eq!(snapshot.successful_requests, 1);
        assert_eq!(snapshot.failed_requests, 1);
        assert_eq!(snapshot.error_rate, 0.5);
        assert_eq!(snapshot.success_rate, 0.5);
    }
}
