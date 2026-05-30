//! Retry logic with exponential backoff for transient failures

use http::{Method, Request, Response, StatusCode};
use octopus_core::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::time::Duration;
use tracing::{debug, warn};

/// Retry policy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicy {
    /// Maximum number of retry attempts (not including initial request)
    pub max_attempts: u32,

    /// Backoff strategy
    pub backoff: BackoffStrategy,

    /// HTTP methods that are safe to retry (idempotent)
    #[serde(skip)]
    pub retryable_methods: HashSet<Method>,

    /// HTTP status codes that should trigger a retry
    pub retryable_status_codes: HashSet<u16>,

    /// Timeout per attempt
    #[serde(with = "humantime_serde")]
    pub timeout_per_attempt: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        let mut retryable_methods = HashSet::new();
        retryable_methods.insert(Method::GET);
        retryable_methods.insert(Method::HEAD);
        retryable_methods.insert(Method::PUT);
        retryable_methods.insert(Method::DELETE);
        retryable_methods.insert(Method::OPTIONS);
        retryable_methods.insert(Method::TRACE);

        let mut retryable_status_codes = HashSet::new();
        retryable_status_codes.insert(408); // Request Timeout
        retryable_status_codes.insert(429); // Too Many Requests
        retryable_status_codes.insert(502); // Bad Gateway
        retryable_status_codes.insert(503); // Service Unavailable
        retryable_status_codes.insert(504); // Gateway Timeout

        Self {
            max_attempts: 3,
            backoff: BackoffStrategy::default(),
            retryable_methods,
            retryable_status_codes,
            timeout_per_attempt: Duration::from_secs(30),
        }
    }
}

impl RetryPolicy {
    /// Create a new retry policy
    pub fn new() -> Self {
        Self::default()
    }

    /// Set maximum retry attempts
    pub fn with_max_attempts(mut self, max_attempts: u32) -> Self {
        self.max_attempts = max_attempts;
        self
    }

    /// Set backoff strategy
    pub fn with_backoff(mut self, backoff: BackoffStrategy) -> Self {
        self.backoff = backoff;
        self
    }

    /// Set timeout per attempt
    pub fn with_timeout_per_attempt(mut self, timeout: Duration) -> Self {
        self.timeout_per_attempt = timeout;
        self
    }

    /// Check if method is retryable
    pub fn is_method_retryable(&self, method: &Method) -> bool {
        self.retryable_methods.contains(method)
    }

    /// Check if status code should trigger retry
    pub fn is_status_retryable(&self, status: StatusCode) -> bool {
        self.retryable_status_codes.contains(&status.as_u16())
    }

    /// Check if error is retryable
    pub fn is_error_retryable(&self, error: &Error) -> bool {
        matches!(error, Error::UpstreamTimeout | Error::UpstreamConnection(_))
    }

    /// Calculate backoff delay for attempt number
    pub fn calculate_backoff(&self, attempt: u32) -> Duration {
        self.backoff.calculate(attempt)
    }
}

/// Backoff strategy for retries
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BackoffStrategy {
    /// Exponential backoff with jitter
    Exponential {
        /// Initial delay
        #[serde(with = "humantime_serde")]
        initial: Duration,

        /// Maximum delay
        #[serde(with = "humantime_serde")]
        max: Duration,

        /// Multiplier factor (typically 2.0)
        factor: f64,

        /// Enable jitter to avoid thundering herd
        jitter: bool,
    },

    /// Linear backoff
    Linear {
        /// Interval between retries
        #[serde(with = "humantime_serde")]
        interval: Duration,
    },

    /// Fixed delay
    Fixed {
        /// Fixed delay between retries
        #[serde(with = "humantime_serde")]
        delay: Duration,
    },
}

impl Default for BackoffStrategy {
    fn default() -> Self {
        Self::Exponential {
            initial: Duration::from_millis(100),
            max: Duration::from_secs(10),
            factor: 2.0,
            jitter: true,
        }
    }
}

impl BackoffStrategy {
    /// Calculate backoff delay for attempt number
    pub fn calculate(&self, attempt: u32) -> Duration {
        match self {
            Self::Exponential {
                initial,
                max,
                factor,
                jitter,
            } => {
                let base_delay = initial.as_millis() as f64 * factor.powi(attempt as i32);
                let mut delay = Duration::from_millis(base_delay as u64);

                // Cap at max delay
                if delay > *max {
                    delay = *max;
                }

                // Add jitter if enabled
                if *jitter {
                    delay = add_jitter(delay);
                }

                delay
            }
            Self::Linear { interval } => *interval * attempt,
            Self::Fixed { delay } => *delay,
        }
    }
}

/// Add jitter to a duration (±25% randomness)
fn add_jitter(duration: Duration) -> Duration {
    use rand::Rng;

    let millis = duration.as_millis() as f64;
    let jitter_range = millis * 0.25;
    let jitter = rand::thread_rng().gen_range(-jitter_range..=jitter_range);

    Duration::from_millis((millis + jitter).max(0.0) as u64)
}

/// Retry context for tracking retry state
#[derive(Debug)]
pub struct RetryContext {
    /// Current attempt number (0-indexed)
    pub attempt: u32,

