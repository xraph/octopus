//! Hybrid backend (local cache + Redis)

use crate::{InMemoryBackend, RedisBackend, Result, StateBackend};
use async_trait::async_trait;
use std::time::Duration;
use tracing::{debug, trace};

/// Hybrid backend combining local cache with Redis
///
/// Provides fast reads from local cache with distributed consistency via Redis.
/// Perfect for high-traffic production where latency matters.
///
/// ## Strategy
/// - **Reads**: Local cache first (μs), Redis on miss (1-2ms)
/// - **Writes**: Write-through to Redis, update local cache
/// - **Invalidation**: TTL-based expiration in local cache
#[derive(Clone)]
pub struct HybridBackend {
    local: InMemoryBackend,
    remote: RedisBackend,
    cache_ttl: Duration,
}

impl std::fmt::Debug for HybridBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HybridBackend")
            .field("cache_ttl", &self.cache_ttl)
            .finish()
    }
}

impl HybridBackend {
    /// Create a new hybrid backend
    pub async fn new(redis_url: &str, cache_ttl: Duration) -> Result<Self> {
        let remote = RedisBackend::new(redis_url).await?;
        let local = InMemoryBackend::with_cleanup(cache_ttl);
        
        debug!(cache_ttl_secs = cache_ttl.as_secs(), "Hybrid backend initialized");
        
        Ok(Self {
            local,
            remote,
            cache_ttl,
        })
    }

    /// Create with key prefix for Redis namespacing
    pub async fn with_prefix(
        redis_url: &str,
        prefix: impl Into<String>,
        cache_ttl: Duration,
    ) -> Result<Self> {
        let remote = RedisBackend::with_prefix(redis_url, prefix).await?;
        let local = InMemoryBackend::with_cleanup(cache_ttl);
        
        Ok(Self {
            local,
            remote,
            cache_ttl,
        })
    }

    /// Invalidate local cache for a key
    pub async fn invalidate_cache(&self, key: &str) -> Result<()> {
        self.local.delete(key).await
    }

    /// Get cache statistics
    pub fn cache_size(&self) -> usize {
        self.local.len()
    }
}

#[async_trait]
impl StateBackend for HybridBackend {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        trace!(key, "Hybrid GET");
        
        // Try local cache first (hot path)
        if let Some(cached) = self.local.get(key).await? {
            trace!(key, "Cache HIT");
            return Ok(Some(cached));
        }
        
        trace!(key, "Cache MISS - fetching from Redis");
        
        // Cache miss - fetch from Redis
        if let Some(value) = self.remote.get(key).await? {
            // Populate local cache with shorter TTL
            let cache_ttl = Some(self.cache_ttl);
            self.local.set(key, value.clone(), cache_ttl).await?;
            return Ok(Some(value));
        }
        
