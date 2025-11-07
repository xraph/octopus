//! # Octopus Runtime
//!
//! Application runtime and lifecycle management with:
//! - Server lifecycle (startup, running, shutdown)
//! - Graceful shutdown with signal handling
//! - Worker thread management
//! - Hot reload support
//! - Health monitoring

#![forbid(unsafe_code)]
#![warn(
    missing_docs,
    missing_debug_implementations,
    rust_2018_idioms,
    unreachable_pub
)]

pub mod admin;
pub mod handler;
pub mod server;
pub mod shutdown;
pub mod worker;

pub use admin::AdminHandler;
pub use handler::RequestHandler;
pub use server::{Server, ServerBuilder};
pub use shutdown::{ShutdownSignal, SignalHandler};

/// Runtime state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeState {
    /// Server is initializing
    Initializing,
    /// Server is running
    Running,
    /// Server is shutting down
    ShuttingDown,
    /// Server is stopped
    Stopped,
}

/// Re-export commonly used types
pub mod prelude {
    pub use crate::server::{Server, ServerBuilder};
    pub use crate::shutdown::{ShutdownSignal, SignalHandler};
    pub use crate::RuntimeState;
}
