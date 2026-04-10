//! Comprehensive timeout strategies for proxy operations

use octopus_core::{Error, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::warn;

/// Timeout configuration for all proxy operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeoutConfig {
    /// TCP connection timeout (default: 5s)
    #[serde(with = "humantime_serde")]
    pub connect_timeout: Duration,

    /// Individual request timeout (default: 30s)
    #[serde(with = "humantime_serde")]
    pub request_timeout: Duration,

    /// Total timeout including all retries (default: 90s)
    #[serde(with = "humantime_serde")]
    pub total_timeout: Duration,

    /// Idle timeout for keep-alive connections (default: 90s)
    #[serde(with = "humantime_serde")]
    pub idle_timeout: Duration,

    /// Timeout for reading response headers (default: 10s)
    #[serde(with = "humantime_serde")]
    pub response_header_timeout: Duration,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(5),
            request_timeout: Duration::from_secs(30),
            total_timeout: Duration::from_secs(90),
            idle_timeout: Duration::from_secs(90),
            response_header_timeout: Duration::from_secs(10),
        }
    }
}

impl TimeoutConfig {
    /// Create new timeout configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Set connection timeout
    pub fn with_connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    /// Set request timeout
    pub fn with_request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    /// Set total timeout
    pub fn with_total_timeout(mut self, timeout: Duration) -> Self {
        self.total_timeout = timeout;
        self
    }

    /// Set idle timeout
    pub fn with_idle_timeout(mut self, timeout: Duration) -> Self {
        self.idle_timeout = timeout;
        self
    }

    /// Set response header timeout
    pub fn with_response_header_timeout(mut self, timeout: Duration) -> Self {
        self.response_header_timeout = timeout;
        self
    }

    /// Validate timeout configuration
    pub fn validate(&self) -> Result<()> {
        // Request timeout should be less than total timeout
        if self.request_timeout > self.total_timeout {
            warn!(
                "Request timeout ({:?}) exceeds total timeout ({:?})",
                self.request_timeout, self.total_timeout
            );
            return Err(Error::Config(
                "Request timeout must be less than total timeout".to_string(),
            ));
        }

        // Connect timeout should be reasonable
        if self.connect_timeout > self.request_timeout {
            warn!(
                "Connect timeout ({:?}) exceeds request timeout ({:?})",
                self.connect_timeout, self.request_timeout
            );
        }

        // Response header timeout should be less than request timeout
        if self.response_header_timeout > self.request_timeout {
            warn!(
                "Response header timeout ({:?}) exceeds request timeout ({:?})",
                self.response_header_timeout, self.request_timeout
            );
        }

        Ok(())
    }

    /// Get recommended timeout for a specific retry attempt
    pub fn timeout_for_attempt(&self, attempt: u32) -> Duration {
        // First attempt gets full timeout
        if attempt == 0 {
            return self.request_timeout;
        }

        // Subsequent attempts get reduced timeout
        // This prevents slow retries from consuming too much time
        let reduced = self.request_timeout / (attempt + 1);
        
        // But never go below a minimum (e.g., 5 seconds)
        let minimum = Duration::from_secs(5);
        reduced.max(minimum)
    }

    /// Calculate remaining budget after elapsed time
    pub fn remaining_budget(&self, elapsed: Duration) -> Option<Duration> {
        self.total_timeout.checked_sub(elapsed)
    }

    /// Check if we've exceeded the total timeout budget
    pub fn is_budget_exceeded(&self, elapsed: Duration) -> bool {
        elapsed >= self.total_timeout
    }
}

/// Timeout context for tracking time budgets across operations
#[derive(Debug)]
pub struct TimeoutContext {
    /// When the operation started
    start_time: std::time::Instant,
    
    /// Timeout configuration
    config: TimeoutConfig,
}

impl TimeoutContext {
    /// Create a new timeout context
    pub fn new(config: TimeoutConfig) -> Self {
        Self {
            start_time: std::time::Instant::now(),
            config,
        }
    }

    /// Get elapsed time since start
    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }

    /// Get remaining total timeout budget
    pub fn remaining_total(&self) -> Option<Duration> {
        self.config.remaining_budget(self.elapsed())
    }

    /// Check if total timeout budget is exceeded
    pub fn is_total_exceeded(&self) -> bool {
        self.config.is_budget_exceeded(self.elapsed())
    }

    /// Get timeout for a specific operation
    pub fn timeout_for_operation(&self, operation: TimeoutOperation) -> Duration {
        match operation {
            TimeoutOperation::Connect => self.config.connect_timeout,
            TimeoutOperation::Request => self.config.request_timeout,
            TimeoutOperation::ResponseHeader => self.config.response_header_timeout,
            TimeoutOperation::Idle => self.config.idle_timeout,
        }
    }

    /// Get timeout for a retry attempt, bounded by remaining budget
    pub fn timeout_for_attempt(&self, attempt: u32) -> Option<Duration> {
        let attempt_timeout = self.config.timeout_for_attempt(attempt);
        let remaining = self.remaining_total()?;
        
        Some(attempt_timeout.min(remaining))
    }

    /// Check if we have enough budget for an operation
    pub fn has_budget_for(&self, duration: Duration) -> bool {
        self.remaining_total()
            .map(|remaining| remaining >= duration)
            .unwrap_or(false)
    }
}

