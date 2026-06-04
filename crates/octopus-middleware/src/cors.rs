//! CORS (Cross-Origin Resource Sharing) middleware

use crate::auth_gateway::MatchedRouteCors;
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

    /// Resolve effective CORS config: per-route override or global default
    fn effective_config(&self, req: &Request<Body>) -> CorsConfig {
        if let Some(route_cors) = req.extensions().get::<MatchedRouteCors>() {
            CorsConfig {
                allowed_origins: route_cors.allowed_origins.clone(),
                allowed_methods: route_cors
                    .allowed_methods
                    .iter()
                    .filter_map(|m| m.parse().ok())
                    .collect(),
                allowed_headers: route_cors.allowed_headers.clone(),
                allow_credentials: route_cors.allow_credentials,
                max_age: Duration::from_secs(route_cors.max_age),
                // Inherit exposed_headers from global config
                exposed_headers: self.config.exposed_headers.clone(),
            }
        } else {
            self.config.clone()
        }
    }

    /// Check if origin is allowed. Supports "*", exact match, and a single
    /// wildcard label — "<scheme>://*.suffix" matches any subdomain of suffix
    /// (e.g. "https://*.twinos.cloud" → "https://acme.twinos.cloud"), so a
    /// multi-tenant deployment can allow every tenant subdomain with one entry
    /// while still reflecting the specific origin for credentialed requests.
    fn is_origin_allowed(config: &CorsConfig, origin: &str) -> bool {
        config
            .allowed_origins
            .iter()
            .any(|pattern| Self::origin_matches(pattern, origin))
    }

    /// Match one allowed-origin pattern against a request origin.
    fn origin_matches(pattern: &str, origin: &str) -> bool {
        if pattern == "*" || pattern == origin {
            return true;
        }
        // Wildcard subdomain: "<scheme>://*.suffix".
        if let Some(star) = pattern.find("://*.") {
            let scheme_prefix = &pattern[..star + 3]; // e.g. "https://"
            let suffix = &pattern[star + 4..]; // e.g. ".twinos.cloud"
            if let Some(host) = origin.strip_prefix(scheme_prefix) {
                // Require a non-empty label before the suffix.
                return host.len() > suffix.len() && host.ends_with(suffix);
            }
        }
        false
    }

    /// Get the appropriate Access-Control-Allow-Origin value
    fn get_allow_origin(config: &CorsConfig, request_origin: Option<&str>) -> Option<String> {
        if config.allowed_origins.contains(&"*".to_string()) {
            if config.allow_credentials {
                request_origin.map(|s| s.to_string())
            } else {
                Some("*".to_string())
            }
        } else if let Some(origin) = request_origin {
            if Self::is_origin_allowed(config, origin) {
                Some(origin.to_string())
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Handle preflight OPTIONS request
    fn handle_preflight(config: &CorsConfig, req: &Request<Body>) -> Response<Body> {
        let origin = req
            .headers()
            .get(header::ORIGIN)
            .and_then(|v| v.to_str().ok());

        let mut response = Response::builder().status(StatusCode::NO_CONTENT);

        if let Some(allow_origin) = Self::get_allow_origin(config, origin) {
            response = response.header(
                header::ACCESS_CONTROL_ALLOW_ORIGIN,
                HeaderValue::from_str(&allow_origin).unwrap(),
            );
        }

        let methods = config
            .allowed_methods
            .iter()
            .map(|m| m.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        response = response.header(header::ACCESS_CONTROL_ALLOW_METHODS, methods);

        let headers = config.allowed_headers.join(", ");
        response = response.header(header::ACCESS_CONTROL_ALLOW_HEADERS, headers);

        response = response.header(
            header::ACCESS_CONTROL_MAX_AGE,
            config.max_age.as_secs().to_string(),
        );

        if config.allow_credentials {
            response = response.header(header::ACCESS_CONTROL_ALLOW_CREDENTIALS, "true");
        }

        response
            .body(Full::new(Bytes::new()))
            .expect("Failed to build CORS preflight response")
    }

    /// Add CORS headers to response
    fn add_cors_headers(
        config: &CorsConfig,
        req: &Request<Body>,
        mut response: Response<Body>,
    ) -> Response<Body> {
        let origin = req
            .headers()
            .get(header::ORIGIN)
            .and_then(|v| v.to_str().ok());

        if let Some(allow_origin) = Self::get_allow_origin(config, origin) {
            response.headers_mut().insert(
                header::ACCESS_CONTROL_ALLOW_ORIGIN,
                HeaderValue::from_str(&allow_origin).unwrap(),
            );
        }

        if !config.exposed_headers.is_empty() {
            let exposed = config.exposed_headers.join(", ");
            response.headers_mut().insert(
                header::ACCESS_CONTROL_EXPOSE_HEADERS,
                HeaderValue::from_str(&exposed).unwrap(),
            );
        }

        if config.allow_credentials {
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
        // Resolve effective CORS config (per-route override or global)
        let effective = self.effective_config(&req);

        // Handle preflight OPTIONS request
        if req.method() == Method::OPTIONS {
            return Ok(Self::handle_preflight(&effective, &req));
        }

        // For actual requests, call next and add CORS headers
        let response = next.run(req.clone()).await?;
        Ok(Self::add_cors_headers(&effective, &req, response))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use octopus_core::Error;

    #[test]
    fn wildcard_subdomain_origin_matching() {
        let config = CorsConfig {
            allowed_origins: vec!["https://*.twinos.cloud".to_string()],
            allow_credentials: true,
            ..Default::default()
        };
        // Any tenant subdomain matches.
        assert!(Cors::is_origin_allowed(
            &config,
            "https://acme.twinos.cloud"
        ));
        assert!(Cors::is_origin_allowed(
            &config,
            "https://acme.api.twinos.cloud"
        ));
        // Spoofed / non-matching origins are rejected.
        assert!(!Cors::is_origin_allowed(&config, "https://evil.com"));
        assert!(!Cors::is_origin_allowed(&config, "https://twinos.cloud")); // apex needs its own entry
        assert!(!Cors::is_origin_allowed(
            &config,
            "http://acme.twinos.cloud"
        )); // scheme mismatch
        assert!(!Cors::is_origin_allowed(
            &config,
            "https://acmetwinos.cloud"
        )); // missing separator
            // The specific origin is reflected (required with credentials).
        assert_eq!(
            Cors::get_allow_origin(&config, Some("https://acme.twinos.cloud")),
            Some("https://acme.twinos.cloud".to_string())
        );
    }

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
