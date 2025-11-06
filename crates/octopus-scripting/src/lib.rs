//! # Octopus Scripting Engine
//!
//! Multi-language scripting support for request/response transformation.
//!
//! ## Supported Languages
//!
//! - **Rhai** - Fast, Rust-native scripting (5-50μs execution)
//! - **Lua** - Coming soon
//! - **JavaScript (Deno)** - Coming soon
//! - **WebAssembly** - Coming soon
//!
//! ## Features
//!
//! - Config-based inline scripts
//! - File-based scripts with hot reload
//! - AST caching for performance
//! - Request/response interception
//! - Async execution
//! - Sandboxed environment

#![forbid(unsafe_code)]
#![warn(
    missing_docs,
    missing_debug_implementations,
    rust_2018_idioms,
    unreachable_pub
)]

pub mod context;
pub mod engine;
pub mod error;
pub mod middleware;
pub mod rhai_engine;

pub use context::{RequestContext, ResponseContext, ScriptContext};
pub use engine::{ScriptEngine, ScriptLanguage, ScriptSource};
pub use error::{Result, ScriptError};
pub use middleware::{ScriptMiddleware, ScriptMiddlewareConfig};
pub use rhai_engine::RhaiEngine;

/// Prelude with commonly used types
pub mod prelude {
    pub use crate::context::{RequestContext, ResponseContext, ScriptContext};
    pub use crate::engine::{ScriptEngine, ScriptLanguage, ScriptSource};
    pub use crate::error::{Result, ScriptError};
    pub use crate::middleware::{ScriptMiddleware, ScriptMiddlewareConfig};
}
