//! CORS (Cross-Origin Resource Sharing) middleware

use async_trait::async_trait;
use bytes::Bytes;
use http::{header, HeaderValue, Method, Request, Response, StatusCode};
use http_body_util::Full;
use octopus_core::{Middleware, Next, Result};
use std::fmt;
use std::time::Duration;

/// Body type alias
pub type Body = Full<Bytes>;

/// CORS configuration
#[derive(Debug, Clone)]
pub struct CorsConfig {
    /// Allowed origins (e.g., "*", "https://example.com")
    pub allowed_origins: Vec<String>,
    /// Allowed HTTP methods
    pub allowed_methods: Vec<Method>,
    /// Allowed request headers
    pub allowed_headers: Vec<String>,
    /// Headers exposed to the browser
    pub exposed_headers: Vec<String>,
    /// Max age for preflight cache (in seconds)
    pub max_age: Duration,
    /// Whether to allow credentials (cookies, auth headers)
    pub allow_credentials: bool,
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self {
            allowed_origins: vec!["*".to_string()],
            allowed_methods: vec![
                Method::GET,
                Method::POST,
                Method::PUT,
                Method::DELETE,
                Method::OPTIONS,
            ],
            allowed_headers: vec![
                "Content-Type".to_string(),
                "Authorization".to_string(),
                "X-Request-ID".to_string(),
            ],
            exposed_headers: vec!["Content-Length".to_string(), "X-Request-ID".to_string()],
            max_age: Duration::from_secs(3600),
            allow_credentials: false,
        }
    }
}

/// CORS middleware
///
/// Handles Cross-Origin Resource Sharing (CORS) by:
/// - Responding to preflight OPTIONS requests
/// - Adding appropriate CORS headers to responses
/// - Validating origin against allowed origins
#[derive(Clone)]
pub struct Cors {
    config: CorsConfig,
}

impl Cors {
    /// Create a new CORS middleware with default config (permissive)
    pub fn new() -> Self {
        Self::with_config(CorsConfig::default())
    }

    /// Create a new CORS middleware with custom config
    pub fn with_config(config: CorsConfig) -> Self {
        Self { config }
    }

    /// Create a permissive CORS middleware (allow all)
    pub fn permissive() -> Self {
        Self::new()
    }

    /// Create a restrictive CORS middleware for specific origins
    pub fn for_origins(origins: Vec<String>) -> Self {
        let config = CorsConfig {
            allowed_origins: origins,
            ..Default::default()
        };
        Self::with_config(config)
    }

    /// Check if origin is allowed
    fn is_origin_allowed(&self, origin: &str) -> bool {
        if self.config.allowed_origins.contains(&"*".to_string()) {
            return true;
        }
        self.config.allowed_origins.contains(&origin.to_string())
    }

