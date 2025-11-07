//! Connection pool for managing upstream connections

use dashmap::DashMap;
use octopus_core::{Result, UpstreamInstance};
use std::sync::Arc;
use std::time::Duration;

/// Connection pool configuration
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// Maximum idle connections per upstream
    pub max_idle_per_upstream: usize,

    /// Maximum connections per upstream
    pub max_per_upstream: usize,

    /// Idle connection timeout
    pub idle_timeout: Duration,

    /// Connection timeout
    pub connect_timeout: Duration,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_idle_per_upstream: 32,
            max_per_upstream: 128,
            idle_timeout: Duration::from_secs(90),
            connect_timeout: Duration::from_secs(5),
        }
    }
}

/// Connection pool for managing upstream connections
#[derive(Debug)]
pub struct ConnectionPool {
    config: PoolConfig,
    // Connection pool state per upstream
    pools: Arc<DashMap<String, UpstreamPool>>,
}

#[derive(Debug)]
struct UpstreamPool {
    instance: UpstreamInstance,
    #[allow(dead_code)]
    active_connections: usize,
}

impl ConnectionPool {
    /// Create a new connection pool
    pub fn new(config: PoolConfig) -> Self {
        Self {
            config,
            pools: Arc::new(DashMap::new()),
        }
    }

    /// Register an upstream instance
    pub fn register(&self, instance: UpstreamInstance) {
        let id = instance.id.clone();
        self.pools.insert(
            id,
            UpstreamPool {
                instance,
                active_connections: 0,
            },
        );
    }

    /// Get an upstream instance (for connection)
    pub fn get(&self, upstream_id: &str) -> Result<UpstreamInstance> {
        self.pools
            .get(upstream_id)
            .map(|pool| {
                // Increment active connections
                // Note: In a real implementation, this would need proper connection tracking
                pool.instance.clone()
            })
            .ok_or_else(|| {
                octopus_core::Error::UpstreamConnection(format!(
                    "Upstream not found: {}",
                    upstream_id
                ))
            })
    }

    /// Remove an upstream instance
    pub fn remove(&self, upstream_id: &str) -> bool {
        self.pools.remove(upstream_id).is_some()
    }

    /// Get pool size
    pub fn pool_count(&self) -> usize {
        self.pools.len()
    }

    /// Get configuration
    pub fn config(&self) -> &PoolConfig {
        &self.config
    }
}

impl Default for ConnectionPool {
    fn default() -> Self {
        Self::new(PoolConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_pool() {
        let pool = ConnectionPool::default();

        let instance = UpstreamInstance::new("test-1", "localhost", 8080);
        pool.register(instance.clone());

        assert_eq!(pool.pool_count(), 1);

        let retrieved = pool.get("test-1").unwrap();
        assert_eq!(retrieved.id, "test-1");

        assert!(pool.remove("test-1"));
        assert_eq!(pool.pool_count(), 0);
    }

    #[test]
    fn test_pool_config() {
        let config = PoolConfig::default();
        assert_eq!(config.max_idle_per_upstream, 32);
        assert_eq!(config.max_per_upstream, 128);
    }
}
