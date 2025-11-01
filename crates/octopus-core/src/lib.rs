//! # Octopus Core
//!
//! Core types, traits, and error handling for the Octopus API Gateway.
//!
//! This crate provides the foundational abstractions used throughout the gateway:
//! - Request/response types
//! - Middleware trait
//! - Error types
//! - Common utilities

#![forbid(unsafe_code)]
#![warn(
    missing_docs,
    missing_debug_implementations,
    rust_2018_idioms,
    unreachable_pub
)]

pub mod error;
pub mod middleware;
pub mod request;
pub mod response;
pub mod types;
pub mod upstream;

pub use error::{Error, Result};
pub use middleware::{Middleware, Next};
pub use request::RequestContext;
pub use response::ResponseBuilder;
pub use types::*;
pub use upstream::{UpstreamCluster, UpstreamInstance};

// Re-export commonly used HTTP types
pub use http::{Request, Response, StatusCode, Method};
pub use bytes::Bytes;

/// Re-export commonly used types
pub mod prelude {
    pub use crate::error::{Error, Result};
    pub use crate::middleware::{Middleware, Next};
    pub use crate::request::RequestContext;
    pub use crate::response::ResponseBuilder;
    pub use crate::types::*;
    pub use crate::upstream::{UpstreamCluster, UpstreamInstance};
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_core_types() {
        // Basic compilation test
        assert_eq!(2 + 2, 4);
    }
}


