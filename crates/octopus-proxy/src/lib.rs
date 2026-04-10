//! # Octopus HTTP Proxy
//!
//! High-performance HTTP proxy with:
//! - Real connection pooling (HTTP/1.1 and HTTP/2)
//! - Zero-copy proxying
//! - Request/response size limits
//! - Timeout handling
//! - Retry logic
//! - Request/response transformation

#![forbid(unsafe_code)]
#![warn(
    missing_docs,
    missing_debug_implementations,
    rust_2018_idioms,
    unreachable_pub
)]

pub mod audit;
pub mod bulkhead;
pub mod client;
pub mod headers;
pub mod limits;
pub mod metrics;
pub mod pool;
pub mod proxy;
pub mod ratelimit;
pub mod retry;
pub mod routing;
pub mod shutdown;
pub mod timeout;
pub mod tls;
pub mod tracing_support;

pub use audit::{AuditEvent, AuditEventType, AuditLogger};
pub use bulkhead::{Bulkhead, BulkheadConfig, BulkheadError, BulkheadPermit};
pub use client::HttpClient;
pub use headers::{HeaderConfig, HeaderProcessor};
pub use limits::{LimitedBody, ProxyLimits};
pub use metrics::{
    CircuitBreakerMetrics, PoolMetrics, ProxyMetrics, RequestTracker, RetryMetrics, TlsMetrics,
};
pub use pool::{ConnectionPool, PoolConfig, PoolStats, PooledConnection, UpstreamKey};
pub use proxy::{HttpProxy, ProxyConfig};
pub use ratelimit::{InMemoryRateLimiter, RateLimitConfig, RateLimitKeyBuilder, RateLimitResult, RateLimiter};
pub use retry::{BackoffStrategy, RetryContext, RetryPolicy};
pub use routing::{CanaryConfig, Router, RoutingConfig, RoutingStrategy, ShadowConfig};
pub use shutdown::{ShutdownHandle, ShutdownSignal};
pub use timeout::{TimeoutConfig, TimeoutContext, TimeoutOperation};
pub use tls::TlsConfig;
pub use tracing_support::{TraceContext, TraceContextMiddleware};

/// Re-export commonly used types
pub mod prelude {
    pub use crate::audit::{AuditEvent, AuditEventType, AuditLogger};
    pub use crate::client::HttpClient;
    pub use crate::limits::{LimitedBody, ProxyLimits};
    pub use crate::metrics::{ProxyMetrics, RequestTracker};
    pub use crate::pool::{ConnectionPool, PoolConfig, PoolStats};
    pub use crate::proxy::{HttpProxy, ProxyConfig};
    pub use crate::retry::{BackoffStrategy, RetryContext, RetryPolicy};
    pub use crate::timeout::{TimeoutConfig, TimeoutContext, TimeoutOperation};
    pub use crate::tls::TlsConfig;
    pub use crate::tracing_support::{TraceContext, TraceContextMiddleware};
}
