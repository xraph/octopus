//! # Octopus State Management
//!
//! Pluggable state backends for stateful features in a stateless gateway:
//! - Rate limiting counters
//! - Session storage
//! - Circuit breaker state
//! - Distributed locks
//!
//! ## Backends
//!
//! - **InMemory**: Fast, zero dependencies, single-instance only (default)
//! - **Redis**: Distributed, persistent, production-ready
//! - **PostgreSQL**: ACID guarantees, compliance-heavy use cases
//! - **Hybrid**: Local cache + Redis for optimal performance
//!
//! ## Example
//!
//! ```rust
//! use octopus_state::{StateBackend, InMemoryBackend};
//! use std::time::Duration;
//!
//! #[tokio::main]
//! async fn main() -> octopus_state::Result<()> {
//!     // Use in-memory backend (default)
//!     let backend = InMemoryBackend::new();
//!     
//!     // Store session data
//!     backend.set("session:123", b"user_data".to_vec(), Some(Duration::from_secs(3600))).await?;
//!     
//!     // Retrieve session
//!     let data = backend.get("session:123").await?;
//!     
//!     Ok(())
//! }
//! ```

#![forbid(unsafe_code)]
#![warn(
    missing_docs,
    missing_debug_implementations,
    rust_2018_idioms,
    unreachable_pub
)]

mod backend;
mod config;
mod error;
mod inmemory;

#[cfg(feature = "redis-backend")]
mod redis_backend;

#[cfg(feature = "postgres-backend")]
mod postgres_backend;

#[cfg(feature = "hybrid")]
mod hybrid;

pub use backend::StateBackend;
pub use config::{BackendConfig, StateConfig};
pub use error::{Error, Result};
pub use inmemory::InMemoryBackend;

#[cfg(feature = "redis-backend")]
pub use redis_backend::RedisBackend;

#[cfg(feature = "postgres-backend")]
pub use postgres_backend::PostgresBackend;

#[cfg(feature = "hybrid")]
pub use hybrid::HybridBackend;

/// Re-export commonly used types
pub mod prelude {
    pub use crate::backend::StateBackend;
    pub use crate::config::{BackendConfig, StateConfig};
    pub use crate::error::{Error, Result};
    pub use crate::inmemory::InMemoryBackend;
    
    #[cfg(feature = "redis-backend")]
    pub use crate::redis_backend::RedisBackend;
    
    #[cfg(feature = "postgres-backend")]
    pub use crate::postgres_backend::PostgresBackend;
    
    #[cfg(feature = "hybrid")]
    pub use crate::hybrid::HybridBackend;
}

