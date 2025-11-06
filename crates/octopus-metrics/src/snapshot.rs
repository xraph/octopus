//! Metrics snapshot for exporting current state

use super::*;
use serde::{Serialize, Deserialize};

/// Snapshot of route-specific metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteMetrics {
    /// Route path
    pub path: String,
    /// Total requests
    pub request_count: u64,
    /// Error count
    pub error_count: u64,
    /// Average latency in milliseconds
    pub avg_latency_ms: f64,
    /// Minimum latency in milliseconds
    pub min_latency_ms: f64,
    /// Maximum latency in milliseconds
    pub max_latency_ms: f64,
    /// P50 latency in milliseconds
    pub p50_latency_ms: f64,
    /// P95 latency in milliseconds
    pub p95_latency_ms: f64,
    /// P99 latency in milliseconds
    pub p99_latency_ms: f64,
    /// Error rate as percentage
    pub error_rate: f64,
}

/// Complete metrics snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    /// Timestamp when snapshot was taken
    pub timestamp: u64,
    /// Total requests across all routes
    pub total_requests: u64,
    /// Total errors across all routes
    pub total_errors: u64,
    /// Number of active connections
    pub active_connections: usize,
    /// Number of unique routes
    pub route_count: usize,
    /// Global average latency in milliseconds
    pub global_avg_latency_ms: f64,
    /// Global error rate as percentage
    pub global_error_rate: f64,
    /// Uptime in seconds
    pub uptime_seconds: u64,
    /// Per-route metrics
    pub routes: Vec<RouteMetrics>,
}

impl MetricsSnapshot {
    /// Create a snapshot from a metrics collector
    pub fn from_collector(collector: &MetricsCollector) -> Self {
        let timestamp = current_timestamp_ms();
        let total_requests = collector.total_requests();
        let total_errors = collector.total_errors();
        let active_connections = collector.active_connections();
        let route_count = collector.route_count();
        let global_avg_latency_ms = collector.global_avg_latency_ms();
        let global_error_rate = collector.global_error_rate();
        let uptime_seconds = collector.uptime_seconds();

        // Collect per-route metrics
        let mut routes = Vec::new();
        for route_name in collector.route_names() {
            if let Some(stats) = collector.route_stats(&route_name) {
                routes.push(RouteMetrics {
                    path: route_name,
                    request_count: stats.request_count.load(Ordering::Relaxed),
                    error_count: stats.error_count.load(Ordering::Relaxed),
                    avg_latency_ms: stats.avg_latency_ms(),
                    min_latency_ms: stats.min_latency_ms(),
                    max_latency_ms: stats.max_latency_ms(),
                    p50_latency_ms: stats.percentile_latency_ms(50.0),
                    p95_latency_ms: stats.percentile_latency_ms(95.0),
                    p99_latency_ms: stats.percentile_latency_ms(99.0),
                    error_rate: stats.error_rate(),
                });
            }
        }

        // Sort routes by request count (descending)
        routes.sort_by(|a, b| b.request_count.cmp(&a.request_count));

        Self {
            timestamp,
            total_requests,
            total_errors,
            active_connections,
            route_count,
            global_avg_latency_ms,
            global_error_rate,
            uptime_seconds,
            routes,
        }
    }

    /// Get formatted uptime string
    pub fn formatted_uptime(&self) -> String {
        let seconds = self.uptime_seconds;
        let days = seconds / 86400;
        let hours = (seconds % 86400) / 3600;
        let minutes = (seconds % 3600) / 60;
        let secs = seconds % 60;

        if days > 0 {
            format!("{}d {}h {}m", days, hours, minutes)
        } else if hours > 0 {
            format!("{}h {}m", hours, minutes)
        } else if minutes > 0 {
            format!("{}m {}s", minutes, secs)
        } else {
            format!("{}s", secs)
        }
    }

    /// Get top N routes by request count
    pub fn top_routes(&self, n: usize) -> &[RouteMetrics] {
        let limit = n.min(self.routes.len());
        &self.routes[0..limit]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_from_collector() {
        let collector = MetricsCollector::new();
        collector.record_request("/users", Duration::from_millis(50), RequestOutcome::Success);
        collector.record_request("/posts", Duration::from_millis(100), RequestOutcome::Success);
        
        let snapshot = MetricsSnapshot::from_collector(&collector);
        
        assert_eq!(snapshot.total_requests, 2);
        assert_eq!(snapshot.route_count, 2);
        assert_eq!(snapshot.routes.len(), 2);
    }

    #[test]
    fn test_formatted_uptime() {
        let snapshot = MetricsSnapshot {
            timestamp: 0,
            total_requests: 0,
            total_errors: 0,
            active_connections: 0,
            route_count: 0,
            global_avg_latency_ms: 0.0,
            global_error_rate: 0.0,
            uptime_seconds: 3665, // 1h 1m 5s
            routes: vec![],
        };

        let uptime = snapshot.formatted_uptime();
        assert!(uptime.contains("1h"));
        assert!(uptime.contains("1m"));
    }

    #[test]
    fn test_top_routes() {
        let routes = vec![
            RouteMetrics {
                path: "/users".to_string(),
                request_count: 100,
                error_count: 0,
                avg_latency_ms: 50.0,
                min_latency_ms: 10.0,
                max_latency_ms: 100.0,
                p50_latency_ms: 50.0,
                p95_latency_ms: 90.0,
                p99_latency_ms: 95.0,
                error_rate: 0.0,
            },
            RouteMetrics {
                path: "/posts".to_string(),
                request_count: 50,
                error_count: 0,
                avg_latency_ms: 30.0,
                min_latency_ms: 10.0,
                max_latency_ms: 50.0,
                p50_latency_ms: 30.0,
                p95_latency_ms: 45.0,
                p99_latency_ms: 48.0,
                error_rate: 0.0,
            },
        ];

        let snapshot = MetricsSnapshot {
            timestamp: 0,
            total_requests: 150,
            total_errors: 0,
            active_connections: 0,
            route_count: 2,
            global_avg_latency_ms: 40.0,
            global_error_rate: 0.0,
            uptime_seconds: 100,
            routes,
        };

        let top = snapshot.top_routes(1);
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].path, "/users");
    }
}

