//! # Octopus HTTP Proxy
//!
//! High-performance HTTP proxy with:
//! - Connection pooling (HTTP/1.1 and HTTP/2)
//! - Zero-copy proxying
//! - Timeout handling
//! - Retry logic
//! - Request/response transformation

#![forbid(unsafe_code)]
#![warn(
    missing_docs,
    missing_debug_implementations,
    rust_2018_idioms,
    unreachable_pub
)]

pub mod client;
pub mod pool;
pub mod proxy;

pub use client::HttpClient;
pub use pool::{ConnectionPool, PoolConfig};
pub use proxy::{HttpProxy, ProxyConfig};

/// Re-export commonly used types
pub mod prelude {
    pub use crate::client::HttpClient;
    pub use crate::pool::{ConnectionPool, PoolConfig};
    pub use crate::proxy::{HttpProxy, ProxyConfig};
}

