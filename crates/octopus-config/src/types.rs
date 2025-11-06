//! Configuration types

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Duration;

/// Main configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Config {
    /// Gateway configuration
    pub gateway: GatewayConfig,
    
    /// Upstream services
    #[serde(default)]
    pub upstreams: Vec<UpstreamConfig>,
    
    /// Routes
    #[serde(default)]
    pub routes: Vec<RouteConfig>,
    
    /// Plugins
    #[serde(default)]
    pub plugins: Vec<PluginConfig>,
    
    /// FARP (service discovery and auto-routing)
    #[serde(default)]
    pub farp: FarpConfig,
    
    /// Observability
    #[serde(default)]
    pub observability: ObservabilityConfig,
}

/// Gateway configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GatewayConfig {
    /// Listen address
    pub listen: SocketAddr,
    
    /// Worker threads (0 = auto)
    #[serde(default)]
    pub workers: usize,
    
    /// Request timeout
    #[serde(default = "default_timeout", with = "humantime_serde")]
    pub request_timeout: Duration,
    
    /// Graceful shutdown timeout (wait for in-flight requests)
    #[serde(default = "default_shutdown_timeout", with = "humantime_serde")]
    pub shutdown_timeout: Duration,
    
    /// Max request body size (bytes)
    #[serde(default = "default_max_body_size")]
    pub max_body_size: usize,
    
    /// TLS configuration
    #[serde(default)]
    pub tls: Option<TlsConfig>,
    
    /// Compression configuration
    #[serde(default)]
    pub compression: CompressionConfig,
    
    /// Internal route prefix (default: "__")
    /// Internal endpoints like admin, metrics, farp will use this prefix
    /// Example: "/__admin", "/__metrics", "/__farp"
    #[serde(default = "default_internal_prefix")]
    pub internal_route_prefix: Option<String>,
}

fn default_internal_prefix() -> Option<String> {
    Some("__".to_string())
}

/// TLS configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TlsConfig {
    /// Certificate file path
    pub cert_file: String,
    
    /// Private key file path
    pub key_file: String,
    
    /// Optional client CA certificate for mTLS
    #[serde(default)]
    pub client_ca_file: Option<String>,
    
    /// Require client certificates (mutual TLS)
    #[serde(default)]
    pub require_client_cert: bool,
    
    /// Minimum TLS version (1.2 or 1.3)
    #[serde(default = "default_min_tls_version")]
    pub min_tls_version: String,
    
    /// Enable certificate reloading
    #[serde(default = "default_tls_cert_reload")]
    pub enable_cert_reload: bool,
    
    /// Certificate reload check interval in seconds
    #[serde(default = "default_tls_reload_interval")]
    pub reload_interval_secs: u64,
}

/// Compression configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompressionConfig {
    /// Enable compression
    #[serde(default = "default_compression_enabled")]
    pub enabled: bool,

    /// Compression level (1-9 for gzip/zstd, 1-11 for brotli)
    #[serde(default = "default_compression_level")]
    pub level: u32,

    /// Minimum response size to compress (in bytes)
    #[serde(default = "default_compression_min_size")]
    pub min_size: usize,

    /// Preferred compression algorithms in order
    #[serde(default = "default_compression_algorithms")]
    pub algorithms: Vec<String>,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            level: 6,
            min_size: 1024, // 1KB
            algorithms: vec![
                "br".to_string(),    // brotli (best compression)
                "zstd".to_string(),  // zstd (fast)
                "gzip".to_string(),  // gzip (universal)
            ],
        }
    }
}

/// FARP (Forge API Gateway Registration Protocol) configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct FarpConfig {
    /// Enable FARP service discovery and auto-routing
    pub enabled: bool,
    
    /// Watch interval for discovering service changes
    #[serde(with = "humantime_serde")]
    pub watch_interval: Duration,
    
    /// Schema cache TTL
    #[serde(with = "humantime_serde")]
    pub schema_cache_ttl: Duration,
    
    /// Discovery backend configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discovery: Option<FarpDiscoveryConfig>,
}