    /// Get the appropriate Access-Control-Allow-Origin value
    fn get_allow_origin(&self, request_origin: Option<&str>) -> Option<String> {
        if self.config.allowed_origins.contains(&"*".to_string()) {
            // If allowing all origins
            if self.config.allow_credentials {
                // Can't use * with credentials, echo back the origin
                request_origin.map(|s| s.to_string())
            } else {
                Some("*".to_string())
            }
        } else if let Some(origin) = request_origin {
            // If specific origins are allowed
            if self.is_origin_allowed(origin) {
                Some(origin.to_string())
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Handle preflight OPTIONS request
    fn handle_preflight(&self, req: &Request<Body>) -> Response<Body> {
        let origin = req
            .headers()
            .get(header::ORIGIN)
            .and_then(|v| v.to_str().ok());

        let mut response = Response::builder().status(StatusCode::NO_CONTENT);

        // Add CORS headers
        if let Some(allow_origin) = self.get_allow_origin(origin) {
            response = response.header(
                header::ACCESS_CONTROL_ALLOW_ORIGIN,
                HeaderValue::from_str(&allow_origin).unwrap(),
            );
        }

        // Allowed methods
        let methods = self
            .config
            .allowed_methods
            .iter()
            .map(|m| m.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        response = response.header(header::ACCESS_CONTROL_ALLOW_METHODS, methods);

        // Allowed headers
        let headers = self.config.allowed_headers.join(", ");
        response = response.header(header::ACCESS_CONTROL_ALLOW_HEADERS, headers);

        // Max age
        response = response.header(
            header::ACCESS_CONTROL_MAX_AGE,
            self.config.max_age.as_secs().to_string(),
        );

        // Credentials
        if self.config.allow_credentials {
            response = response.header(header::ACCESS_CONTROL_ALLOW_CREDENTIALS, "true");
        }

        response
            .body(Full::new(Bytes::new()))
            .expect("Failed to build CORS preflight response")
    }

    /// Add CORS headers to response
    fn add_cors_headers(
        &self,
        req: &Request<Body>,
        mut response: Response<Body>,
    ) -> Response<Body> {
        let origin = req
            .headers()
            .get(header::ORIGIN)
            .and_then(|v| v.to_str().ok());

        // Add Access-Control-Allow-Origin
        if let Some(allow_origin) = self.get_allow_origin(origin) {
            response.headers_mut().insert(
                header::ACCESS_CONTROL_ALLOW_ORIGIN,
                HeaderValue::from_str(&allow_origin).unwrap(),
            );
        }

        // Add exposed headers
        if !self.config.exposed_headers.is_empty() {
            let exposed = self.config.exposed_headers.join(", ");
            response.headers_mut().insert(
                header::ACCESS_CONTROL_EXPOSE_HEADERS,
                HeaderValue::from_str(&exposed).unwrap(),
            );
        }

        // Add credentials header
        if self.config.allow_credentials {
            response.headers_mut().insert(
                header::ACCESS_CONTROL_ALLOW_CREDENTIALS,
                HeaderValue::from_static("true"),
            );
        }

        response
    }
}

impl Default for Cors {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Cors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Cors")
            .field("allowed_origins", &self.config.allowed_origins)
            .field("allow_credentials", &self.config.allow_credentials)
            .finish()
    }
}

#[async_trait]
impl Middleware for Cors {
    async fn call(&self, req: Request<Body>, next: Next) -> Result<Response<Body>> {
        // Handle preflight OPTIONS request
        if req.method() == Method::OPTIONS {
            return Ok(self.handle_preflight(&req));
        }

        // For actual requests, call next and add CORS headers
        let response = next.run(req.clone()).await?;
        Ok(self.add_cors_headers(&req, response))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use octopus_core::Error;

    #[derive(Debug)]
    struct TestHandler;

    #[async_trait]
    impl Middleware for TestHandler {
        async fn call(&self, _req: Request<Body>, _next: Next) -> Result<Response<Body>> {
            Response::builder()
                .status(StatusCode::OK)
                .body(Full::new(Bytes::from("success")))
                .map_err(|e| Error::Internal(e.to_string()))
        }
    }

    #[tokio::test]
    async fn test_cors_permissive() {
        let cors = Cors::permissive();
        let handler = TestHandler;

        let stack: std::sync::Arc<[std::sync::Arc<dyn Middleware>]> =
            std::sync::Arc::new([std::sync::Arc::new(cors), std::sync::Arc::new(handler)]);

        let next = Next::new(stack);

        let req = Request::builder()
            .uri("/test")
            .header(header::ORIGIN, "https://example.com")
            .body(Body::from(""))
            .unwrap();

        let response = next.run(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .unwrap(),
            "*"
        );
    }

    #[tokio::test]
    async fn test_cors_preflight() {
        let cors = Cors::permissive();
        let handler = TestHandler;

        let stack: std::sync::Arc<[std::sync::Arc<dyn Middleware>]> =
            std::sync::Arc::new([std::sync::Arc::new(cors), std::sync::Arc::new(handler)]);

        let next = Next::new(stack);

        let req = Request::builder()
            .method(Method::OPTIONS)
            .uri("/test")
            .header(header::ORIGIN, "https://example.com")
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
            .body(Body::from(""))
            .unwrap();

        let response = next.run(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert!(response
            .headers()
            .contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN));
        assert!(response
            .headers()
            .contains_key(header::ACCESS_CONTROL_ALLOW_METHODS));
        assert!(response
            .headers()
            .contains_key(header::ACCESS_CONTROL_MAX_AGE));
    }

    #[tokio::test]
    async fn test_cors_specific_origins() {
        let cors = Cors::for_origins(vec!["https://allowed.com".to_string()]);
        let handler = TestHandler;

        let stack: std::sync::Arc<[std::sync::Arc<dyn Middleware>]> =
            std::sync::Arc::new([std::sync::Arc::new(cors), std::sync::Arc::new(handler)]);

        // Allowed origin
        let next = Next::new(stack.clone());
        let req = Request::builder()
            .uri("/test")
            .header(header::ORIGIN, "https://allowed.com")
            .body(Body::from(""))
            .unwrap();

        let response = next.run(req).await.unwrap();
        assert_eq!(
            response
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .unwrap(),
            "https://allowed.com"
        );

        // Disallowed origin
        let next = Next::new(stack.clone());
        let req = Request::builder()
            .uri("/test")
            .header(header::ORIGIN, "https://evil.com")
            .body(Body::from(""))
            .unwrap();

        let response = next.run(req).await.unwrap();
        assert!(!response
            .headers()
            .contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN));
    }

    #[tokio::test]
    async fn test_cors_with_credentials() {
        let config = CorsConfig {
            allowed_origins: vec!["https://example.com".to_string()],
            allow_credentials: true,
            ..Default::default()
        };

        let cors = Cors::with_config(config);
        let handler = TestHandler;

        let stack: std::sync::Arc<[std::sync::Arc<dyn Middleware>]> =
            std::sync::Arc::new([std::sync::Arc::new(cors), std::sync::Arc::new(handler)]);

        let next = Next::new(stack);

        let req = Request::builder()
            .uri("/test")
            .header(header::ORIGIN, "https://example.com")
            .body(Body::from(""))
            .unwrap();

        let response = next.run(req).await.unwrap();

        assert_eq!(
            response
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_CREDENTIALS)
                .unwrap(),
            "true"
        );
    }
}
