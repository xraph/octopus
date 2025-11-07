//! Plugin runtime error types

use octopus_plugin_api::PluginError;
use std::fmt;

/// Plugin runtime error type
#[derive(Debug, thiserror::Error)]
pub enum PluginRuntimeError {
    /// Plugin error
    #[error("Plugin error: {0}")]
    PluginError(#[from] PluginError),

    /// Plugin not found
    #[error("Plugin not found: {0}")]
    PluginNotFound(String),

    /// Plugin already exists
    #[error("Plugin already exists: {0}")]
    PluginAlreadyExists(String),

    /// Dependency missing
    #[error("Dependency missing: {0}")]
    DependencyMissing(String),

    /// Dependency cycle detected
    #[error("Dependency cycle detected: {0}")]
    DependencyCycle(String),

    /// Invalid state
    #[error("Invalid state: {0}")]
    InvalidState(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// I/O error
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// Serialization error
    #[error("Serialization error: {0}")]
    SerdeError(#[from] serde_json::Error),

    /// Other error
    #[error("{0}")]
    Other(String),
}

/// Result type for plugin runtime operations
pub type Result<T> = std::result::Result<T, PluginRuntimeError>;

impl PluginRuntimeError {
    /// Create a new plugin not found error
    pub fn not_found(name: impl fmt::Display) -> Self {
        Self::PluginNotFound(name.to_string())
    }

    /// Create a new already exists error
    pub fn already_exists(name: impl fmt::Display) -> Self {
        Self::PluginAlreadyExists(name.to_string())
    }

    /// Create a new dependency missing error
    pub fn dependency_missing(name: impl fmt::Display) -> Self {
        Self::DependencyMissing(name.to_string())
    }

    /// Create a new dependency cycle error
    pub fn dependency_cycle(msg: impl fmt::Display) -> Self {
        Self::DependencyCycle(msg.to_string())
    }

    /// Create a new invalid state error
    pub fn invalid_state(msg: impl fmt::Display) -> Self {
        Self::InvalidState(msg.to_string())
    }

    /// Create a new config error
    pub fn config(msg: impl fmt::Display) -> Self {
        Self::ConfigError(msg.to_string())
    }

    /// Create a new other error
    pub fn other(msg: impl fmt::Display) -> Self {
        Self::Other(msg.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let err = PluginRuntimeError::not_found("test");
        assert!(matches!(err, PluginRuntimeError::PluginNotFound(_)));

        let err = PluginRuntimeError::already_exists("test");
        assert!(matches!(err, PluginRuntimeError::PluginAlreadyExists(_)));
    }

    #[test]
    fn test_error_display() {
        let err = PluginRuntimeError::PluginNotFound("auth".to_string());
        assert_eq!(err.to_string(), "Plugin not found: auth");
    }
}
