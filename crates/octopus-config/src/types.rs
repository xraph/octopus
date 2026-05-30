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

    /// Named authentication providers
    #[serde(default)]
    pub auth_providers: HashMap<String, AuthProviderConfig>,

    /// Global authentication and authorization settings
    #[serde(default)]
    pub auth: AuthConfig,

    /// Global CORS configuration
    #[serde(default)]
    pub cors: Option<CorsGlobalConfig>,

    /// Admin dashboard configuration
    #[serde(default)]
    pub admin: AdminConfig,

    /// gRPC gateway configuration
    #[serde(default)]
    pub grpc: GrpcConfig,
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
                "br".to_string(),   // brotli (best compression)
                "zstd".to_string(), // zstd (fast)
                "gzip".to_string(), // gzip (universal)
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

    /// Auth provider name (overrides global default_provider)
    #[serde(default)]
    pub auth_provider: Option<String>,

    /// Skip authentication for this route
    #[serde(default)]
    pub skip_auth: bool,

    /// Required roles for authorization
    #[serde(default)]
    pub require_roles: Vec<String>,

    /// Required scopes for authorization
    #[serde(default)]
    pub require_scopes: Vec<String>,

    /// Custom authorization rule (Rhai expression)
    #[serde(default)]
    pub authz_rule: Option<String>,

    /// Per-route request timeout override
    #[serde(default, with = "humantime_serde::option")]
    pub timeout: Option<Duration>,

    /// Per-route rate limit
    #[serde(default)]
    pub rate_limit: Option<RouteRateLimitConfig>,

    /// Per-route CORS override
    #[serde(default)]
    pub cors: Option<RouteCorsConfig>,
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
    vec!["br".to_string(), "zstd".to_string(), "gzip".to_string()]
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

// ============================================================================
// Authentication & Authorization Configuration
// ============================================================================

/// Auth provider definition (tagged by type)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthProviderConfig {
    /// JWT token validation with static keys
    Jwt(JwtProviderConfig),
    /// OIDC provider with auto-discovery and JWKS refresh
    Oidc(OidcProviderConfig),
    /// API key authentication
    ApiKey(ApiKeyProviderConfig),
    /// Delegate auth to external service
    ForwardAuth(ForwardAuthProviderConfig),
    /// Mutual TLS client certificate
    Mtls(MtlsProviderConfig),
}

/// JWT provider configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JwtProviderConfig {
    /// HMAC secret for HS256/HS384/HS512
    #[serde(default)]
    pub secret: Option<String>,
    /// RSA/ECDSA public key (PEM string)
    #[serde(default)]
    pub public_key: Option<String>,
    /// Path to public key file
    #[serde(default)]
    pub public_key_file: Option<String>,
    /// Algorithm (HS256, RS256, ES256, etc.)
    #[serde(default = "default_jwt_algorithm")]
    pub algorithm: String,
    /// Expected issuer claim
    #[serde(default)]
    pub issuer: Option<String>,
    /// Expected audience claim
    #[serde(default)]
    pub audience: Option<String>,
    /// Header to extract token from
    #[serde(default = "default_auth_header")]
    pub header_name: String,
    /// Token prefix to strip
    #[serde(default = "default_token_prefix")]
    pub token_prefix: String,
}

/// OIDC provider configuration with auto-discovery
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OidcProviderConfig {
    /// OIDC issuer URL (e.g., https://accounts.google.com)
    pub issuer_url: String,
    /// Expected audience
    #[serde(default)]
    pub audience: Option<String>,
    /// JWKS key refresh interval
    #[serde(default = "default_jwks_refresh", with = "humantime_serde")]
    pub jwks_refresh_interval: Duration,
    /// Required scopes
    #[serde(default)]
    pub required_scopes: Vec<String>,
    /// Header to extract token from
    #[serde(default = "default_auth_header")]
    pub header_name: String,
    /// Token prefix
    #[serde(default = "default_token_prefix")]
    pub token_prefix: String,
    /// Fallback provider name if OIDC discovery fails
    #[serde(default)]
    pub fallback_provider: Option<String>,
}

/// API key provider configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ApiKeyProviderConfig {
    /// Header name for API key
    #[serde(default = "default_apikey_header")]
    pub header_name: String,
    /// Optional query parameter for API key
    #[serde(default)]
    pub query_param: Option<String>,
    /// Static API key entries
    #[serde(default)]
    pub keys: Vec<ApiKeyEntry>,
    /// External validator URL (POST with key in body)
    #[serde(default)]
    pub external_validator: Option<String>,
}