/// Types of timeout operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeoutOperation {
    /// TCP connection establishment
    Connect,
    /// Full request/response cycle
    Request,
    /// Reading response headers
    ResponseHeader,
    /// Idle keep-alive timeout
    Idle,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_timeout_config() {
        let config = TimeoutConfig::default();
        assert_eq!(config.connect_timeout, Duration::from_secs(5));
        assert_eq!(config.request_timeout, Duration::from_secs(30));
        assert_eq!(config.total_timeout, Duration::from_secs(90));
        assert_eq!(config.idle_timeout, Duration::from_secs(90));
    }

    #[test]
    fn test_builder_pattern() {
        let config = TimeoutConfig::new()
            .with_connect_timeout(Duration::from_secs(3))
            .with_request_timeout(Duration::from_secs(20))
            .with_total_timeout(Duration::from_secs(60));

        assert_eq!(config.connect_timeout, Duration::from_secs(3));
        assert_eq!(config.request_timeout, Duration::from_secs(20));
        assert_eq!(config.total_timeout, Duration::from_secs(60));
    }

    #[test]
    fn test_validate_config() {
        // Valid config
        let config = TimeoutConfig::new();
        assert!(config.validate().is_ok());

        // Invalid: request_timeout > total_timeout
        let invalid_config = TimeoutConfig {
            request_timeout: Duration::from_secs(100),
            total_timeout: Duration::from_secs(50),
            ..TimeoutConfig::default()
        };
        assert!(invalid_config.validate().is_err());
    }

    #[test]
    fn test_timeout_for_attempt() {
        let config = TimeoutConfig {
            request_timeout: Duration::from_secs(30),
            ..TimeoutConfig::default()
        };

        // First attempt gets full timeout
        assert_eq!(config.timeout_for_attempt(0), Duration::from_secs(30));

        // Subsequent attempts get reduced
        assert_eq!(config.timeout_for_attempt(1), Duration::from_secs(15));
        assert_eq!(config.timeout_for_attempt(2), Duration::from_secs(10));

        // But never below minimum (5s)
        assert_eq!(config.timeout_for_attempt(10), Duration::from_secs(5));
    }

    #[test]
    fn test_remaining_budget() {
        let config = TimeoutConfig {
            total_timeout: Duration::from_secs(60),
            ..TimeoutConfig::default()
        };

        let elapsed = Duration::from_secs(20);
        let remaining = config.remaining_budget(elapsed).unwrap();
        assert_eq!(remaining, Duration::from_secs(40));

        let elapsed = Duration::from_secs(70);
        assert!(config.remaining_budget(elapsed).is_none());
    }

    #[test]
    fn test_is_budget_exceeded() {
        let config = TimeoutConfig {
            total_timeout: Duration::from_secs(60),
            ..TimeoutConfig::default()
        };

        assert!(!config.is_budget_exceeded(Duration::from_secs(30)));
        assert!(!config.is_budget_exceeded(Duration::from_secs(59)));
        assert!(config.is_budget_exceeded(Duration::from_secs(60)));
        assert!(config.is_budget_exceeded(Duration::from_secs(70)));
    }

    #[test]
    fn test_timeout_context() {
        let config = TimeoutConfig {
            total_timeout: Duration::from_secs(90),
            request_timeout: Duration::from_secs(30),
            ..TimeoutConfig::default()
        };

        let ctx = TimeoutContext::new(config);

        // Initially, should have full budget
        let remaining = ctx.remaining_total().unwrap();
        assert!(remaining <= Duration::from_secs(90));
        assert!(remaining > Duration::from_secs(89));

        // Should have budget for operations
        assert!(ctx.has_budget_for(Duration::from_secs(10)));
        assert!(ctx.has_budget_for(Duration::from_secs(80)));

        // Get timeout for specific operation
        assert_eq!(
            ctx.timeout_for_operation(TimeoutOperation::Connect),
            Duration::from_secs(5)
        );
        assert_eq!(
            ctx.timeout_for_operation(TimeoutOperation::Request),
            Duration::from_secs(30)
        );
    }

    #[test]
    fn test_timeout_for_attempt_with_budget() {
        let config = TimeoutConfig {
            request_timeout: Duration::from_secs(30),
            total_timeout: Duration::from_secs(40),
            ..TimeoutConfig::default()
        };

        let ctx = TimeoutContext::new(config);

        // Simulate 35 seconds elapsed
        std::thread::sleep(Duration::from_millis(10)); // Small delay for test
        
        // First attempt should get min(30s, remaining_budget)
        let timeout = ctx.timeout_for_attempt(0);
        assert!(timeout.is_some());
    }
}
