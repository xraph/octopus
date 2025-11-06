//! Metrics collector for tracking gateway performance

use super::*;
use std::collections::VecDeque;

/// Per-route metrics tracking
#[derive(Debug)]
pub struct RouteStats {
    /// Total requests to this route
    pub request_count: AtomicU64,
    /// Total errors for this route
    pub error_count: AtomicU64,
    /// Total latency in nanoseconds
    total_latency_ns: AtomicU64,
    /// Minimum latency in nanoseconds
    min_latency_ns: AtomicU64,
    /// Maximum latency in nanoseconds
    max_latency_ns: AtomicU64,
    /// Recent latencies for percentile calculation (last 1000)
    recent_latencies: parking_lot::Mutex<VecDeque<u64>>,
}

impl RouteStats {
    /// Create new route stats
    pub fn new() -> Self {
        Self {
            request_count: AtomicU64::new(0),
            error_count: AtomicU64::new(0),
            total_latency_ns: AtomicU64::new(0),
            min_latency_ns: AtomicU64::new(u64::MAX),
            max_latency_ns: AtomicU64::new(0),
            recent_latencies: parking_lot::Mutex::new(VecDeque::with_capacity(1000)),
        }
    }

    /// Record a request with its latency
    pub fn record_request(&self, latency_ns: u64, outcome: RequestOutcome) {
        // Update counts
        self.request_count.fetch_add(1, Ordering::Relaxed);
        if outcome == RequestOutcome::Error || outcome == RequestOutcome::Timeout {
            self.error_count.fetch_add(1, Ordering::Relaxed);
        }

        // Update latency stats
        self.total_latency_ns.fetch_add(latency_ns, Ordering::Relaxed);
        
        // Update min latency
        let mut current_min = self.min_latency_ns.load(Ordering::Relaxed);
        while latency_ns < current_min {
            match self.min_latency_ns.compare_exchange_weak(
                current_min,
                latency_ns,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(x) => current_min = x,
            }
        }

        // Update max latency
        let mut current_max = self.max_latency_ns.load(Ordering::Relaxed);
        while latency_ns > current_max {
            match self.max_latency_ns.compare_exchange_weak(
                current_max,
                latency_ns,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(x) => current_max = x,
            }
        }

        // Store for percentile calculation
        let mut latencies = self.recent_latencies.lock();
        if latencies.len() >= 1000 {
            latencies.pop_front();
        }
        latencies.push_back(latency_ns);
    }

    /// Get average latency in milliseconds
    pub fn avg_latency_ms(&self) -> f64 {
        let count = self.request_count.load(Ordering::Relaxed);
        if count == 0 {
            return 0.0;
        }
        let total_ns = self.total_latency_ns.load(Ordering::Relaxed);
        (total_ns as f64 / count as f64) / 1_000_000.0
    }

    /// Get minimum latency in milliseconds
    pub fn min_latency_ms(&self) -> f64 {
        let min_ns = self.min_latency_ns.load(Ordering::Relaxed);
        if min_ns == u64::MAX {
            return 0.0;
        }
        min_ns as f64 / 1_000_000.0
    }

    /// Get maximum latency in milliseconds
    pub fn max_latency_ms(&self) -> f64 {
        let max_ns = self.max_latency_ns.load(Ordering::Relaxed);
        max_ns as f64 / 1_000_000.0
    }

    /// Calculate percentile latency
    pub fn percentile_latency_ms(&self, percentile: f64) -> f64 {
        let latencies = self.recent_latencies.lock();
        if latencies.is_empty() {
            return 0.0;
        }

        let mut sorted: Vec<u64> = latencies.iter().copied().collect();
        sorted.sort_unstable();

        let index = ((percentile / 100.0) * (sorted.len() as f64)) as usize;
        let index = index.min(sorted.len() - 1);
        sorted[index] as f64 / 1_000_000.0
    }

    /// Get error rate as percentage
    pub fn error_rate(&self) -> f64 {
        let total = self.request_count.load(Ordering::Relaxed);
        if total == 0 {
            return 0.0;
        }
        let errors = self.error_count.load(Ordering::Relaxed);
        (errors as f64 / total as f64) * 100.0
    }
}

impl Default for RouteStats {
    fn default() -> Self {
        Self::new()
    }
}

/// Main metrics collector for the gateway
#[derive(Debug, Clone)]
pub struct MetricsCollector {
    /// Global request counter
    total_requests: Arc<AtomicU64>,
    /// Global error counter
    total_errors: Arc<AtomicU64>,
    /// Per-route statistics
    route_stats: Arc<DashMap<String, Arc<RouteStats>>>,
    /// Active connections
    active_connections: Arc<AtomicUsize>,
    /// Start time of the collector
    start_time: Arc<AtomicU64>,
}

