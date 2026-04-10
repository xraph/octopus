//! Rate limiting with token bucket algorithm
//!
//! Provides both in-memory and distributed (Redis-ready) rate limiting.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use parking_lot::RwLock;
use tracing::{debug, warn};

/// Rate limit configuration
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Requests per window
    pub requests_per_window: u32,
    
    /// Time window duration
    pub window_duration: Duration,
    
    /// Burst size (max tokens that can accumulate)
    pub burst_size: u32,
    
    /// Key prefix for distributed rate limiting
    pub key_prefix: String,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_window: 100,
            window_duration: Duration::from_secs(60),
            burst_size: 150,
            key_prefix: "ratelimit".to_string(),
        }
    }
}

impl RateLimitConfig {
    /// Create a new rate limit configuration
    pub fn new(requests_per_window: u32, window_duration: Duration) -> Self {
        Self {
            requests_per_window,
            window_duration,
            burst_size: requests_per_window + (requests_per_window / 2), // 1.5x base rate
            key_prefix: "ratelimit".to_string(),
        }
    }

    /// Set burst size
    pub fn with_burst_size(mut self, burst_size: u32) -> Self {
        self.burst_size = burst_size;
        self
    }

    /// Set key prefix
    pub fn with_key_prefix(mut self, prefix: String) -> Self {
        self.key_prefix = prefix;
        self
    }
}

/// Rate limit result
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RateLimitResult {
    /// Request is allowed
    Allowed {
        /// Remaining tokens
        remaining: u32,
        /// Time until limit resets
        reset_after: Duration,
    },
    /// Request is rate limited
    Limited {
        /// Time to wait before retry
        retry_after: Duration,
    },
}

impl RateLimitResult {
    /// Check if request is allowed
    pub fn is_allowed(&self) -> bool {
        matches!(self, RateLimitResult::Allowed { .. })
    }

    /// Get retry after duration
    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            RateLimitResult::Limited { retry_after } => Some(*retry_after),
            _ => None,
        }
    }

    /// Get remaining capacity
    pub fn remaining(&self) -> Option<u32> {
        match self {
            RateLimitResult::Allowed { remaining, .. } => Some(*remaining),
            _ => None,
        }
    }
}

/// Token bucket for rate limiting
#[derive(Debug)]
struct TokenBucket {
    /// Number of available tokens
    tokens: f64,
    
    /// Maximum number of tokens (burst size)
    capacity: f64,
    
    /// Rate at which tokens are replenished (tokens per second)
    refill_rate: f64,
    
    /// Last update time
    last_update: Instant,
}

impl TokenBucket {
    /// Create a new token bucket
    fn new(capacity: u32, refill_rate: f64) -> Self {
        Self {
            tokens: capacity as f64,
            capacity: capacity as f64,
            refill_rate,
            last_update: Instant::now(),
        }
    }

    /// Try to consume a token
    fn try_consume(&mut self, tokens: u32) -> bool {
        self.refill();
        
        let tokens_f64 = tokens as f64;
        if self.tokens >= tokens_f64 {
            self.tokens -= tokens_f64;
            debug!(
                tokens = self.tokens,
                capacity = self.capacity,
                "Token consumed"
            );
            true
        } else {
            warn!(
                tokens = self.tokens,
                requested = tokens,
                "Rate limit exceeded"
            );
            false
        }
    }

    /// Refill tokens based on elapsed time
    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_update).as_secs_f64();
        
        if elapsed > 0.0 {
            let new_tokens = elapsed * self.refill_rate;
            self.tokens = (self.tokens + new_tokens).min(self.capacity);
            self.last_update = now;
        }
    }

    /// Get remaining tokens
    fn remaining(&mut self) -> u32 {
        self.refill();
        self.tokens.floor() as u32
    }

    /// Get time until next token is available
    fn time_until_available(&mut self, needed_tokens: u32) -> Duration {
        self.refill();
        
        let deficit = (needed_tokens as f64) - self.tokens;
        if deficit <= 0.0 {
            return Duration::from_secs(0);
        }
        
        let seconds = deficit / self.refill_rate;
        Duration::from_secs_f64(seconds)
    }
}

/// In-memory rate limiter using token bucket algorithm
pub struct InMemoryRateLimiter {
    config: RateLimitConfig,
    buckets: Arc<RwLock<HashMap<String, TokenBucket>>>,
}

impl InMemoryRateLimiter {
    /// Create a new in-memory rate limiter
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            buckets: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Check rate limit for a key
    pub fn check(&self, key: &str) -> RateLimitResult {
        self.check_tokens(key, 1)
    }