impl Default for FarpConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            watch_interval: Duration::from_secs(5),
            schema_cache_ttl: Duration::from_secs(300), // 5 minutes
            discovery: None,
        }
    }
}

/// FARP discovery backend configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FarpDiscoveryConfig {
    /// Discovery backends
    #[serde(default)]
    pub backends: Vec<DiscoveryBackendConfig>,
}

/// Individual discovery backend configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum DiscoveryBackendConfig {
    /// mDNS/Bonjour discovery
    Mdns {
        /// Enable this backend
        #[serde(default = "default_true")]
        enabled: bool,
        /// Backend-specific configuration
        config: MdnsDiscoveryConfig,
    },
    /// DNS-based discovery
    Dns {
        /// Enable this backend
        #[serde(default = "default_true")]
        enabled: bool,
        /// Backend-specific configuration
        config: DnsDiscoveryConfig,
    },
    /// Consul discovery
    Consul {
        /// Enable this backend
        #[serde(default = "default_true")]
        enabled: bool,
        /// Backend-specific configuration
        config: ConsulDiscoveryConfig,
    },
    /// Kubernetes discovery
    Kubernetes {
        /// Enable this backend
        #[serde(default = "default_true")]
        enabled: bool,
        /// Backend-specific configuration
        config: KubernetesDiscoveryConfig,
    },
}

/// mDNS discovery configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MdnsDiscoveryConfig {
    /// Service type to discover
    pub service_type: String,
    /// mDNS domain
    pub domain: String,
    /// Watch interval
    #[serde(with = "humantime_serde")]
    pub watch_interval: Duration,
    /// Query timeout
    #[serde(with = "humantime_serde")]
    pub query_timeout: Duration,
    /// Enable IPv6 discovery
    #[serde(default = "default_true")]
    pub enable_ipv6: bool,
}

/// DNS discovery configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DnsDiscoveryConfig {
    /// DNS server addresses
    pub servers: Vec<String>,
    /// Domain to query
    pub domain: String,
    /// Watch interval
    #[serde(with = "humantime_serde")]
    pub watch_interval: Duration,
}

/// Consul discovery configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConsulDiscoveryConfig {
    /// Consul address
    pub address: String,
    /// Datacenter
    pub datacenter: String,
    /// ACL token
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// Watch interval
    #[serde(with = "humantime_serde")]
    pub watch_interval: Duration,
}

/// Kubernetes discovery configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KubernetesDiscoveryConfig {
    /// Namespace to watch
    pub namespace: String,
    /// Label selector
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_selector: Option<String>,
    /// Watch interval
    #[serde(with = "humantime_serde")]
    pub watch_interval: Duration,
}

/// Helper function for default true
fn default_true() -> bool {
    true
}

/// Upstream service configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UpstreamConfig {
    /// Unique name
    pub name: String,
    
    /// Service instances
    pub instances: Vec<InstanceConfig>,
    
    /// Load balancing policy
    #[serde(default = "default_lb_policy")]
    pub lb_policy: String,
    
    /// Health check configuration
    #[serde(default)]
    pub health_check: Option<HealthCheckConfig>,
    
    /// Circuit breaker configuration
    #[serde(default)]
    pub circuit_breaker: Option<CircuitBreakerConfig>,
}

/// Instance configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InstanceConfig {
    /// Instance ID
    pub id: String,
    
    /// Host address
    pub host: String,
    
    /// Port
    pub port: u16,
    
    /// Weight for load balancing
    #[serde(default = "default_weight")]
    pub weight: u32,
    
    /// Metadata
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

/// Health check configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HealthCheckConfig {
    /// Check type (http, tcp, grpc)
    #[serde(rename = "type")]
    pub check_type: String,
    
    /// Path for HTTP checks
    #[serde(default)]
    pub path: Option<String>,
    
    /// Interval between checks
    #[serde(with = "humantime_serde")]
    pub interval: Duration,
    
    /// Timeout for each check
    #[serde(with = "humantime_serde")]
    pub timeout: Duration,
    
    /// Healthy threshold
    #[serde(default = "default_healthy_threshold")]
    pub healthy_threshold: u32,
    
    /// Unhealthy threshold
    #[serde(default = "default_unhealthy_threshold")]
    pub unhealthy_threshold: u32,
}