    /// Total attempts made so far
    pub total_attempts: u32,

    /// Last error encountered
    pub last_error: Option<Error>,

    /// Last status code received
    pub last_status: Option<StatusCode>,
}

impl RetryContext {
    /// Create a new retry context
    pub fn new() -> Self {
        Self {
            attempt: 0,
            total_attempts: 0,
            last_error: None,
            last_status: None,
        }
    }

    /// Check if should retry based on policy
    pub fn should_retry<B>(
        &self,
        policy: &RetryPolicy,
        request: &Request<B>,
        result: &Result<Response<impl http_body::Body>>,
    ) -> bool {
        // Check if we've exceeded max attempts
        if self.attempt >= policy.max_attempts {
            debug!(
                attempt = self.attempt,
                max_attempts = policy.max_attempts,
                "Max retry attempts reached"
            );
            return false;
        }

        // Check if method is retryable
        if !policy.is_method_retryable(request.method()) {
            debug!(
                method = %request.method(),
                "Method is not retryable"
            );
            return false;
        }

        // Check result
        match result {
            Ok(response) => {
                // Check if status code is retryable
                if policy.is_status_retryable(response.status()) {
                    debug!(
                        status = response.status().as_u16(),
                        attempt = self.attempt,
                        "Retryable status code, will retry"
                    );
                    true
                } else {
                    false
                }
            }
            Err(error) => {
                // Check if error is retryable
                if policy.is_error_retryable(error) {
                    debug!(
                        error = %error,
                        attempt = self.attempt,
                        "Retryable error, will retry"
                    );
                    true
                } else {
                    warn!(
                        error = %error,
                        "Non-retryable error, giving up"
                    );
                    false
                }
            }
        }
    }

    /// Record attempt
    pub fn record_attempt(&mut self) {
        self.attempt += 1;
        self.total_attempts += 1;
    }

    /// Record error
    pub fn record_error(&mut self, error: Error) {
        self.last_error = Some(error);
    }

    /// Record status
    pub fn record_status(&mut self, status: StatusCode) {
        self.last_status = Some(status);
    }
}

impl Default for RetryContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse Retry-After header value
pub fn parse_retry_after(value: &str) -> Option<Duration> {
    // Try parsing as seconds
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }

    // Try parsing as HTTP date
    if let Ok(date) = httpdate::parse_http_date(value) {
        let now = std::time::SystemTime::now();
        if let Ok(duration) = date.duration_since(now) {
            return Some(duration);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_retry_policy() {
        let policy = RetryPolicy::default();
        assert_eq!(policy.max_attempts, 3);
        assert!(policy.is_method_retryable(&Method::GET));
        assert!(!policy.is_method_retryable(&Method::POST));
        assert!(policy.is_status_retryable(StatusCode::from_u16(503).unwrap()));
        assert!(!policy.is_status_retryable(StatusCode::from_u16(200).unwrap()));
    }

    #[test]
    fn test_exponential_backoff() {
        let strategy = BackoffStrategy::Exponential {
            initial: Duration::from_millis(100),
            max: Duration::from_secs(10),
            factor: 2.0,
            jitter: false,
        };

        assert_eq!(strategy.calculate(0), Duration::from_millis(100));
        assert_eq!(strategy.calculate(1), Duration::from_millis(200));
        assert_eq!(strategy.calculate(2), Duration::from_millis(400));
        assert_eq!(strategy.calculate(3), Duration::from_millis(800));
    }

    #[test]
    fn test_linear_backoff() {
        let strategy = BackoffStrategy::Linear {
            interval: Duration::from_millis(500),
        };

        assert_eq!(strategy.calculate(0), Duration::from_millis(0));
        assert_eq!(strategy.calculate(1), Duration::from_millis(500));
        assert_eq!(strategy.calculate(2), Duration::from_millis(1000));
    }

    #[test]
    fn test_fixed_backoff() {
        let strategy = BackoffStrategy::Fixed {
            delay: Duration::from_millis(300),
        };

        assert_eq!(strategy.calculate(0), Duration::from_millis(300));
        assert_eq!(strategy.calculate(1), Duration::from_millis(300));
        assert_eq!(strategy.calculate(5), Duration::from_millis(300));
    }

    #[test]
    fn test_retry_context() {
        let mut context = RetryContext::new();
        assert_eq!(context.attempt, 0);
        assert_eq!(context.total_attempts, 0);

        context.record_attempt();
        assert_eq!(context.attempt, 1);
        assert_eq!(context.total_attempts, 1);

        context.record_status(StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(context.last_status, Some(StatusCode::SERVICE_UNAVAILABLE));
    }

    #[test]
    fn test_parse_retry_after() {
        // Seconds format
        assert_eq!(parse_retry_after("120"), Some(Duration::from_secs(120)));

        // Invalid format
        assert_eq!(parse_retry_after("invalid"), None);
    }

    #[test]
    fn test_builder_pattern() {
        let policy = RetryPolicy::new()
            .with_max_attempts(5)
            .with_timeout_per_attempt(Duration::from_secs(10));

        assert_eq!(policy.max_attempts, 5);
        assert_eq!(policy.timeout_per_attempt, Duration::from_secs(10));
    }
}
