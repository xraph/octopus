//! # Octopus Middleware
//!
//! Built-in middleware collection with:
//! - CORS (Cross-Origin Resource Sharing)
//! - Compression (gzip, brotli, zstd)
//! - Request logging
//! - Rate limiting
//! - Timeout enforcement
//! - Request ID injection

#![forbid(unsafe_code)]
#![warn(
    missing_docs,
    missing_debug_implementations,
    rust_2018_idioms,
    unreachable_pub
)]

pub mod audit_logger;
pub mod auth_gateway;
pub mod body_transform;
pub mod bot_detection;
pub mod builder;
pub mod caching;
pub mod canary;
pub mod circuit_breaker;
pub mod compression;
pub mod connection_limits;
pub mod cors;
pub mod deduplication;
pub mod forward_auth;
pub mod header_transform;
pub mod ip_filter;
pub mod jwt;
pub mod logging;
pub mod rate_limit;
pub mod redirect;
pub mod request_id;
pub mod request_limits;
pub mod retry;
pub mod security_headers;
pub mod timeout;
pub mod waf;

pub use audit_logger::{
    AuditEvent, AuditEventType, AuditHandler, AuditLogger, AuditLoggerConfig, AuditOutput,
};
pub use auth_gateway::{
    AuthGatewayMiddleware, AuthRateLimitKey, MatchedRouteAuth, MatchedRouteCors, ResolvedGateway,
};
pub use body_transform::{BodyRule, BodyTransform, BodyTransformConfig};
pub use bot_detection::{BotDetection, BotDetectionConfig, BotMode};
pub use builder::MiddlewareBuilder;
pub use caching::{CacheStore, CachedResponse, Caching, CachingConfig, InMemoryCacheStore};
pub use canary::{Canary, CanaryConfig, CanaryRule, CanaryUpstream};
pub use circuit_breaker::{CircuitBreaker, CircuitBreakerConfig};
pub use compression::{Compression, CompressionAlgorithm, CompressionConfig};
pub use connection_limits::{ConnectionLimits, ConnectionLimitsConfig};
pub use cors::{Cors, CorsConfig};
pub use deduplication::{Deduplication, DeduplicationConfig};
pub use forward_auth::{ForwardAuth, ForwardAuthConfig};
pub use header_transform::{HeaderRules, HeaderTransform, HeaderTransformConfig};
pub use ip_filter::{IpFilter, IpFilterConfig, IpPattern};
pub use jwt::{Claims, JwtAuth, JwtConfig};
pub use logging::{LoggingConfig, RequestLogger};
pub use rate_limit::{
    KeyExtractor, MatchedRouteRateLimit, RateLimit, RateLimitConfig, RateLimitStrategy,
    RouteRateLimit,
};
pub use redirect::{Redirect, RedirectConfig, RedirectRule, TrailingSlash};
pub use request_id::{IdGenerator, RequestId, RequestIdConfig};
pub use request_limits::{RequestLimits, RequestLimitsConfig};
pub use retry::{Retry, RetryConfig};
pub use security_headers::{SecurityHeaders, SecurityHeadersConfig};
pub use timeout::{Timeout, TimeoutConfig};
pub use waf::{Waf, WafConfig, WafMode, WafRule, WafTarget};

#[cfg(feature = "distributed")]
pub use rate_limit::{DistributedRateLimit, DistributedRateLimitConfig, RouteRateLimiter};

// Re-export core middleware types from octopus-core
pub use octopus_core::middleware::{Middleware, Next};

/// Re-export commonly used types
pub mod prelude {
    pub use crate::builder::MiddlewareBuilder;
    pub use crate::compression::{Compression, CompressionAlgorithm, CompressionConfig};
    pub use crate::cors::{Cors, CorsConfig};
    pub use crate::logging::{LoggingConfig, RequestLogger};
    pub use crate::rate_limit::{KeyExtractor, RateLimit, RateLimitConfig, RateLimitStrategy};
    pub use crate::request_id::{IdGenerator, RequestId, RequestIdConfig};
    pub use crate::timeout::{Timeout, TimeoutConfig};
    pub use octopus_core::middleware::{Middleware, Next};
}
