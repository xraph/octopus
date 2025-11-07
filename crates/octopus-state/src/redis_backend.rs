//! Redis state backend implementation

use crate::{Error, Result, StateBackend};
use async_trait::async_trait;
use redis::{aio::ConnectionManager, AsyncCommands, RedisError};
use std::time::Duration;
use tracing::{debug, trace};

/// Redis state backend
///
/// Distributed, persistent, production-ready backend using Redis.
/// Perfect for multi-instance deployments and high-traffic production.
#[derive(Clone)]
pub struct RedisBackend {
    client: ConnectionManager,
    prefix: Option<String>,
}

impl std::fmt::Debug for RedisBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RedisBackend")
            .field("prefix", &self.prefix)
            .finish()
    }
}

impl RedisBackend {
    /// Create a new Redis backend
    pub async fn new(url: &str) -> Result<Self> {
        let client = redis::Client::open(url).map_err(|e| Error::Connection(e.to_string()))?;

        let connection_manager = ConnectionManager::new(client)
            .await
            .map_err(|e| Error::Connection(e.to_string()))?;

        debug!(url, "Redis backend connected");

        Ok(Self {
            client: connection_manager,
            prefix: None,
        })
    }

    /// Create with key prefix for namespacing
    pub async fn with_prefix(url: &str, prefix: impl Into<String>) -> Result<Self> {
        let mut backend = Self::new(url).await?;
        backend.prefix = Some(prefix.into());
        Ok(backend)
    }

    /// Add prefix to key if configured
    fn key(&self, key: &str) -> String {
        match &self.prefix {
            Some(prefix) => format!("{}:{}", prefix, key),
            None => key.to_string(),
        }
    }

    /// Remove prefix from key if configured
    fn unprefix(&self, key: &str) -> String {
        match &self.prefix {
            Some(prefix) => {
                let prefix_with_colon = format!("{}:", prefix);
                key.strip_prefix(&prefix_with_colon)
                    .unwrap_or(key)
                    .to_string()
            }
            None => key.to_string(),
        }
    }
}

#[async_trait]
impl StateBackend for RedisBackend {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        trace!(key, "Redis GET");

        let key = self.key(key);
        let mut conn = self.client.clone();

        let result: Option<Vec<u8>> = conn.get(&key).await?;

