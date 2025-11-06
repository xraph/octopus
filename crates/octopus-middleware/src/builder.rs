//! Middleware chain builder
//!
//! This module provides a builder pattern for constructing middleware chains.

use crate::*;
use std::sync::Arc;
use std::time::Duration;

/// Middleware chain builder
#[derive(Debug, Default)]
pub struct MiddlewareBuilder {
    middlewares: Vec<Arc<dyn Middleware>>,
}

impl MiddlewareBuilder {
    /// Create a new middleware builder
    #[must_use]
    pub fn new() -> Self {
        Self {
            middlewares: Vec::new(),
        }
    }

    /// Add Request ID middleware
    #[must_use]
    pub fn with_request_id(mut self) -> Self {
        self.middlewares.push(Arc::new(RequestId::new()));
        self
    }

    /// Add Request ID middleware with custom configuration
    #[must_use]
    pub fn with_request_id_config(mut self, config: RequestIdConfig) -> Self {
        self.middlewares
            .push(Arc::new(RequestId::with_config(config)));
        self
    }

    /// Add Timeout middleware with default config (30s timeout)
    #[must_use]
    pub fn with_timeout(mut self) -> Self {
        self.middlewares
            .push(Arc::new(Timeout::new()));
        self
    }
    
    /// Add Timeout middleware with custom duration
    #[must_use]
    pub fn with_timeout_duration(mut self, timeout: Duration) -> Self {
        let config = TimeoutConfig {
            request_timeout: timeout,
            custom_error_message: None,
        };
        self.middlewares
            .push(Arc::new(Timeout::with_config(config)));
        self
    }

    /// Add Timeout middleware with custom configuration
    #[must_use]
    pub fn with_timeout_config(mut self, config: TimeoutConfig) -> Self {
        self.middlewares
            .push(Arc::new(Timeout::with_config(config)));
        self
    }

    /// Add Logging middleware
    #[must_use]
    pub fn with_logging(mut self) -> Self {
        self.middlewares.push(Arc::new(RequestLogger::new()));
        self
    }

    /// Add Logging middleware with custom configuration
    #[must_use]
    pub fn with_logging_config(mut self, config: LoggingConfig) -> Self {
        self.middlewares
            .push(Arc::new(RequestLogger::with_config(config)));
        self
    }

    /// Add Rate Limiting middleware with default config
    #[must_use]
    pub fn with_rate_limit(mut self) -> Self {
        self.middlewares
            .push(Arc::new(RateLimit::new()));
        self
    }

    /// Add Rate Limiting middleware with specific limits
    #[must_use]
    pub fn with_rate_limit_params(mut self, requests_per_window: u32, window: Duration) -> Self {
        let config = RateLimitConfig {
            requests_per_window,
            window_size: window,
            ..Default::default()
        };
        self.middlewares
            .push(Arc::new(RateLimit::with_config(config)));
        self
    }

    /// Add Rate Limiting middleware with custom configuration
    #[must_use]
    pub fn with_rate_limit_config(mut self, config: RateLimitConfig) -> Self {
        self.middlewares
            .push(Arc::new(RateLimit::with_config(config)));
        self
    }

    /// Add CORS middleware
    #[must_use]
    pub fn with_cors(mut self) -> Self {
        self.middlewares.push(Arc::new(Cors::new()));
        self
    }

    /// Add CORS middleware with custom configuration
    #[must_use]
    pub fn with_cors_config(mut self, config: CorsConfig) -> Self {
        self.middlewares
            .push(Arc::new(Cors::with_config(config)));
        self
    }

    /// Add Compression middleware
    #[must_use]
    pub fn with_compression(mut self) -> Self {
        self.middlewares.push(Arc::new(Compression::new()));
        self
    }

    /// Add Compression middleware with custom configuration
    #[must_use]
    pub fn with_compression_config(mut self, config: CompressionConfig) -> Self {
        self.middlewares
            .push(Arc::new(Compression::with_config(config)));
        self
    }

    /// Add custom middleware
    #[must_use]
    pub fn with_middleware(mut self, middleware: Arc<dyn Middleware>) -> Self {
        self.middlewares.push(middleware);
        self
    }

    /// Build the middleware chain
    ///
    /// Returns an `Arc<[Arc<dyn Middleware>]>` for efficient sharing.
    #[must_use]
    pub fn build(self) -> Arc<[Arc<dyn Middleware>]> {
        self.middlewares.into()
    }

    /// Get the number of middlewares in the chain
    #[must_use]
    pub fn len(&self) -> usize {
        self.middlewares.len()
    }

    /// Check if the chain is empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.middlewares.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_empty() {
        let chain = MiddlewareBuilder::new().build();
        assert!(chain.is_empty());
    }

    #[test]
    fn test_builder_single_middleware() {
        let chain = MiddlewareBuilder::new().with_request_id().build();
        assert_eq!(chain.len(), 1);
    }

    #[test]
    fn test_builder_multiple_middlewares() {
        let chain = MiddlewareBuilder::new()
            .with_request_id()
            .with_timeout()
            .with_logging()
            .with_rate_limit()
            .with_cors()
            .with_compression()
            .build();
        assert_eq!(chain.len(), 6);
    }

    #[test]
    fn test_builder_custom_config() {
        let request_id_config = RequestIdConfig {
            header_name: "X-Custom-Request-ID".to_string(),
            ..Default::default()
        };

        let chain = MiddlewareBuilder::new()
            .with_request_id_config(request_id_config)
            .build();
        assert_eq!(chain.len(), 1);
    }

    #[test]
    fn test_builder_len_and_is_empty() {
        let builder = MiddlewareBuilder::new();
        assert_eq!(builder.len(), 0);
        assert!(builder.is_empty());

        let builder = builder.with_request_id();
        assert_eq!(builder.len(), 1);
        assert!(!builder.is_empty());
    }
}

