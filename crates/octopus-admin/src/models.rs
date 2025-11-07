//! Data models for the admin dashboard

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Dashboard statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardStats {
    pub total_requests: u64,
    pub active_routes: usize,
    pub avg_latency_ms: f64,
    pub health_status: String,
    pub requests_per_second: f64,
    pub error_rate: f64,
    pub active_connections: u64,
    pub cpu_usage: f64,
    pub memory_usage: f64,
}

/// Extended metrics for analytics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsMetrics {
    pub timeframe: String,
    pub request_volume: Vec<TimeSeriesPoint>,
    pub latency_percentiles: LatencyPercentiles,
    pub error_breakdown: HashMap<String, u64>,
    pub top_routes: Vec<RouteMetric>,
    pub status_code_distribution: HashMap<u16, u64>,
    pub traffic_by_method: HashMap<String, u64>,
}

/// Time series data point
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeSeriesPoint {
    pub timestamp: String,
    pub value: f64,
}

/// Latency percentiles
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencyPercentiles {
    pub p50: f64,
    pub p90: f64,
    pub p95: f64,
    pub p99: f64,
}

/// Route metric
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteMetric {
    pub path: String,
    pub requests: u64,
    pub avg_latency: f64,
    pub error_rate: f64,
}

/// Route information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteInfo {
    pub id: String,
    pub path: String,
    pub method: String,
    pub upstream: String,
    pub request_count: u64,
    pub is_healthy: bool,
    pub avg_latency_ms: f64,
    pub error_count: u64,
    pub last_accessed: Option<String>,
}

/// Route configuration for CRUD operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteConfig {
    pub id: Option<String>,
    pub path: String,
    pub method: String,
    pub upstream: String,
    pub timeout_ms: Option<u64>,
    pub retry_count: Option<u32>,
    pub circuit_breaker: Option<CircuitBreakerConfig>,
    pub rate_limit: Option<RateLimitConfig>,
}

/// Circuit breaker configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerConfig {
    pub failure_threshold: u32,
    pub success_threshold: u32,
    pub timeout_seconds: u64,
}

/// Rate limit configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    pub requests_per_second: u32,
    pub burst_size: u32,
}

/// Health check information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckInfo {
    pub name: String,
    pub status: String, // "passing", "warning", "critical"
    pub response_time_ms: u64,
    pub message: Option<String>,
    pub endpoint: Option<String>,
    pub last_check: String,
    pub consecutive_failures: u32,
}

/// Plugin information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: Option<String>,
    pub enabled: bool,
    pub has_dashboard: bool,
    pub config: Option<serde_json::Value>,
}

/// Activity log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityLogEntry {
    pub timestamp: String,
    pub level: String, // "info", "warning", "error"
    pub message: String,
    pub details: Option<String>,
    pub source: Option<String>,
}

/// Log query parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogQuery {
    pub level: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    pub search: Option<String>,
}

/// System information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfo {
    pub version: String,
    pub uptime_seconds: u64,
    pub start_time: String,
    pub hostname: String,
    pub os: String,
    pub arch: String,
    pub num_cpus: usize,
    pub total_memory: u64,
}

/// Performance metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    pub cpu_usage: f64,
    pub memory_usage: f64,
    pub memory_total: u64,
    pub memory_available: u64,
    pub goroutines: usize,
    pub gc_count: u64,
    pub gc_pause_ms: f64,
}

/// Security event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityEvent {
    pub timestamp: String,
    pub event_type: String, // "rate_limit", "blocked_ip", "auth_failure"
    pub severity: String,   // "low", "medium", "high", "critical"
    pub source_ip: String,
    pub details: String,
}

/// Configuration item
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigItem {
    pub key: String,
    pub value: serde_json::Value,
    pub description: Option<String>,
    pub editable: bool,
}

/// Plugin stats card
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginStatsCard {
    pub title: String,
    pub value: String,
}

impl Default for DashboardStats {
    fn default() -> Self {
        Self {
            total_requests: 0,
            active_routes: 0,
            avg_latency_ms: 0.0,
            health_status: "healthy".to_string(),
            requests_per_second: 0.0,
            error_rate: 0.0,
            active_connections: 0,
            cpu_usage: 0.0,
            memory_usage: 0.0,
        }
    }
}