        Ok(result)
    }

    async fn set(&self, key: &str, value: Vec<u8>, ttl: Option<Duration>) -> Result<()> {
        trace!(key, ttl_secs = ?ttl.map(|d| d.as_secs()), "Redis SET");

        let key = self.key(key);
        let mut conn = self.client.clone();

        if let Some(ttl) = ttl {
            conn.set_ex(&key, value, ttl.as_secs()).await?;
        } else {
            conn.set(&key, value).await?;
        }

        Ok(())
    }

    async fn increment(&self, key: &str, delta: i64, ttl: Option<Duration>) -> Result<i64> {
        trace!(key, delta, "Redis INCRBY");

        let key = self.key(key);
        let mut conn = self.client.clone();

        let new_value: i64 = conn.incr(&key, delta).await?;

        // Set TTL if provided and this is a new key
        if let Some(ttl) = ttl {
            conn.expire(&key, ttl.as_secs() as i64).await?;
        }

        Ok(new_value)
    }

    async fn delete(&self, key: &str) -> Result<()> {
        trace!(key, "Redis DEL");

        let key = self.key(key);
        let mut conn = self.client.clone();

        let _: () = conn.del(&key).await?;

        Ok(())
    }

    async fn compare_and_swap(
        &self,
        key: &str,
        expected: Vec<u8>,
        new_value: Vec<u8>,
    ) -> Result<bool> {
        trace!(key, "Redis CAS (using Lua script)");

        let key = self.key(key);
        let mut conn = self.client.clone();

        // Lua script for atomic CAS
        let script = redis::Script::new(
            r#"
            local current = redis.call('GET', KEYS[1])
            if current == ARGV[1] then
                redis.call('SET', KEYS[1], ARGV[2])
                return 1
            else
                return 0
            end
            "#,
        );

        let result: i32 = script
            .key(&key)
            .arg(expected)
            .arg(new_value)
            .invoke_async(&mut conn)
            .await?;

        Ok(result == 1)
    }

    async fn expire(&self, key: &str, ttl: Duration) -> Result<bool> {
        trace!(key, ttl_secs = ttl.as_secs(), "Redis EXPIRE");

        let key = self.key(key);
        let mut conn = self.client.clone();

        let result: bool = conn.expire(&key, ttl.as_secs() as i64).await?;

        Ok(result)
    }

    async fn mget(&self, keys: &[String]) -> Result<Vec<Option<Vec<u8>>>> {
        trace!(count = keys.len(), "Redis MGET");

        let prefixed_keys: Vec<String> = keys.iter().map(|k| self.key(k)).collect();
        let mut conn = self.client.clone();

        let values: Vec<Option<Vec<u8>>> = conn.get(&prefixed_keys).await?;

        Ok(values)
    }

    async fn mset(&self, items: Vec<(String, Vec<u8>, Option<Duration>)>) -> Result<()> {
        trace!(count = items.len(), "Redis MSET (pipelined)");

        let mut conn = self.client.clone();
        let mut pipe = redis::pipe();

        for (key, value, ttl) in items {
            let key = self.key(&key);
            if let Some(ttl) = ttl {
                pipe.set_ex(&key, value, ttl.as_secs());
            } else {
                pipe.set(&key, value);
            }
        }

        let _: () = pipe.query_async(&mut conn).await?;

        Ok(())
    }

    async fn mdel(&self, keys: &[String]) -> Result<()> {
        trace!(count = keys.len(), "Redis DEL (multiple)");

        let prefixed_keys: Vec<String> = keys.iter().map(|k| self.key(k)).collect();
        let mut conn = self.client.clone();

        let _: () = conn.del(&prefixed_keys).await?;

        Ok(())
    }

    async fn keys(&self, pattern: &str) -> Result<Vec<String>> {
        trace!(pattern, "Redis KEYS");

        let pattern = self.key(pattern);
        let mut conn = self.client.clone();

        let keys: Vec<String> = conn.keys(&pattern).await?;

        // Remove prefix from results
        let keys = keys.into_iter().map(|k| self.unprefix(&k)).collect();

        Ok(keys)
    }

    async fn flush(&self) -> Result<()> {
        debug!("Redis FLUSHDB");

        let mut conn = self.client.clone();

        // Use SCAN and DEL with prefix to avoid flushing the entire DB
        if let Some(ref prefix) = self.prefix {
            let pattern = format!("{}:*", prefix);
            let keys: Vec<String> = conn.keys(&pattern).await?;
            if !keys.is_empty() {
                let _: () = conn.del(&keys).await?;
            }
        } else {
            // No prefix - flush entire DB (dangerous!)
            let _: () = redis::cmd("FLUSHDB").query_async(&mut conn).await?;
        }

        Ok(())
    }

    async fn health_check(&self) -> Result<()> {
        let mut conn = self.client.clone();

        // PING command
        let response: String = redis::cmd("PING")
            .query_async(&mut conn)
            .await
            .map_err(|e| Error::Backend(e.to_string()))?;

        if response == "PONG" {
            Ok(())
        } else {
            Err(Error::Backend("Unexpected PING response".to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // These tests require a running Redis instance
    // Run with: docker run -p 6379:6379 redis:7-alpine

    async fn setup() -> Option<RedisBackend> {
        RedisBackend::with_prefix("redis://127.0.0.1:6379", "octopus_test")
            .await
            .ok()
    }

    #[tokio::test]
    async fn test_redis_get_set() {
        let Some(backend) = setup().await else {
            eprintln!("Skipping Redis tests - Redis not available");
            return;
        };

        backend
            .set("test_key", b"test_value".to_vec(), None)
            .await
            .unwrap();
        let value = backend.get("test_key").await.unwrap();

        assert_eq!(value, Some(b"test_value".to_vec()));

        backend.delete("test_key").await.unwrap();
    }

    #[tokio::test]
    async fn test_redis_ttl() {
        let Some(backend) = setup().await else {
            return;
        };

        backend
            .set("ttl_key", b"value".to_vec(), Some(Duration::from_secs(1)))
            .await
            .unwrap();

        assert!(backend.get("ttl_key").await.unwrap().is_some());

        tokio::time::sleep(Duration::from_secs(2)).await;

        assert!(backend.get("ttl_key").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_redis_increment() {
        let Some(backend) = setup().await else {
            return;
        };

        let val1 = backend.increment("counter", 1, None).await.unwrap();
        assert_eq!(val1, 1);

        let val2 = backend.increment("counter", 5, None).await.unwrap();
        assert_eq!(val2, 6);

        backend.delete("counter").await.unwrap();
    }

    #[tokio::test]
    async fn test_redis_cas() {
        let Some(backend) = setup().await else {
            return;
        };

        backend.set("cas_key", b"old".to_vec(), None).await.unwrap();

        let success = backend
            .compare_and_swap("cas_key", b"old".to_vec(), b"new".to_vec())
            .await
            .unwrap();
        assert!(success);

        let value = backend.get("cas_key").await.unwrap();
        assert_eq!(value, Some(b"new".to_vec()));

        backend.delete("cas_key").await.unwrap();
    }

    #[tokio::test]
    async fn test_redis_health_check() {
        let Some(backend) = setup().await else {
            return;
        };

        assert!(backend.health_check().await.is_ok());
    }
}
