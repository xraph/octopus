//! # Octopus Plugin API
//!
//! This crate provides the SDK for developing plugins for Octopus API Gateway.
//!
//! ## Plugin Types
//!
//! - **Request Interceptors**: Modify requests before routing
//! - **Response Interceptors**: Modify responses before returning
//! - **Authentication Providers**: Custom auth schemes
//! - **Authorization Policies**: Custom authz logic
//! - **Transform Plugins**: Request/response transformation
//! - **Protocol Handlers**: Custom protocol support
//!
//! ## Example
//!
//! ```rust,no_run
//! use octopus_plugin_api::*;
//! use async_trait::async_trait;
//!
//! #[derive(Debug)]
//! struct MyPlugin;
//!
//! #[async_trait]
//! impl Plugin for MyPlugin {
//!     fn name(&self) -> &str { "my-plugin" }
//!     fn version(&self) -> &str { "1.0.0" }
//!    
//!     async fn init(&mut self, config: serde_json::Value) -> Result<(), PluginError> {
//!         Ok(())
//!     }
//!    
//!     async fn start(&mut self) -> Result<(), PluginError> {
//!         Ok(())
//!     }
//!    
//!     async fn stop(&mut self) -> Result<(), PluginError> {
//!         Ok(())
//!     }
//! }
//! ```

#![forbid(unsafe_code)]
#![warn(
    missing_docs,
    missing_debug_implementations,
    rust_2018_idioms,
    unreachable_pub
)]

pub mod auth;
pub mod context;
pub mod error;
pub mod interceptor;
pub mod plugin;
pub mod protocol;
pub mod script;
pub mod transform;

#[cfg(feature = "testing")]
pub mod testing;

// Re-export commonly used types
pub use auth::{AuthProvider, AuthResult, Credentials, Principal};
pub use context::{RequestContext, ResponseContext};
pub use error::PluginError;
pub use interceptor::{InterceptorAction, RequestInterceptor, ResponseInterceptor};
pub use plugin::{HealthStatus, Plugin, PluginDependency, PluginInfo, PluginMetadata, PluginState};
pub use protocol::ProtocolHandler;
pub use script::{ScriptCacheStats, ScriptConfig, ScriptInterceptorPlugin, ScriptLanguage};
pub use transform::{BodyTransform, TransformConfig, TransformPlugin};

/// Prelude module with commonly used types
pub mod prelude {
    pub use crate::auth::{AuthProvider, AuthResult, Credentials, Principal};
    pub use crate::context::{RequestContext, ResponseContext};
    pub use crate::error::PluginError;
    pub use crate::interceptor::{InterceptorAction, RequestInterceptor, ResponseInterceptor};
    pub use crate::plugin::{HealthStatus, Plugin, PluginDependency};
    pub use crate::protocol::ProtocolHandler;
    pub use crate::script::{
        ScriptCacheStats, ScriptConfig, ScriptInterceptorPlugin, ScriptLanguage,
    };
    pub use crate::transform::{TransformConfig, TransformPlugin};
    pub use async_trait::async_trait;
}