/// Circuit breaker configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CircuitBreakerConfig {
    /// Error threshold percentage
    pub error_threshold: f32,
    
    /// Minimum requests before activation
    pub min_requests: u32,
    
    /// Open state timeout
    #[serde(with = "humantime_serde")]
    pub timeout: Duration,
}

/// Route configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RouteConfig {
    /// Route path pattern
    pub path: String,
    
    /// HTTP methods
    #[serde(default)]
    pub methods: Vec<String>,
    
    /// Upstream name
    pub upstream: String,
    
    /// Priority (higher = matched first)
    #[serde(default)]
    pub priority: i32,
    
    /// Strip prefix
    #[serde(default)]
    pub strip_prefix: Option<String>,
    
    /// Add prefix
    #[serde(default)]
    pub add_prefix: Option<String>,
    
    /// Metadata
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

/// Plugin configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginConfig {
    /// Plugin name
    pub name: String,
    
    /// Plugin type (static, dynamic)
    #[serde(default = "default_plugin_type")]
    pub plugin_type: String,
    
    /// Enabled
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    
    /// Priority (higher = runs first)
    #[serde(default)]
    pub priority: i32,
    
    /// Configuration
    #[serde(default)]
    pub config: HashMap<String, serde_json::Value>,
}

/// Observability configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ObservabilityConfig {
    /// Logging configuration
    pub logging: LoggingConfig,
    
    /// Metrics configuration
    pub metrics: MetricsConfig,
    
    /// Tracing configuration
    pub tracing: TracingConfig,
}

/// Logging configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LoggingConfig {
    /// Log level
    pub level: String,
    
    /// Log format (json, text)
    pub format: String,
}

/// Metrics configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MetricsConfig {
    /// Enable metrics
    pub enabled: bool,
    
    /// Metrics endpoint
    pub endpoint: String,
}

/// Tracing configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TracingConfig {
    /// Enable tracing
    pub enabled: bool,
    
    /// Jaeger endpoint
    pub jaeger_endpoint: Option<String>,
}

// Default functions
fn default_timeout() -> Duration {
    Duration::from_secs(30)
}

fn default_shutdown_timeout() -> Duration {
    Duration::from_secs(30)
}

fn default_max_body_size() -> usize {
    10 * 1024 * 1024 // 10 MB
}

fn default_lb_policy() -> String {
    "round_robin".to_string()
}

fn default_weight() -> u32 {
    1
}

fn default_healthy_threshold() -> u32 {
    2
}

fn default_unhealthy_threshold() -> u32 {
    3
}

fn default_plugin_type() -> String {
    "static".to_string()
}

fn default_enabled() -> bool {
    true
}

fn default_compression_enabled() -> bool {
    true
}

fn default_compression_level() -> u32 {
    6
}

fn default_compression_min_size() -> usize {
    1024
}

fn default_compression_algorithms() -> Vec<String> {
    vec![
        "br".to_string(),
        "zstd".to_string(),
        "gzip".to_string(),
    ]
}

fn default_min_tls_version() -> String {
    "1.2".to_string()
}

fn default_tls_cert_reload() -> bool {
    true
}

fn default_tls_reload_interval() -> u64 {
    300 // 5 minutes
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            logging: LoggingConfig {
                level: "info".to_string(),
                format: "text".to_string(),
            },
            metrics: MetricsConfig {
                enabled: true,
                endpoint: "/metrics".to_string(),
            },
            tracing: TracingConfig {
                enabled: false,
                jaeger_endpoint: None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_values() {
        assert_eq!(default_timeout(), Duration::from_secs(30));
        assert_eq!(default_max_body_size(), 10 * 1024 * 1024);
        assert_eq!(default_lb_policy(), "round_robin");
        assert_eq!(default_weight(), 1);
    }

    #[test]
    fn test_observability_default() {
        let config = ObservabilityConfig::default();
        assert_eq!(config.logging.level, "info");
        assert!(config.metrics.enabled);
        assert!(!config.tracing.enabled);
    }
}


