//! Configuration for state backends

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// State management configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateConfig {
    /// Backend type
    #[serde(default)]
    pub backend: BackendConfig,
    
    /// Cleanup interval for expired keys (in-memory backend)
    #[serde(default = "default_cleanup_interval")]
    pub cleanup_interval: Duration,
}

impl Default for StateConfig {
    fn default() -> Self {
        Self {
            backend: BackendConfig::InMemory,
            cleanup_interval: default_cleanup_interval(),
        }
    }
}

/// Backend configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum BackendConfig {
    /// In-memory backend (default, single-instance only)
    InMemory,
    
    /// Redis backend (distributed, production-ready)
    #[cfg(feature = "redis-backend")]
    Redis {
        /// Redis connection URL (redis://host:port or rediss:// for TLS)
        url: String,
        
        /// Connection pool size
        #[serde(default = "default_pool_size")]
        pool_size: u32,
        
        /// Connection timeout
        #[serde(default = "default_timeout")]
        timeout: Duration,
        
        /// Key prefix for namespacing
        #[serde(default)]
        prefix: Option<String>,
    },
    
    /// PostgreSQL backend (ACID, compliance-heavy)
    #[cfg(feature = "postgres-backend")]
    Postgres {
        /// PostgreSQL connection URL
        url: String,
        
        /// Connection pool size
        #[serde(default = "default_pool_size")]
        pool_size: u32,
        
        /// Connection timeout
        #[serde(default = "default_timeout")]
        timeout: Duration,
        
        /// Table name for state storage
        #[serde(default = "default_table_name")]
        table_name: String,
    },
    
    /// Hybrid backend (local cache + Redis)
    #[cfg(feature = "hybrid")]
    Hybrid {
        /// Redis configuration
        redis_url: String,
        
        /// Local cache TTL
        #[serde(default = "default_cache_ttl")]
        cache_ttl: Duration,
        
        /// Connection pool size
        #[serde(default = "default_pool_size")]
        pool_size: u32,
        
        /// Key prefix for namespacing
        #[serde(default)]
        prefix: Option<String>,
    },
}

impl Default for BackendConfig {
    fn default() -> Self {
        Self::InMemory
    }
}

fn default_cleanup_interval() -> Duration {
    Duration::from_secs(60)
}

#[allow(dead_code)]
fn default_pool_size() -> u32 {
    10
}

#[allow(dead_code)]
fn default_timeout() -> Duration {
    Duration::from_secs(5)
}

#[allow(dead_code)]
fn default_cache_ttl() -> Duration {
    Duration::from_secs(60)
}

#[allow(dead_code)]
fn default_table_name() -> String {
    "octopus_state".to_string()
}