    /// Check rate limit with custom token cost
    pub fn check_tokens(&self, key: &str, tokens: u32) -> RateLimitResult {
        let full_key = format!("{}:{}", self.config.key_prefix, key);
        
        // Fast path: read lock first to check if bucket exists
        {
            let buckets = self.buckets.read();
            if let Some(bucket) = buckets.get(&full_key) {
                // Need to drop read lock before getting write lock
                drop(buckets);
                
                // Now get write lock to modify
                let mut buckets = self.buckets.write();
                if let Some(bucket) = buckets.get_mut(&full_key) {
                    return self.check_bucket(bucket, tokens);
                }
            }
        }

        // Slow path: create new bucket
        let mut buckets = self.buckets.write();
        let bucket = buckets.entry(full_key).or_insert_with(|| {
            let refill_rate = self.config.requests_per_window as f64 
                / self.config.window_duration.as_secs_f64();
            TokenBucket::new(self.config.burst_size, refill_rate)
        });
        
        self.check_bucket(bucket, tokens)
    }

    /// Check a specific bucket
    fn check_bucket(&self, bucket: &mut TokenBucket, tokens: u32) -> RateLimitResult {
        if bucket.try_consume(tokens) {
            RateLimitResult::Allowed {
                remaining: bucket.remaining(),
                reset_after: self.config.window_duration,
            }
        } else {
            RateLimitResult::Limited {
                retry_after: bucket.time_until_available(tokens),
            }
        }
    }

    /// Get current state for a key
    pub fn get_state(&self, key: &str) -> Option<(u32, Duration)> {
        let full_key = format!("{}:{}", self.config.key_prefix, key);
        let mut buckets = self.buckets.write();
        
        buckets.get_mut(&full_key).map(|bucket| {
            let remaining = bucket.remaining();
            let reset_after = self.config.window_duration;
            (remaining, reset_after)
        })
    }

    /// Clear all rate limit state
    pub fn clear(&self) {
        self.buckets.write().clear();
    }

    /// Clear rate limit state for a specific key
    pub fn clear_key(&self, key: &str) {
        let full_key = format!("{}:{}", self.config.key_prefix, key);
        self.buckets.write().remove(&full_key);
    }

    /// Get configuration
    pub fn config(&self) -> &RateLimitConfig {
        &self.config
    }

    /// Cleanup expired buckets (call periodically)
    pub fn cleanup_expired(&self) {
        let mut buckets = self.buckets.write();
        let now = Instant::now();
        
        buckets.retain(|_, bucket| {
            // Remove buckets that haven't been used in 2x window duration
            now.duration_since(bucket.last_update) < (self.config.window_duration * 2)
        });
    }
}

impl std::fmt::Debug for InMemoryRateLimiter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InMemoryRateLimiter")
            .field("config", &self.config)
            .field("buckets_count", &self.buckets.read().len())
            .finish()
    }
}

/// Rate limiter trait for abstraction over storage backends
pub trait RateLimiter: Send + Sync {
    /// Check rate limit for a key
    fn check(&self, key: &str) -> RateLimitResult;
    
    /// Check rate limit with custom token cost
    fn check_tokens(&self, key: &str, tokens: u32) -> RateLimitResult;
    
    /// Clear all rate limit state
    fn clear(&self);
}

impl RateLimiter for InMemoryRateLimiter {
    fn check(&self, key: &str) -> RateLimitResult {
        self.check(key)
    }

    fn check_tokens(&self, key: &str, tokens: u32) -> RateLimitResult {
        self.check_tokens(key, tokens)
    }

    fn clear(&self) {
        self.clear()
    }
}

/// Rate limit key builder for common patterns
pub struct RateLimitKeyBuilder;

impl RateLimitKeyBuilder {
    /// Build key from client IP
    pub fn by_ip(ip: &str) -> String {
        format!("ip:{}", ip)
    }

    /// Build key from user ID
    pub fn by_user(user_id: &str) -> String {
        format!("user:{}", user_id)
    }

    /// Build key from API key
    pub fn by_api_key(api_key: &str) -> String {
        format!("apikey:{}", api_key)
    }

    /// Build key from path
    pub fn by_path(path: &str) -> String {
        format!("path:{}", path)
    }

