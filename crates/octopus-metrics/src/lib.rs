//! Metrics collection and tracking for the Octopus API Gateway
//!
//! This crate provides comprehensive metrics tracking including:
//! - Request counts (total and per-route)
//! - Latency tracking (min, max, avg, p50, p95, p99)
//! - Error rates and counts
//! - Active connections
//! - Activity logs for recent requests

use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub mod collector;
pub mod activity;
pub mod snapshot;
pub mod prometheus;

pub use collector::MetricsCollector;
pub use activity::{ActivityLog, ActivityEntry};
pub use snapshot::{MetricsSnapshot, RouteMetrics};
pub use prometheus::PrometheusExporter;

/// Request outcome
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestOutcome {
    /// Request succeeded
    Success,
    /// Request failed
    Error,
    /// Request timed out
    Timeout,
}

/// Helper function to get current timestamp in milliseconds
pub fn current_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Helper function to format duration in milliseconds
pub fn format_duration_ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_current_timestamp() {
        let ts1 = current_timestamp_ms();
        std::thread::sleep(Duration::from_millis(10));
        let ts2 = current_timestamp_ms();
        assert!(ts2 > ts1);
    }

    #[test]
    fn test_format_duration() {
        let duration = Duration::from_millis(123);
        let formatted = format_duration_ms(duration);
        assert!((formatted - 123.0).abs() < 0.1);
    }
}