        Ok(None)
    }

    async fn set(&self, key: &str, value: Vec<u8>, ttl: Option<Duration>) -> Result<()> {
        trace!(key, ttl_secs = ?ttl.map(|d| d.as_secs()), "Hybrid SET");
        
        // Write-through: write to Redis first
        self.remote.set(key, value.clone(), ttl).await?;
        
        // Update local cache with shorter TTL
        let cache_ttl = Some(ttl.unwrap_or(self.cache_ttl).min(self.cache_ttl));
        self.local.set(key, value, cache_ttl).await?;
        
        Ok(())
    }

    async fn increment(&self, key: &str, delta: i64, ttl: Option<Duration>) -> Result<i64> {
        trace!(key, delta, "Hybrid INCREMENT");
        
        // Increment in Redis (source of truth)
        let new_value = self.remote.increment(key, delta, ttl).await?;
        
        // Invalidate local cache (stale after increment)
        self.local.delete(key).await?;
        
        Ok(new_value)
    }

    async fn delete(&self, key: &str) -> Result<()> {
        trace!(key, "Hybrid DELETE");
        
        // Delete from both
        self.remote.delete(key).await?;
        self.local.delete(key).await?;
        
        Ok(())
    }

    async fn compare_and_swap(
        &self,
        key: &str,
        expected: Vec<u8>,
        new_value: Vec<u8>,
    ) -> Result<bool> {
        trace!(key, "Hybrid CAS");
        
        // CAS must go to Redis (atomic operation)
        let success = self.remote.compare_and_swap(key, expected, new_value.clone()).await?;
        
        if success {
            // Update local cache on successful CAS
            let cache_ttl = Some(self.cache_ttl);
            self.local.set(key, new_value, cache_ttl).await?;
        }
        
        Ok(success)
    }

    async fn expire(&self, key: &str, ttl: Duration) -> Result<bool> {
        trace!(key, ttl_secs = ttl.as_secs(), "Hybrid EXPIRE");
        
        // Update TTL in Redis
        let success = self.remote.expire(key, ttl).await?;
        
        if success {
            // Update local cache TTL
            let cache_ttl = Some(ttl.min(self.cache_ttl));
            if let Some(value) = self.local.get(key).await? {
                self.local.set(key, value, cache_ttl).await?;
            }
        }
        
        Ok(success)
    }

    async fn mget(&self, keys: &[String]) -> Result<Vec<Option<Vec<u8>>>> {
        trace!(count = keys.len(), "Hybrid MGET");
        
        let mut results = Vec::with_capacity(keys.len());
        let mut missing_keys = Vec::new();
        let mut missing_indices = Vec::new();
        
        // Check local cache first
        for (i, key) in keys.iter().enumerate() {
            if let Some(cached) = self.local.get(key).await? {
                results.push(Some(cached));
            } else {
                results.push(None);
                missing_keys.push(key.clone());
                missing_indices.push(i);
            }
        }
        
        // Fetch missing from Redis
        if !missing_keys.is_empty() {
            let remote_values = self.remote.mget(&missing_keys).await?;
            
            for (idx, value) in missing_indices.iter().zip(remote_values.iter()) {
                if let Some(val) = value {
                    // Update local cache
                    let cache_ttl = Some(self.cache_ttl);
                    self.local.set(&keys[*idx], val.clone(), cache_ttl).await?;
                    results[*idx] = Some(val.clone());
                }
            }
        }
        
        Ok(results)
    }

    async fn mset(&self, items: Vec<(String, Vec<u8>, Option<Duration>)>) -> Result<()> {
        trace!(count = items.len(), "Hybrid MSET");
        
        // Write to Redis
        self.remote.mset(items.clone()).await?;
        
        // Update local cache
        for (key, value, ttl) in items {
            let cache_ttl = Some(ttl.unwrap_or(self.cache_ttl).min(self.cache_ttl));
            self.local.set(&key, value, cache_ttl).await?;
        }
        
        Ok(())
    }

    async fn mdel(&self, keys: &[String]) -> Result<()> {
        trace!(count = keys.len(), "Hybrid MDEL");
        
        // Delete from both
        self.remote.mdel(keys).await?;
        self.local.mdel(keys).await?;
        
        Ok(())
    }

    async fn keys(&self, pattern: &str) -> Result<Vec<String>> {
        trace!(pattern, "Hybrid KEYS (Redis only)");
        
        // Keys operation only queries Redis (source of truth)
        self.remote.keys(pattern).await
    }

    async fn flush(&self) -> Result<()> {
        debug!("Hybrid FLUSH");
        
        self.remote.flush().await?;
        self.local.flush().await?;
        
        Ok(())
    }

    async fn health_check(&self) -> Result<()> {
        // Check both backends
        self.local.health_check().await?;
        self.remote.health_check().await?;
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn setup() -> Option<HybridBackend> {
        HybridBackend::with_prefix(
            "redis://127.0.0.1:6379",
            "octopus_hybrid_test",
            Duration::from_secs(5),
        )
        .await
        .ok()
    }

    #[tokio::test]
    async fn test_hybrid_cache_hit() {
        let Some(backend) = setup().await else {
            eprintln!("Skipping Hybrid tests - Redis not available");
            return;
        };
        
        backend.set("test_key", b"test_value".to_vec(), None).await.unwrap();
        
        // First get - cache miss
        let value1 = backend.get("test_key").await.unwrap();
        assert_eq!(value1, Some(b"test_value".to_vec()));
        
        // Second get - cache hit (faster)
        let value2 = backend.get("test_key").await.unwrap();
        assert_eq!(value2, Some(b"test_value".to_vec()));
        
        assert_eq!(backend.cache_size(), 1);
        
        backend.delete("test_key").await.unwrap();
    }

    #[tokio::test]
    async fn test_hybrid_increment_invalidates_cache() {
        let Some(backend) = setup().await else { return; };
        
        backend.set("counter", b"5".to_vec(), None).await.unwrap();
        
        // Get to populate cache
        backend.get("counter").await.unwrap();
        assert_eq!(backend.cache_size(), 1);
        
        // Increment should invalidate cache
        let new_val = backend.increment("counter", 1, None).await.unwrap();
        assert_eq!(new_val, 6);
        
        // Cache should be invalidated
        assert_eq!(backend.cache_size(), 0);
        
        backend.delete("counter").await.unwrap();
    }

    #[tokio::test]
    async fn test_hybrid_health_check() {
        let Some(backend) = setup().await else { return; };
        
        assert!(backend.health_check().await.is_ok());
    }
}

