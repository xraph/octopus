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
pub mod builder;
pub mod compression;
pub mod connection_limits;
pub mod cors;
pub mod jwt;
pub mod logging;
pub mod rate_limit;
pub mod request_id;
pub mod request_limits;
pub mod security_headers;
pub mod timeout;

pub use audit_logger::{
    AuditEvent, AuditEventType, AuditHandler, AuditLogger, AuditLoggerConfig, AuditOutput,
};
pub use builder::MiddlewareBuilder;
pub use compression::{Compression, CompressionAlgorithm, CompressionConfig};
pub use connection_limits::{ConnectionLimits, ConnectionLimitsConfig};
pub use cors::{Cors, CorsConfig};
pub use jwt::{Claims, JwtAuth, JwtConfig};
pub use logging::{LoggingConfig, RequestLogger};
pub use rate_limit::{KeyExtractor, RateLimit, RateLimitConfig, RateLimitStrategy, RouteRateLimit};
pub use request_id::{IdGenerator, RequestId, RequestIdConfig};
pub use request_limits::{RequestLimits, RequestLimitsConfig};
pub use security_headers::{SecurityHeaders, SecurityHeadersConfig};
pub use timeout::{Timeout, TimeoutConfig};

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