/// Static API key entry
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ApiKeyEntry {
    /// The API key value
    pub key: String,
    /// Human-readable name for the key holder
    pub name: String,
    /// Allowed scopes
    #[serde(default)]
    pub scopes: Vec<String>,
    /// Per-key rate limit (requests/minute)
    #[serde(default)]
    pub rate_limit: Option<u32>,
}

/// Forward auth provider configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ForwardAuthProviderConfig {
    /// External auth service endpoint
    pub endpoint: String,
    /// Headers to forward to auth service
    #[serde(default = "default_forward_headers")]
    pub forward_headers: Vec<String>,
    /// Headers to copy from auth response back to upstream request
    #[serde(default = "default_response_headers")]
    pub response_headers: Vec<String>,
    /// Auth service timeout
    #[serde(default = "default_forward_auth_timeout", with = "humantime_serde")]
    pub timeout: Duration,
    /// Cache auth responses by token hash
    #[serde(default, with = "humantime_serde::option")]
    pub cache_ttl: Option<Duration>,
}

/// mTLS provider configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MtlsProviderConfig {
    /// Client CA certificate file
    pub client_ca_file: String,
    /// Require client certificate
    #[serde(default = "default_true")]
    pub require_client_cert: bool,
    /// Extract CN as principal ID
    #[serde(default = "default_true")]
    pub extract_cn_as_principal: bool,
    /// Map certificate CN patterns to roles
    #[serde(default)]
    pub cn_to_roles: HashMap<String, Vec<String>>,
}

/// Global authentication settings
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AuthConfig {
    /// Default auth provider name (applied to routes without explicit provider)
    pub default_provider: Option<String>,
    /// Enforce authentication globally (all routes require auth unless skip_auth)
    pub global_enforce: bool,
    /// Paths to skip authentication (supports wildcards)
    pub skip_paths: Vec<String>,
    /// Header to inject authenticated principal ID
    pub principal_header: String,
    /// Header to inject authenticated principal roles
    pub roles_header: String,
    /// Header to inject authenticated scopes
    pub scopes_header: String,
    /// Cache validated tokens for this duration
    #[serde(with = "humantime_serde")]
    pub token_cache_ttl: Duration,
    /// Error response format ("json" or "text")
    pub error_format: String,
    /// Authorization settings
    pub authz: AuthzConfig,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            default_provider: None,
            global_enforce: false,
            skip_paths: vec![],
            principal_header: "X-Auth-Principal".to_string(),
            roles_header: "X-Auth-Roles".to_string(),
            scopes_header: "X-Auth-Scopes".to_string(),
            token_cache_ttl: Duration::from_secs(60),
            error_format: "json".to_string(),
            authz: AuthzConfig::default(),
        }
    }
}

/// Authorization configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AuthzConfig {
    /// Authz engine to use
    pub engine: AuthzEngine,
    /// Global authorization rules (applied to all authenticated requests)
    pub global_rules: Vec<AuthzRule>,
    /// OPA integration settings
    pub opa: Option<OpaConfig>,
}

impl Default for AuthzConfig {
    fn default() -> Self {
        Self {
            engine: AuthzEngine::Rhai,
            global_rules: vec![],
            opa: None,
        }
    }
}

/// Authorization engine selection
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AuthzEngine {
    /// Built-in Rhai scripting engine
    Rhai,
    /// External Open Policy Agent
    Opa,
    /// Both: Rhai for inline rules, OPA for complex policies
    Both,
}

/// OPA (Open Policy Agent) configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OpaConfig {
    /// OPA REST API endpoint
    pub endpoint: String,
    /// Request timeout
    #[serde(default = "default_opa_timeout", with = "humantime_serde")]
    pub timeout: Duration,
    /// Cache OPA decisions
    #[serde(default = "default_opa_cache_ttl", with = "humantime_serde")]
    pub cache_ttl: Duration,
    /// Allow request if OPA is unreachable
    #[serde(default)]
    pub fail_open: bool,
}

/// Authorization rule
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuthzRule {
    /// Rule name
    pub name: String,
    /// Rule description
    #[serde(default)]
    pub description: Option<String>,
    /// Override engine for this rule
    #[serde(default)]
    pub engine: Option<AuthzEngine>,
    /// Rule expression (Rhai script or OPA policy path)
    pub rule: String,
    /// Action when rule matches
    #[serde(default)]
    pub action: AuthzAction,
}