    /// Build composite key
    pub fn composite(parts: &[&str]) -> String {
        parts.join(":")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_token_bucket() {
        let mut bucket = TokenBucket::new(10, 10.0); // 10 tokens, refill 10/sec
        
        assert!(bucket.try_consume(5));
        assert_eq!(bucket.remaining(), 5);
        
        assert!(bucket.try_consume(5));
        assert_eq!(bucket.remaining(), 0);
        
        assert!(!bucket.try_consume(1)); // Should fail
    }

    #[test]
    fn test_token_refill() {
        let mut bucket = TokenBucket::new(10, 10.0);
        
        bucket.try_consume(10);
        assert_eq!(bucket.remaining(), 0);
        
        thread::sleep(Duration::from_millis(100)); // Wait 0.1s
        
        // Should have ~1 token refilled
        assert!(bucket.remaining() >= 1);
    }

    #[tokio::test]
    async fn test_rate_limiter_basic() {
        // Use a very long window so refill rate is negligible during test
        // Set burst_size = requests_per_window to avoid extra tokens
        let config = RateLimitConfig::new(10, Duration::from_secs(3600))
            .with_burst_size(10);
        let limiter = InMemoryRateLimiter::new(config);
        
        // Consume all tokens quickly to minimize refill
        let results: Vec<_> = (0..11).map(|_| limiter.check("test-key")).collect();
        
        // First 10 should succeed
        for i in 0..10 {
            assert!(results[i].is_allowed(), "Request {} should be allowed", i);
        }
        
        // 11th request should be rate limited
        assert!(!results[10].is_allowed(), "Request 11 should be rate limited");
    }

    #[test]
    fn test_rate_limiter_burst() {
        let config = RateLimitConfig {
            requests_per_window: 10,
            window_duration: Duration::from_secs(1),
            burst_size: 15,
            key_prefix: "test".to_string(),
        };
        let limiter = InMemoryRateLimiter::new(config);
        
        // Can consume up to burst size
        for _ in 0..15 {
            let result = limiter.check("test-key");
            assert!(result.is_allowed());
        }
        
        // Next request should fail
        let result = limiter.check("test-key");
        assert!(!result.is_allowed());
    }

    #[tokio::test]
    async fn test_rate_limiter_multiple_keys() {
        // Use a very long window so refill rate is negligible during test
        // Set burst_size = requests_per_window to avoid extra tokens
        let config = RateLimitConfig::new(5, Duration::from_secs(3600))
            .with_burst_size(5);
        let limiter = InMemoryRateLimiter::new(config);
        
        // Consume all tokens quickly for both keys
        let key1_results: Vec<_> = (0..6).map(|_| limiter.check("key1")).collect();
        let key2_results: Vec<_> = (0..6).map(|_| limiter.check("key2")).collect();
        
        // First 5 should succeed for each key
        for i in 0..5 {
            assert!(key1_results[i].is_allowed(), "key1 request {} should be allowed", i);
            assert!(key2_results[i].is_allowed(), "key2 request {} should be allowed", i);
        }
        
        // 6th request should be limited for both
        assert!(!key1_results[5].is_allowed(), "key1 request 6 should be rate limited");
        assert!(!key2_results[5].is_allowed(), "key2 request 6 should be rate limited");
    }

    #[tokio::test]
    async fn test_rate_limiter_clear() {
        // Use a very long window so refill rate is negligible during test
        // Set burst_size = requests_per_window to avoid extra tokens
        let config = RateLimitConfig::new(2, Duration::from_secs(3600))
            .with_burst_size(2);
        let limiter = InMemoryRateLimiter::new(config);
        
        // Consume all tokens quickly
        let results: Vec<_> = (0..3).map(|_| limiter.check("test-key")).collect();
        
        // First 2 should succeed
        assert!(results[0].is_allowed());
        assert!(results[1].is_allowed());
        
        // 3rd should be limited
        assert!(!results[2].is_allowed());
        
        // Clear and try again
        limiter.clear();
        assert!(limiter.check("test-key").is_allowed());
    }

    #[test]
    fn test_key_builder() {
        assert_eq!(RateLimitKeyBuilder::by_ip("192.168.1.1"), "ip:192.168.1.1");
        assert_eq!(RateLimitKeyBuilder::by_user("user123"), "user:user123");
        assert_eq!(RateLimitKeyBuilder::by_api_key("abc123"), "apikey:abc123");
        assert_eq!(RateLimitKeyBuilder::by_path("/api/users"), "path:/api/users");
        
        let composite = RateLimitKeyBuilder::composite(&["ip", "192.168.1.1", "path", "/api"]);
        assert_eq!(composite, "ip:192.168.1.1:path:/api");
    }

    #[test]
    fn test_rate_limit_result() {
        let allowed = RateLimitResult::Allowed {
            remaining: 5,
            reset_after: Duration::from_secs(60),
        };
        
        assert!(allowed.is_allowed());
        assert_eq!(allowed.remaining(), Some(5));
        assert_eq!(allowed.retry_after(), None);
        
        let limited = RateLimitResult::Limited {
            retry_after: Duration::from_secs(10),
        };
        
        assert!(!limited.is_allowed());
        assert_eq!(limited.remaining(), None);
        assert_eq!(limited.retry_after(), Some(Duration::from_secs(10)));
    }

    #[test]
    fn test_token_bucket_time_until_available() {
        let mut bucket = TokenBucket::new(10, 10.0); // 10 tokens/sec
        
        bucket.try_consume(10);
        
        let wait_time = bucket.time_until_available(1);
        assert!(wait_time.as_secs_f64() > 0.0 && wait_time.as_secs_f64() <= 0.2);
    }
}
