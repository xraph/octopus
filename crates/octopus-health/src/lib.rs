//! # Octopus Health System
//!
//! Health checking and circuit breaker with:
//! - Active health checks (HTTP, TCP, gRPC)
//! - Passive health checks (request success/failure tracking)
//! - Circuit breaker pattern
//! - Health state tracking
//! - Configurable thresholds

#![forbid(unsafe_code)]
#![warn(
    missing_docs,
    missing_debug_implementations,
    rust_2018_idioms,
    unreachable_pub
)]

pub mod checker;
pub mod circuit_breaker;
pub mod tracker;

pub use checker::{
    HealthCheck, HealthCheckConfig, HealthCheckResult, HealthCheckType, HealthChecker,
    HealthStatus, HttpHealthCheck, TcpHealthCheck,
};
pub use circuit_breaker::{
    CircuitBreaker, CircuitBreakerConfig, CircuitBreakerMetrics, CircuitState,
};
pub use tracker::{HealthMetrics, HealthSnapshot, HealthTracker, HealthTrackerConfig};

/// Re-export commonly used types
pub mod prelude {
    pub use crate::checker::{
        HealthCheck, HealthCheckConfig, HealthCheckResult, HealthCheckType, HealthChecker,
        HealthStatus, HttpHealthCheck, TcpHealthCheck,
    };
    pub use crate::circuit_breaker::{
        CircuitBreaker, CircuitBreakerConfig, CircuitBreakerMetrics, CircuitState,
    };
    pub use crate::tracker::{HealthMetrics, HealthSnapshot, HealthTracker, HealthTrackerConfig};
}

