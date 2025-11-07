//! In-memory state backend implementation

use crate::{Error, Result, StateBackend};
use async_trait::async_trait;
use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::interval;
use tracing::{debug, trace};

/// Entry in the in-memory store
#[derive(Debug, Clone)]
struct Entry {
    value: Vec<u8>,
    expires_at: Option<Instant>,
}

impl Entry {
    fn new(value: Vec<u8>, ttl: Option<Duration>) -> Self {
        Self {
            value,
            expires_at: ttl.map(|d| Instant::now() + d),
        }
    }

    fn is_expired(&self) -> bool {
        self.expires_at
            .map(|exp| Instant::now() > exp)
            .unwrap_or(false)
    }
}

/// In-memory state backend
///
/// Fast, zero dependencies, but single-instance only.
/// Perfect for development, testing, and single-node deployments.
#[derive(Debug, Clone)]
pub struct InMemoryBackend {
    store: Arc<DashMap<String, Entry>>,
}

impl InMemoryBackend {
    /// Create a new in-memory backend
    pub fn new() -> Self {
        Self {
            store: Arc::new(DashMap::new()),
        }
    }

    /// Create with background cleanup task
    ///
    /// Spawns a tokio task that periodically removes expired entries.
    pub fn with_cleanup(cleanup_interval: Duration) -> Self {
        let backend = Self::new();
        let store = backend.store.clone();

        tokio::spawn(async move {
            let mut ticker = interval(cleanup_interval);
            loop {
                ticker.tick().await;
                Self::cleanup_expired(&store);
            }
        });

        backend
    }

    /// Manually trigger cleanup of expired entries
    pub fn cleanup(&self) {
        Self::cleanup_expired(&self.store);
    }

    /// Internal cleanup implementation
    fn cleanup_expired(store: &DashMap<String, Entry>) {
        let mut removed = 0;
        store.retain(|_, entry| {
            if entry.is_expired() {
                removed += 1;
                false
            } else {
                true
            }
        });

        if removed > 0 {
            debug!(removed, "Cleaned up expired entries");
        }
    }

    /// Get the number of entries in the store
    pub fn len(&self) -> usize {
        self.store.len()
    }

    /// Check if the store is empty
    pub fn is_empty(&self) -> bool {
        self.store.is_empty()
    }
}

impl Default for InMemoryBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl StateBackend for InMemoryBackend {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        trace!(key, "InMemory GET");

        if let Some(entry) = self.store.get(key) {
            if entry.is_expired() {
                drop(entry); // Release read lock
                self.store.remove(key);
                return Ok(None);
            }
            return Ok(Some(entry.value.clone()));
        }

