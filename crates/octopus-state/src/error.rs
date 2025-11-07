//! Error types for state management

/// Result type for state operations
pub type Result<T> = std::result::Result<T, Error>;

/// Error types for state operations
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Backend connection error
    #[error("Backend connection error: {0}")]
    Connection(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Key not found
    #[error("Key not found: {0}")]
    NotFound(String),

    /// Operation timeout
    #[error("Operation timeout: {0}")]
    Timeout(String),

    /// Compare-and-swap failed
    #[error("Compare-and-swap failed for key: {0}")]
    CasFailed(String),

    /// Backend-specific error
    #[error("Backend error: {0}")]
    Backend(String),

    /// Invalid configuration
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    /// Internal error
    #[error("Internal error: {0}")]
    Internal(String),
}

#[cfg(feature = "redis-backend")]
impl From<redis::RedisError> for Error {
    fn from(err: redis::RedisError) -> Self {
        Error::Backend(err.to_string())
    }
}

#[cfg(feature = "postgres-backend")]
impl From<sqlx::Error> for Error {
    fn from(err: sqlx::Error) -> Self {
        match err {
            sqlx::Error::RowNotFound => Error::NotFound("Row not found".to_string()),
            sqlx::Error::PoolTimedOut => Error::Timeout("Pool timeout".to_string()),
            _ => Error::Backend(err.to_string()),
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Error::Serialization(err.to_string())
    }
}
