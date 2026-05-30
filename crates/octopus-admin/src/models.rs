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

/// Per-route rate limit, as exposed to the admin API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RateLimitInfo {
    /// Requests allowed per window.
    pub requests: u32,
    /// Window length in milliseconds.
    pub window_ms: u64,
}

/// Route information (operational metrics + effective configuration).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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

    // ── Effective route configuration ──────────────────────────────────
    /// Match priority (higher wins).
    #[serde(default)]
    pub priority: i32,
    /// Prefix stripped before proxying.
    #[serde(default)]
    pub strip_prefix: Option<String>,
    /// Prefix added before proxying.
    #[serde(default)]
    pub add_prefix: Option<String>,
    /// Auth provider enforced on this route.
    #[serde(default)]
    pub auth_provider: Option<String>,
    /// Whether authentication is skipped.
    #[serde(default)]
    pub skip_auth: bool,
    /// Required roles.
    #[serde(default)]
    pub require_roles: Vec<String>,
    /// Required scopes.
    #[serde(default)]
    pub require_scopes: Vec<String>,
    /// Authorization rule expression.
    #[serde(default)]
    pub authz_rule: Option<String>,
    /// Per-route timeout in milliseconds.
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    /// Per-route rate limit.
    #[serde(default)]
    pub rate_limit: Option<RateLimitInfo>,
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

/// Upstream cluster information with per-instance health
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamClusterInfo {
    pub name: String,
    pub strategy: String,
    pub instance_count: usize,
    pub healthy_count: usize,
    pub instances: Vec<UpstreamInstanceInfo>,
}

/// Per-instance upstream information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamInstanceInfo {
    pub id: String,
    pub address: String,
    pub port: u16,
    pub url: String,
    pub weight: u32,
    pub healthy: bool,
    pub active_connections: u32,
    pub avg_latency_ms: f64,
    pub error_rate: f64,
}

/// FARP service information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FarpServiceInfo {
    pub name: String,
    pub version: String,
    pub instance_id: Option<String>,
    pub schemas_count: usize,
    pub capabilities: Vec<String>,
    pub registered_at: String,
    pub updated_at: String,
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

// ============================================================================
// Upstream CRUD payloads
// ============================================================================

/// Upstream cluster create/update payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamConfig {
    /// Cluster name (unique key).
    pub name: String,
    /// Load-balancing strategy: `round_robin`, `least_connections`,
    /// `weighted_round_robin`, `random`, or `ip_hash`. Defaults to round-robin.
    #[serde(default)]
    pub strategy: Option<String>,
    /// Backend instances.
    #[serde(default)]
    pub instances: Vec<UpstreamInstanceConfig>,
}

/// A single upstream instance in a create/update payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamInstanceConfig {
    /// Stable instance id (generated when omitted).
    #[serde(default)]
    pub id: Option<String>,
    /// Host or IP address.
    pub address: String,
    /// Port.
    pub port: u16,
    /// Relative weight for weighted strategies.
    #[serde(default)]
    pub weight: Option<u32>,
}

// ============================================================================
// TLS certificate inspection
// ============================================================================

/// A TLS certificate as surfaced to the admin dashboard.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TlsCertInfo {
    /// Logical name (the configured cert file name or SNI host).
    pub name: String,
    /// Certificate file path on disk (if file-backed).
    pub cert_file: Option<String>,
    /// Private key file path on disk (if file-backed).
    pub key_file: Option<String>,
    /// SNI hostnames this certificate serves (from SANs).
    pub sni_hosts: Vec<String>,
    /// Subject common name.
    pub subject_cn: Option<String>,
    /// Subject alternative names (DNS).
    pub sans: Vec<String>,
    /// Issuer distinguished name.
    pub issuer: Option<String>,
    /// Not-before (RFC3339).
    pub not_before: Option<String>,
    /// Not-after / expiry (RFC3339).
    pub not_after: Option<String>,
    /// Days until expiry (negative when expired).
    pub days_until_expiry: Option<i64>,
    /// `valid`, `expiring` (< 30 days), `expired`, or `unknown`.
    pub status: String,
    /// Minimum negotiated TLS version, when known from config.
    pub min_tls_version: Option<String>,
    /// Whether mutual TLS (client certs) is required.
    pub require_client_cert: bool,
    /// Where this entry came from: `config` | `operator` | `manual`.
    pub source: String,
}

/// Payload for uploading a PEM certificate pair (best-effort; persisted only
/// when a writable certificate path is configured).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsCertUpload {
    /// Logical name / primary hostname.
    pub name: String,
    /// PEM-encoded certificate chain.
    pub cert_pem: String,
    /// PEM-encoded private key.
    pub key_pem: String,
}

// ============================================================================
// Kubernetes CRD views
// ============================================================================

/// A thin, serde-friendly summary of a Kubernetes custom resource.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct K8sResourceSummary {
    /// Resource name.
    pub name: String,
    /// Namespace (None for cluster-scoped).
    pub namespace: Option<String>,
    /// Kind (`OctopusGateway`, `OctopusRoute`, `OctopusPolicy`, `OctopusUpstream`).
    pub kind: String,
    /// The `.spec` payload.
    pub spec: serde_json::Value,
    /// Creation timestamp (RFC3339), when available.
    pub created_at: Option<String>,
}

/// Kubernetes operator connectivity status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct K8sStatus {
    /// Whether the admin process could reach a cluster.
    pub connected: bool,
    /// Whether the crate was compiled with the `kubernetes` feature.
    pub feature_enabled: bool,
    /// Human-readable detail (error message or context name).
    pub detail: Option<String>,
    /// Counts per CRD kind.
    pub counts: std::collections::HashMap<String, usize>,
}

// ============================================================================
// Admin authentication / session
// ============================================================================

/// Login request body.
#[derive(Debug, Clone, Deserialize)]
pub struct LoginRequest {
    /// Username.
    pub username: String,
    /// Password.
    pub password: String,
}

/// Login response body.
#[derive(Debug, Clone, Serialize)]
pub struct LoginResponse {
    /// Whether the login succeeded.
    pub success: bool,
    /// Bearer token (also set as an HttpOnly cookie). Present on success.
    pub token: Option<String>,
    /// Token expiry (unix seconds, as string).
    pub expires_at: Option<String>,
    /// Optional human-readable message.
    pub message: Option<String>,
}

/// Current-session response body (`GET /admin/api/auth/me`).
#[derive(Debug, Clone, Serialize)]
pub struct MeResponse {
    /// Whether the caller holds a valid session (always true when auth is disabled).
    pub authenticated: bool,
    /// Whether the dashboard requires authentication at all.
    pub auth_required: bool,
    /// Authenticated username.
    pub username: Option<String>,
    /// Role (currently always `admin`).
    pub role: Option<String>,
}
