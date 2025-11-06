//! Plugin error types

use std::fmt;

/// Plugin error type
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    /// Initialization failed
    #[error("Initialization failed: {0}")]
    InitError(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// Runtime error
    #[error("Runtime error: {0}")]
    RuntimeError(String),

    /// Dependency missing
    #[error("Dependency missing: {0}")]
    DependencyMissing(String),

    /// Invalid state
    #[error("Invalid state: {0}")]
    InvalidState(String),

    /// Authentication error
    #[error("Authentication error: {0}")]
    AuthError(String),

    /// Authorization error
    #[error("Authorization error: {0}")]
    AuthzError(String),

    /// Transform error
    #[error("Transform error: {0}")]
    TransformError(String),

    /// Protocol error
    #[error("Protocol error: {0}")]
    ProtocolError(String),

    /// I/O error
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// Serialization error
    #[error("Serialization error: {0}")]
    SerdeError(#[from] serde_json::Error),
}

/// Result type for plugin operations
pub type Result<T> = std::result::Result<T, PluginError>;

impl PluginError {
    /// Create a new initialization error
    pub fn init(msg: impl fmt::Display) -> Self {
        Self::InitError(msg.to_string())
    }

    /// Create a new configuration error
    pub fn config(msg: impl fmt::Display) -> Self {
        Self::ConfigError(msg.to_string())
    }

    /// Create a new runtime error
    pub fn runtime(msg: impl fmt::Display) -> Self {
        Self::RuntimeError(msg.to_string())
    }

    /// Create a new dependency missing error
    pub fn dependency(name: impl fmt::Display) -> Self {
        Self::DependencyMissing(name.to_string())
    }

    /// Create a new invalid state error
    pub fn invalid_state(msg: impl fmt::Display) -> Self {
        Self::InvalidState(msg.to_string())
    }

    /// Create a new authentication error
    pub fn auth(msg: impl fmt::Display) -> Self {
        Self::AuthError(msg.to_string())
    }

    /// Create a new authorization error
    pub fn authz(msg: impl fmt::Display) -> Self {
        Self::AuthzError(msg.to_string())
    }

    /// Create a new transform error
    pub fn transform(msg: impl fmt::Display) -> Self {
        Self::TransformError(msg.to_string())
    }

    /// Create a new protocol error
    pub fn protocol(msg: impl fmt::Display) -> Self {
        Self::ProtocolError(msg.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let err = PluginError::init("test");
        assert!(matches!(err, PluginError::InitError(_)));

        let err = PluginError::config("test");
        assert!(matches!(err, PluginError::ConfigError(_)));

        let err = PluginError::runtime("test");
        assert!(matches!(err, PluginError::RuntimeError(_)));
    }

    #[test]
    fn test_error_display() {
        let err = PluginError::InitError("failed to init".to_string());
        assert_eq!(err.to_string(), "Initialization failed: failed to init");
    }
}

