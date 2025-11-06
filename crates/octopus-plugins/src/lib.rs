//! # Octopus Plugin System
//!
//! Plugin system with support for:
//! - Static plugins (compiled into binary)
//! - Dynamic plugins (loaded at runtime from .so/.dylib/.dll)
//! - Plugin lifecycle hooks (init, start, stop, shutdown)
//! - Plugin configuration
//! - Plugin dependencies
//! - Safe plugin isolation

#![forbid(unsafe_code)]
#![warn(
    missing_docs,
    missing_debug_implementations,
    rust_2018_idioms,
    unreachable_pub
)]

pub mod registry;
pub mod traits;
pub mod loader;
pub mod manager;

pub use registry::PluginRegistry;
pub use traits::{Plugin, PluginMetadata, PluginType};
pub use loader::PluginLoader;
pub use manager::PluginManager;


/// Re-export commonly used types
pub mod prelude {
    pub use crate::registry::PluginRegistry;
    pub use crate::traits::{Plugin, PluginMetadata, PluginType};
    pub use crate::loader::PluginLoader;
    pub use crate::manager::PluginManager;
}