impl MetricsCollector {
    /// Create a new metrics collector
    pub fn new() -> Self {
        Self {
            total_requests: Arc::new(AtomicU64::new(0)),
            total_errors: Arc::new(AtomicU64::new(0)),
            route_stats: Arc::new(DashMap::new()),
            active_connections: Arc::new(AtomicUsize::new(0)),
            start_time: Arc::new(AtomicU64::new(current_timestamp_ms())),
        }
    }

    /// Record a request
    pub fn record_request(&self, route: &str, latency: Duration, outcome: RequestOutcome) {
        // Update global counters
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        if outcome == RequestOutcome::Error || outcome == RequestOutcome::Timeout {
            self.total_errors.fetch_add(1, Ordering::Relaxed);
        }

        // Update route-specific stats
        let stats = self.route_stats
            .entry(route.to_string())
            .or_insert_with(|| Arc::new(RouteStats::new()))
            .clone();

        stats.record_request(latency.as_nanos() as u64, outcome);
    }

    /// Increment active connections
    pub fn increment_active_connections(&self) {
        self.active_connections.fetch_add(1, Ordering::Relaxed);
    }

    /// Decrement active connections
    pub fn decrement_active_connections(&self) {
        self.active_connections.fetch_sub(1, Ordering::Relaxed);
    }

    /// Get total request count
    pub fn total_requests(&self) -> u64 {
        self.total_requests.load(Ordering::Relaxed)
    }

    /// Get total error count
    pub fn total_errors(&self) -> u64 {
        self.total_errors.load(Ordering::Relaxed)
    }

    /// Get active connections count
    pub fn active_connections(&self) -> usize {
        self.active_connections.load(Ordering::Relaxed)
    }

    /// Get number of unique routes
    pub fn route_count(&self) -> usize {
        self.route_stats.len()
    }

    /// Get stats for a specific route
    pub fn route_stats(&self, route: &str) -> Option<Arc<RouteStats>> {
        self.route_stats.get(route).map(|r| r.clone())
    }

    /// Get all route names
    pub fn route_names(&self) -> Vec<String> {
        self.route_stats.iter().map(|r| r.key().clone()).collect()
    }

    /// Calculate global average latency
    pub fn global_avg_latency_ms(&self) -> f64 {
        let total_requests = self.total_requests.load(Ordering::Relaxed);
        if total_requests == 0 {
            return 0.0;
        }

        let mut total_latency_ns: u64 = 0;
        for entry in self.route_stats.iter() {
            let stats = entry.value();
            let route_total = stats.total_latency_ns.load(Ordering::Relaxed);
            total_latency_ns = total_latency_ns.saturating_add(route_total);
        }

        (total_latency_ns as f64 / total_requests as f64) / 1_000_000.0
    }

    /// Get uptime in seconds
    pub fn uptime_seconds(&self) -> u64 {
        let start = self.start_time.load(Ordering::Relaxed);
        let now = current_timestamp_ms();
        (now - start) / 1000
    }

    /// Get global error rate
    pub fn global_error_rate(&self) -> f64 {
        let total = self.total_requests.load(Ordering::Relaxed);
        if total == 0 {
            return 0.0;
        }
        let errors = self.total_errors.load(Ordering::Relaxed);
        (errors as f64 / total as f64) * 100.0
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_route_stats_new() {
        let stats = RouteStats::new();
        assert_eq!(stats.request_count.load(Ordering::Relaxed), 0);
        assert_eq!(stats.error_count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_route_stats_record() {
        let stats = RouteStats::new();
        stats.record_request(5_000_000, RequestOutcome::Success); // 5ms
        assert_eq!(stats.request_count.load(Ordering::Relaxed), 1);
        assert_eq!(stats.error_count.load(Ordering::Relaxed), 0);
        assert_eq!(stats.avg_latency_ms(), 5.0);
    }

    #[test]
    fn test_route_stats_errors() {
        let stats = RouteStats::new();
        stats.record_request(5_000_000, RequestOutcome::Success);
        stats.record_request(10_000_000, RequestOutcome::Error);
        assert_eq!(stats.request_count.load(Ordering::Relaxed), 2);
        assert_eq!(stats.error_count.load(Ordering::Relaxed), 1);
        assert_eq!(stats.error_rate(), 50.0);
    }

    #[test]
    fn test_metrics_collector() {
        let collector = MetricsCollector::new();
        collector.record_request("/users", Duration::from_millis(5), RequestOutcome::Success);
        collector.record_request("/posts", Duration::from_millis(10), RequestOutcome::Success);
        
        assert_eq!(collector.total_requests(), 2);
        assert_eq!(collector.route_count(), 2);
    }

    #[test]
    fn test_active_connections() {
        let collector = MetricsCollector::new();
        collector.increment_active_connections();
        collector.increment_active_connections();
        assert_eq!(collector.active_connections(), 2);
        collector.decrement_active_connections();
        assert_eq!(collector.active_connections(), 1);
    }
}