        Ok(None)
    }

    async fn set(&self, key: &str, value: Vec<u8>, ttl: Option<Duration>) -> Result<()> {
        trace!(key, ttl_secs = ?ttl.map(|d| d.as_secs()), "InMemory SET");

        let entry = Entry::new(value, ttl);
        self.store.insert(key.to_string(), entry);

        Ok(())
    }

    async fn increment(&self, key: &str, delta: i64, ttl: Option<Duration>) -> Result<i64> {
        trace!(key, delta, "InMemory INCREMENT");

        let mut new_value = delta;

        self.store
            .entry(key.to_string())
            .and_modify(|entry| {
                if !entry.is_expired() {
                    // Parse existing value and increment
                    if let Ok(current) = std::str::from_utf8(&entry.value) {
                        if let Ok(current_num) = current.parse::<i64>() {
                            new_value = current_num + delta;
                            entry.value = new_value.to_string().into_bytes();

                            // Update TTL if provided
                            if let Some(ttl) = ttl {
                                entry.expires_at = Some(Instant::now() + ttl);
                            }
                            return;
                        }
                    }
                }

                // If expired or invalid, set to delta
                entry.value = delta.to_string().into_bytes();
                entry.expires_at = ttl.map(|d| Instant::now() + d);
            })
            .or_insert_with(|| Entry::new(delta.to_string().into_bytes(), ttl));

        Ok(new_value)
    }

    async fn delete(&self, key: &str) -> Result<()> {
        trace!(key, "InMemory DELETE");
        self.store.remove(key);
        Ok(())
    }

    async fn compare_and_swap(
        &self,
        key: &str,
        expected: Vec<u8>,
        new_value: Vec<u8>,
    ) -> Result<bool> {
        trace!(key, "InMemory CAS");

        if let Some(mut entry) = self.store.get_mut(key) {
            if entry.is_expired() {
                return Ok(false);
            }

            if entry.value == expected {
                entry.value = new_value;
                return Ok(true);
            }
            return Ok(false);
        }

        Ok(false)
    }

    async fn expire(&self, key: &str, ttl: Duration) -> Result<bool> {
        trace!(key, ttl_secs = ttl.as_secs(), "InMemory EXPIRE");

        if let Some(mut entry) = self.store.get_mut(key) {
            if !entry.is_expired() {
                entry.expires_at = Some(Instant::now() + ttl);
                return Ok(true);
            }
        }

        Ok(false)
    }

    async fn keys(&self, pattern: &str) -> Result<Vec<String>> {
        trace!(pattern, "InMemory KEYS");

        // Simple glob pattern matching (* and ?)
        let regex_pattern = pattern.replace("*", ".*").replace("?", ".");

        let re = regex::Regex::new(&format!("^{}$", regex_pattern))
            .map_err(|e| Error::InvalidConfig(e.to_string()))?;

        let keys: Vec<String> = self
            .store
            .iter()
            .filter(|entry| !entry.value().is_expired() && re.is_match(entry.key()))
            .map(|entry| entry.key().clone())
            .collect();

        Ok(keys)
    }

    async fn flush(&self) -> Result<()> {
        debug!("InMemory FLUSH - clearing all keys");
        self.store.clear();
        Ok(())
    }

    async fn mget(&self, keys: &[String]) -> Result<Vec<Option<Vec<u8>>>> {
        trace!(count = keys.len(), "InMemory MGET");

        let mut results = Vec::with_capacity(keys.len());
        for key in keys {
            results.push(self.get(key).await?);
        }
        Ok(results)
    }

    async fn mset(&self, items: Vec<(String, Vec<u8>, Option<Duration>)>) -> Result<()> {
        trace!(count = items.len(), "InMemory MSET");

        for (key, value, ttl) in items {
            self.set(&key, value, ttl).await?;
        }
        Ok(())
    }

    async fn mdel(&self, keys: &[String]) -> Result<()> {
        trace!(count = keys.len(), "InMemory MDEL");

        for key in keys {
            self.store.remove(key);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::sleep;

    #[tokio::test]
    async fn test_get_set() {
        let backend = InMemoryBackend::new();

        backend.set("key1", b"value1".to_vec(), None).await.unwrap();
        let value = backend.get("key1").await.unwrap();

        assert_eq!(value, Some(b"value1".to_vec()));
    }

    #[tokio::test]
    async fn test_ttl_expiration() {
        let backend = InMemoryBackend::new();

        backend
            .set("key1", b"value1".to_vec(), Some(Duration::from_millis(50)))
            .await
            .unwrap();

        // Value should exist immediately
        assert!(backend.get("key1").await.unwrap().is_some());

        // Wait for expiration
        sleep(Duration::from_millis(100)).await;

        // Value should be expired
        assert!(backend.get("key1").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_increment() {
        let backend = InMemoryBackend::new();

        let val1 = backend.increment("counter", 1, None).await.unwrap();
        assert_eq!(val1, 1);

        let val2 = backend.increment("counter", 5, None).await.unwrap();
        assert_eq!(val2, 6);

        let val3 = backend.increment("counter", -2, None).await.unwrap();
        assert_eq!(val3, 4);
    }

    #[tokio::test]
    async fn test_delete() {
        let backend = InMemoryBackend::new();

        backend.set("key1", b"value1".to_vec(), None).await.unwrap();
        assert!(backend.get("key1").await.unwrap().is_some());

        backend.delete("key1").await.unwrap();
        assert!(backend.get("key1").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_compare_and_swap() {
        let backend = InMemoryBackend::new();

        backend.set("key1", b"old".to_vec(), None).await.unwrap();

        // Successful CAS
        let success = backend
            .compare_and_swap("key1", b"old".to_vec(), b"new".to_vec())
            .await
            .unwrap();
        assert!(success);

        let value = backend.get("key1").await.unwrap();
        assert_eq!(value, Some(b"new".to_vec()));

        // Failed CAS (wrong expected value)
        let failed = backend
            .compare_and_swap("key1", b"old".to_vec(), b"newer".to_vec())
            .await
            .unwrap();
        assert!(!failed);
    }

    #[tokio::test]
    async fn test_expire() {
        let backend = InMemoryBackend::new();

        backend.set("key1", b"value1".to_vec(), None).await.unwrap();

        let success = backend
            .expire("key1", Duration::from_millis(50))
            .await
            .unwrap();
        assert!(success);

        // Should still exist
        assert!(backend.get("key1").await.unwrap().is_some());

        // Wait for expiration
        sleep(Duration::from_millis(100)).await;

        // Should be expired
        assert!(backend.get("key1").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_keys_pattern() {
        let backend = InMemoryBackend::new();

        backend
            .set("user:1", b"alice".to_vec(), None)
            .await
            .unwrap();
        backend.set("user:2", b"bob".to_vec(), None).await.unwrap();
        backend
            .set("session:1", b"data".to_vec(), None)
            .await
            .unwrap();

        let keys = backend.keys("user:*").await.unwrap();
        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&"user:1".to_string()));
        assert!(keys.contains(&"user:2".to_string()));
    }

    #[tokio::test]
    async fn test_batch_operations() {
        let backend = InMemoryBackend::new();

        // MSET
        backend
            .mset(vec![
                ("key1".to_string(), b"val1".to_vec(), None),
                ("key2".to_string(), b"val2".to_vec(), None),
            ])
            .await
            .unwrap();

        // MGET
        let values = backend
            .mget(&["key1".to_string(), "key2".to_string()])
            .await
            .unwrap();
        assert_eq!(values[0], Some(b"val1".to_vec()));
        assert_eq!(values[1], Some(b"val2".to_vec()));

        // MDEL
        backend.mdel(&["key1".to_string()]).await.unwrap();
        assert!(backend.get("key1").await.unwrap().is_none());
        assert!(backend.get("key2").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn test_cleanup() {
        let backend = InMemoryBackend::new();

        backend
            .set("key1", b"val1".to_vec(), Some(Duration::from_millis(50)))
            .await
            .unwrap();
        backend.set("key2", b"val2".to_vec(), None).await.unwrap();

        assert_eq!(backend.len(), 2);

        sleep(Duration::from_millis(100)).await;
        backend.cleanup();

        // Only key2 should remain
        assert_eq!(backend.len(), 1);
        assert!(backend.get("key2").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn test_health_check() {
        let backend = InMemoryBackend::new();
        assert!(backend.health_check().await.is_ok());
    }
}
