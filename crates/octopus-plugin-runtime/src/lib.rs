//! # Octopus Plugin Runtime
//!
//! Runtime and lifecycle management for Octopus plugins.
//!
//! ## Features
//!
//! - **Plugin Registry**: Central registration and discovery
//! - **Lifecycle Management**: Init, start, stop, reload
//! - **Hot Reload**: Update plugins without gateway restart
//! - **Health Monitoring**: Track plugin health status
//!
//! ## Example
//!
//! ```rust,no_run
//! use octopus_plugin_runtime::*;
//! use std::sync::Arc;
//!
//! # async fn example() -> Result<()> {
//! // Create plugin registry
//! let registry = Arc::new(PluginRegistry::new());
//!
//! // Register a plugin (plugin must implement Plugin trait)
//! // registry.register("my-plugin", Box::new(MyPlugin::new())).await?;
//!
//! // Start all plugins
//! registry.start_all().await?;
//!
//! // Stop all plugins
//! registry.stop_all().await?;
//! # Ok(())
//! # }
//! ```

#![forbid(unsafe_code)]
#![warn(
    missing_docs,
    missing_debug_implementations,
    rust_2018_idioms,
    unreachable_pub
)]

pub mod error;
pub mod hot_reload;
pub mod manager;
pub mod registry;

pub use error::{PluginRuntimeError, Result};
pub use hot_reload::{HotReloadWatcher, ReloadEvent};
pub use manager::{PluginManager, PluginStats};
pub use registry::{PluginEntry, PluginRegistry, PluginState as RegistryPluginState};

// Re-export plugin API types for convenience
pub use octopus_plugin_api::{
    auth, context, interceptor, plugin, protocol, transform, HealthStatus, Plugin,
    PluginDependency, PluginError, PluginInfo, PluginState,
};

/// Prelude module with commonly used types
pub mod prelude {
    pub use crate::error::{PluginRuntimeError, Result};
    pub use crate::manager::PluginManager;
    pub use crate::registry::{PluginEntry, PluginRegistry};
    pub use octopus_plugin_api::prelude::*;
}