/// Authorization action
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AuthzAction {
    /// Allow the request
    #[default]
    Allow,
    /// Deny the request
    Deny,
}

/// Global CORS configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CorsGlobalConfig {
    /// Allowed origins (use ["*"] carefully)
    #[serde(default)]
    pub allowed_origins: Vec<String>,
    /// Allowed HTTP methods
    #[serde(default = "default_cors_methods")]
    pub allowed_methods: Vec<String>,
    /// Allowed request headers
    #[serde(default)]
    pub allowed_headers: Vec<String>,
    /// Exposed response headers
    #[serde(default)]
    pub exposed_headers: Vec<String>,
    /// Preflight cache max age in seconds
    #[serde(default = "default_cors_max_age")]
    pub max_age: u64,
    /// Allow credentials (cookies, auth headers)
    #[serde(default)]
    pub allow_credentials: bool,
}

/// Per-route CORS override
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RouteCorsConfig {
    /// Allowed origins
    #[serde(default)]
    pub allowed_origins: Vec<String>,
    /// Allowed methods
    #[serde(default)]
    pub allowed_methods: Vec<String>,
    /// Allowed headers
    #[serde(default)]
    pub allowed_headers: Vec<String>,
    /// Allow credentials
    #[serde(default)]
    pub allow_credentials: bool,
    /// Max age (seconds)
    #[serde(default = "default_cors_max_age")]
    pub max_age: u64,
}

/// Per-route rate limit configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RouteRateLimitConfig {
    /// Requests allowed per window
    pub requests_per_window: u32,
    /// Window duration
    #[serde(with = "humantime_serde")]
    pub window_size: Duration,
}

/// Admin dashboard configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
#[derive(Default)]
pub struct AdminConfig {
    /// Auth provider name for protecting admin dashboard (None = no auth)
    pub auth_provider: Option<String>,
    /// IP allowlist for admin access (empty = all allowed)
    pub allowed_ips: Vec<String>,
}

// Auth config defaults
fn default_jwt_algorithm() -> String {
    "HS256".to_string()
}

fn default_auth_header() -> String {
    "Authorization".to_string()
}

fn default_token_prefix() -> String {
    "Bearer ".to_string()
}

fn default_jwks_refresh() -> Duration {
    Duration::from_secs(3600) // 1 hour
}

fn default_apikey_header() -> String {
    "X-API-Key".to_string()
}

fn default_forward_headers() -> Vec<String> {
    vec![
        "Authorization".to_string(),
        "Cookie".to_string(),
        "X-Forwarded-For".to_string(),
    ]
}

fn default_response_headers() -> Vec<String> {
    vec![
        "X-Auth-Subject".to_string(),
        "X-Auth-Role".to_string(),
        "X-Auth-Scopes".to_string(),
    ]
}

fn default_forward_auth_timeout() -> Duration {
    Duration::from_secs(5)
}

fn default_opa_timeout() -> Duration {
    Duration::from_millis(100)
}

fn default_opa_cache_ttl() -> Duration {
    Duration::from_secs(300)
}

fn default_cors_methods() -> Vec<String> {
    vec![
        "GET".to_string(),
        "POST".to_string(),
        "PUT".to_string(),
        "DELETE".to_string(),
        "OPTIONS".to_string(),
    ]
}

fn default_cors_max_age() -> u64 {
    3600
}

// ============================================================================
// gRPC Configuration
// ============================================================================

/// gRPC gateway configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct GrpcConfig {
    /// Enable gRPC proxying
    pub enabled: bool,
    /// Maximum message size in bytes (default: 4MB)
    pub max_message_size: usize,
    /// Enable gRPC reflection proxy
    pub enable_reflection: bool,
    /// Enable gRPC-Web support (HTTP/1.1 compatible)
    pub enable_grpc_web: bool,
    /// Propagate gRPC deadlines to upstreams
    pub deadline_propagation: bool,
    /// Explicit gRPC service-to-upstream mapping
    /// Key: fully qualified service name (e.g., "users.UserService")
    /// Value: upstream name
    #[serde(default)]
    pub services: HashMap<String, String>,
}

impl Default for GrpcConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_message_size: 4 * 1024 * 1024, // 4MB
            enable_reflection: false,
            enable_grpc_web: false,
            deadline_propagation: true,
            services: HashMap::new(),
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
