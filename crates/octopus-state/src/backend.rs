//! State backend trait definition

use crate::{Error, Result};
use async_trait::async_trait;
use std::time::Duration;

/// State backend trait
///
/// Defines the interface for pluggable state storage backends.
/// All operations are async and designed for distributed systems.
#[async_trait]
pub trait StateBackend: Send + Sync + Clone + 'static {
    /// Get a value by key
    ///
    /// Returns `None` if the key doesn't exist or has expired.
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>>;

    /// Set a value with optional TTL
    ///
    /// If TTL is None, the value persists indefinitely.
    /// If TTL is Some, the value expires after the duration.
    async fn set(&self, key: &str, value: Vec<u8>, ttl: Option<Duration>) -> Result<()>;

    /// Atomic increment operation
    ///
    /// Increments the value at key by delta.
    /// If the key doesn't exist, it's created with the delta value.
    /// Returns the new value after increment.
    async fn increment(&self, key: &str, delta: i64, ttl: Option<Duration>) -> Result<i64>;

    /// Delete a key
    ///
    /// Returns Ok(()) whether the key existed or not.
    async fn delete(&self, key: &str) -> Result<()>;

    /// Compare-and-swap operation (for distributed locks)
    ///
    /// Atomically checks if the current value equals `expected`,
    /// and if so, sets it to `new_value`.
    /// Returns true if the swap succeeded, false otherwise.
    async fn compare_and_swap(
        &self,
        key: &str,
        expected: Vec<u8>,
        new_value: Vec<u8>,
    ) -> Result<bool>;

    /// Check if a key exists
    async fn exists(&self, key: &str) -> Result<bool> {
        Ok(self.get(key).await?.is_some())
    }

    /// Set TTL on an existing key
    async fn expire(&self, key: &str, ttl: Duration) -> Result<bool>;

    /// Get multiple keys at once (batch operation)
    async fn mget(&self, keys: &[String]) -> Result<Vec<Option<Vec<u8>>>> {
        // Default implementation: sequential gets
        let mut results = Vec::with_capacity(keys.len());
        for key in keys {
            results.push(self.get(key).await?);
        }
        Ok(results)
    }

    /// Set multiple keys at once (batch operation)
    async fn mset(&self, items: Vec<(String, Vec<u8>, Option<Duration>)>) -> Result<()> {
        // Default implementation: sequential sets
        for (key, value, ttl) in items {
            self.set(&key, value, ttl).await?;
        }
        Ok(())
    }

    /// Delete multiple keys at once
    async fn mdel(&self, keys: &[String]) -> Result<()> {
        // Default implementation: sequential deletes
        for key in keys {
            self.delete(key).await?;
        }
        Ok(())
    }

    /// List keys matching a pattern (use sparingly in production)
    async fn keys(&self, pattern: &str) -> Result<Vec<String>>;

    /// Flush all keys (dangerous - use only in dev/test)
    async fn flush(&self) -> Result<()>;

    /// Health check - verify backend is reachable
    async fn health_check(&self) -> Result<()> {
        // Default implementation: set and get a test key
        let test_key = "__health_check__";
        let test_value = b"ok".to_vec();
        
        self.set(test_key, test_value.clone(), Some(Duration::from_secs(1))).await?;
        let result = self.get(test_key).await?;
        self.delete(test_key).await?;
        
        if result == Some(test_value) {
            Ok(())
        } else {
            Err(Error::Backend("Health check failed".to_string()))
        }
    }
}

