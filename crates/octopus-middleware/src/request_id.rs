//! Request ID middleware for distributed tracing

use async_trait::async_trait;
use bytes::Bytes;
use http::{header::HeaderName, Request, Response};
use http_body_util::Full;
use octopus_core::{Middleware, Next, Result};
use std::fmt;
use uuid::Uuid;

/// Body type alias
pub type Body = Full<Bytes>;

/// Request ID generator strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdGenerator {
    /// Generate UUID v4
    UuidV4,
    /// Generate ULID (Universally Unique Lexicographically Sortable Identifier)
    Ulid,
}

impl IdGenerator {
    /// Generate a new ID
    pub fn generate(&self) -> String {
        match self {
            IdGenerator::UuidV4 => Uuid::new_v4().to_string(),
            IdGenerator::Ulid => {
                // Simple ULID-like: timestamp + random
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis();
                format!("{:016x}{}", now, Uuid::new_v4().simple())
            }
        }
    }
}

/// Configuration for Request ID middleware
#[derive(Debug, Clone)]
pub struct RequestIdConfig {
    /// Header name for request ID
    pub header_name: String,
    /// ID generator strategy
    pub generator: IdGenerator,
    /// Whether to add request ID to response headers
    pub add_to_response: bool,
}

impl Default for RequestIdConfig {
    fn default() -> Self {
        Self {
            header_name: "X-Request-ID".to_string(),
            generator: IdGenerator::UuidV4,
            add_to_response: true,
        }
    }
}

/// Request ID middleware
///
/// Injects a unique request ID for distributed tracing.
/// If a request ID already exists in the headers, it will be preserved.
#[derive(Clone)]
pub struct RequestId {
    config: RequestIdConfig,
    header_name: HeaderName,
}

impl RequestId {
    /// Create a new Request ID middleware with default config
    pub fn new() -> Self {
        Self::with_config(RequestIdConfig::default())
    }

    /// Create a new Request ID middleware with custom config
    pub fn with_config(config: RequestIdConfig) -> Self {
        let header_name = HeaderName::from_bytes(config.header_name.as_bytes())
            .unwrap_or_else(|_| HeaderName::from_static("x-request-id"));

        Self {
            config,
            header_name,
        }
    }

    /// Generate a new request ID
    fn generate_id(&self) -> String {
        self.config.generator.generate()
    }
}

impl Default for RequestId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for RequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RequestId")
            .field("header_name", &self.config.header_name)
            .field("generator", &self.config.generator)
            .finish()
    }
}

#[async_trait]
impl Middleware for RequestId {
    async fn call(&self, mut req: Request<Body>, next: Next) -> Result<Response<Body>> {
        // Check if request already has an ID
        let request_id = if let Some(existing_id) = req.headers().get(&self.header_name) {
            // Use existing ID
            existing_id
                .to_str()
                .ok()
                .map(|s| s.to_string())
                .unwrap_or_else(|| self.generate_id())
        } else {
            // Generate new ID
            let new_id = self.generate_id();
            // Add to request headers
            req.headers_mut()
                .insert(self.header_name.clone(), new_id.parse().unwrap());
            new_id
        };

        // Call next middleware
        let mut response = next.run(req).await?;

        // Add request ID to response headers if configured
        if self.config.add_to_response {
            response
                .headers_mut()
                .insert(self.header_name.clone(), request_id.parse().unwrap());
        }

        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::StatusCode;
    use octopus_core::Error;

    #[derive(Debug)]
    struct TestHandler;

    #[async_trait]
    impl Middleware for TestHandler {
        async fn call(&self, req: Request<Body>, _next: Next) -> Result<Response<Body>> {
            // Echo back the request ID
            let request_id = req
                .headers()
                .get("X-Request-ID")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("none");

            Response::builder()
                .status(StatusCode::OK)
                .body(Full::new(Bytes::from(request_id.to_string())))
                .map_err(|e| Error::Internal(e.to_string()))
        }
    }

    #[tokio::test]
    async fn test_request_id_generation() {
        let middleware = RequestId::new();
        let handler = TestHandler;

        let stack: std::sync::Arc<[std::sync::Arc<dyn Middleware>]> = std::sync::Arc::new([
            std::sync::Arc::new(middleware),
            std::sync::Arc::new(handler),
        ]);

        let next = Next::new(stack);

        let req = Request::builder()
            .uri("/test")
            .body(Body::from(""))
            .unwrap();

        let response = next.run(req).await.unwrap();

        // Should have X-Request-ID header
        assert!(response.headers().contains_key("X-Request-ID"));
    }

    #[tokio::test]
    async fn test_request_id_preservation() {
        let middleware = RequestId::new();
        let handler = TestHandler;

        let stack: std::sync::Arc<[std::sync::Arc<dyn Middleware>]> = std::sync::Arc::new([
            std::sync::Arc::new(middleware),
            std::sync::Arc::new(handler),
        ]);

        let next = Next::new(stack);

        let existing_id = "existing-id-12345";
        let req = Request::builder()
            .uri("/test")
            .header("X-Request-ID", existing_id)
            .body(Body::from(""))
            .unwrap();

        let response = next.run(req).await.unwrap();

        // Should preserve existing ID
        assert_eq!(response.headers().get("X-Request-ID").unwrap(), existing_id);
    }

    #[tokio::test]
    async fn test_ulid_generator() {
        let config = RequestIdConfig {
            header_name: "X-Request-ID".to_string(),
            generator: IdGenerator::Ulid,
            add_to_response: true,
        };

        let middleware = RequestId::with_config(config);
        let id1 = middleware.generate_id();
        let id2 = middleware.generate_id();

        // IDs should be different
        assert_ne!(id1, id2);
        // ULID should be longer than UUID
        assert!(id1.len() > 36);
    }
}
