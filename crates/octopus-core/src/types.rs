//! Common types used throughout Octopus

use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::Duration;

/// Load balancing strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoadBalanceStrategy {
    /// Round-robin load balancing
    RoundRobin,
    /// Least connections
    LeastConnections,
    /// Weighted round-robin
    WeightedRoundRobin,
    /// Random selection
    Random,
    /// IP hash
    IpHash,
}

impl Default for LoadBalanceStrategy {
    fn default() -> Self {
        Self::RoundRobin
    }
}

/// Health check configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckConfig {
    /// Health check interval
    #[serde(with = "humantime_serde")]
    pub interval: Duration,

    /// Request timeout
    #[serde(with = "humantime_serde")]
    pub timeout: Duration,

    /// Health check path
    pub path: String,

    /// Expected status code
    pub expected_status: u16,

    /// Number of consecutive successes to mark healthy
    pub healthy_threshold: u32,

    /// Number of consecutive failures to mark unhealthy
    pub unhealthy_threshold: u32,
}

impl Default for HealthCheckConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(10),
            timeout: Duration::from_secs(5),
            path: "/health".to_string(),
            expected_status: 200,
            healthy_threshold: 2,
            unhealthy_threshold: 3,
        }
    }
}

/// Circuit breaker state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitBreakerState {
    /// Circuit is closed (normal operation)
    Closed,
    /// Circuit is open (failing fast)
    Open,
    /// Circuit is half-open (testing if backend recovered)
    HalfOpen,
}

impl fmt::Display for CircuitBreakerState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Closed => write!(f, "closed"),
            Self::Open => write!(f, "open"),
            Self::HalfOpen => write!(f, "half_open"),
        }
    }
}

/// Circuit breaker configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerConfig {
    /// Failure threshold before opening circuit
    pub failure_threshold: u32,

    /// Success threshold to close circuit from half-open
    pub success_threshold: u32,

    /// Timeout before transitioning from open to half-open
    #[serde(with = "humantime_serde")]
    pub timeout: Duration,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            success_threshold: 2,
            timeout: Duration::from_secs(60),
        }
    }
}

/// Timeout configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeoutConfig {
    /// Connect timeout
    #[serde(with = "humantime_serde")]
    pub connect: Duration,

    /// Request timeout
    #[serde(with = "humantime_serde")]
    pub request: Duration,

    /// Idle connection timeout
    #[serde(with = "humantime_serde")]
    pub idle: Duration,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            connect: Duration::from_secs(5),
            request: Duration::from_secs(30),
            idle: Duration::from_secs(90),
        }
    }
}

/// Retry policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicy {
    /// Maximum number of retries
    pub max_retries: u32,

    /// Base delay for exponential backoff
    #[serde(with = "humantime_serde")]
    pub base_delay: Duration,

    /// Maximum delay
    #[serde(with = "humantime_serde")]
    pub max_delay: Duration,

    /// HTTP status codes that should trigger retry
    pub retryable_status_codes: Vec<u16>,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(10),
            retryable_status_codes: vec![502, 503, 504],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_balance_strategy_serde() {
        let strategy = LoadBalanceStrategy::RoundRobin;
        let json = serde_json::to_string(&strategy).unwrap();
        assert_eq!(json, "\"round_robin\"");

        let deserialized: LoadBalanceStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, strategy);
    }

    #[test]
    fn test_circuit_breaker_state_display() {
        assert_eq!(CircuitBreakerState::Closed.to_string(), "closed");
        assert_eq!(CircuitBreakerState::Open.to_string(), "open");
        assert_eq!(CircuitBreakerState::HalfOpen.to_string(), "half_open");
    }
}


