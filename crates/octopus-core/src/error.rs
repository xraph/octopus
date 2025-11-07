//! Error types for Octopus Gateway

/// Result type alias using [`Error`]
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Main error type for Octopus Gateway
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// HTTP-related errors
    #[error("HTTP error: {0}")]
    Http(#[from] hyper::Error),

    /// Invalid HTTP request
    #[error("Invalid HTTP request: {0}")]
    InvalidRequest(String),

    /// Route not found
    #[error("Route not found: {0}")]
    RouteNotFound(String),

    /// Upstream connection error
    #[error("Failed to connect to upstream: {0}")]
    UpstreamConnection(String),

    /// Upstream timeout
    #[error("Upstream request timed out")]
    UpstreamTimeout,

    /// Upstream unavailable
    #[error("No healthy upstream instances available")]
    NoHealthyUpstream,

    /// Configuration error
    #[error("Configuration error: {0}")]
    Config(String),

    /// Plugin error
    #[error("Plugin error in '{plugin}': {message}")]
    Plugin {
        /// Plugin name
        plugin: String,
        /// Error message
        message: String,
    },

    /// Middleware error
    #[error("Middleware error: {0}")]
    Middleware(String),

    /// Authentication error
    #[error("Authentication failed: {0}")]
    Authentication(String),

    /// Authorization error
    #[error("Authorization failed: {0}")]
    Authorization(String),

    /// Rate limit exceeded
    #[error("Rate limit exceeded")]
    RateLimitExceeded,

    /// Circuit breaker open
    #[error("Circuit breaker is open for upstream '{0}'")]
    CircuitBreakerOpen(String),

    /// FARP protocol error
    #[error("FARP error: {0}")]
    Farp(String),

    /// Schema error
    #[error("Schema error: {0}")]
    Schema(String),

    /// Discovery backend error
    #[error("Discovery backend error: {0}")]
    Discovery(String),

    /// Runtime error
    #[error("Runtime error: {0}")]
    Runtime(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// HTTP error
    #[error("HTTP error: {0}")]
    HttpError(#[from] http::Error),

    /// Generic error with context
    #[error("{0}")]
    Generic(String),

    /// Internal error (should not happen in production)
    #[error("Internal error: {0}")]
    Internal(String),
}

impl Error {
    /// Convert error to HTTP status code
    pub fn to_status_code(&self) -> http::StatusCode {
        use http::StatusCode;
        match self {
            Error::Http(_) | Error::InvalidRequest(_) => StatusCode::BAD_REQUEST,
            Error::RouteNotFound(_) => StatusCode::NOT_FOUND,
            Error::UpstreamConnection(_) | Error::UpstreamTimeout => StatusCode::BAD_GATEWAY,
            Error::NoHealthyUpstream => StatusCode::SERVICE_UNAVAILABLE,
            Error::Authentication(_) => StatusCode::UNAUTHORIZED,
            Error::Authorization(_) => StatusCode::FORBIDDEN,
            Error::RateLimitExceeded => StatusCode::TOO_MANY_REQUESTS,
            Error::CircuitBreakerOpen(_) => StatusCode::SERVICE_UNAVAILABLE,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// Create a plugin error
    pub fn plugin(plugin: impl Into<String>, message: impl Into<String>) -> Self {
        Error::Plugin {
            plugin: plugin.into(),
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::StatusCode;

    #[test]
    fn test_error_status_codes() {
        assert_eq!(
            Error::RouteNotFound("/test".to_string()).to_status_code(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            Error::Authentication("invalid token".to_string()).to_status_code(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            Error::RateLimitExceeded.to_status_code(),
            StatusCode::TOO_MANY_REQUESTS
        );
    }

    #[test]
    fn test_plugin_error() {
        let err = Error::plugin("jwt-auth", "invalid signature");
        assert!(matches!(err, Error::Plugin { .. }));
        assert!(err.to_string().contains("jwt-auth"));
    }
}
