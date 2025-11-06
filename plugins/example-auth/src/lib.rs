//! # Example JWT Authentication Plugin
//!
//! Demonstrates how to build an authentication plugin using the RequestInterceptor trait.
//!
//! ## Features
//!
//! - JWT token validation
//! - Configurable secret key
//! - Optional routes (can skip auth for certain paths)
//! - Custom error responses
//!
//! ## Example
//!
//! ```rust,no_run
//! use example_auth::JwtAuthPlugin;
//! use octopus_plugin_api::prelude::*;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let mut plugin = JwtAuthPlugin::new();
//! plugin.init(serde_json::json!({
//!     "secret": "my-secret-key",
//!     "skip_routes": ["/health", "/metrics"]
//! })).await?;
//! # Ok(())
//! # }
//! ```

use async_trait::async_trait;
use bytes::Bytes;
use http::{Request, Response, StatusCode};
use http_body_util::Full;
use jsonwebtoken::{decode, DecodingKey, Validation};
use octopus_plugin_api::prelude::*;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

/// JWT Authentication Plugin
///
/// Validates JWT tokens in the Authorization header and injects
/// the authenticated principal into the request context.
#[derive(Debug)]
pub struct JwtAuthPlugin {
    config: JwtAuthConfig,
}

/// Plugin configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtAuthConfig {
    /// JWT secret key for validation
    pub secret: String,

    /// Routes that skip authentication
    #[serde(default)]
    pub skip_routes: Vec<String>,

    /// JWT algorithm
    #[serde(default = "default_algorithm")]
    pub algorithm: String,

    /// Whether to require authentication (if false, only validates if present)
    #[serde(default = "default_require_auth")]
    pub require_auth: bool,
}

fn default_algorithm() -> String {
    "HS256".to_string()
}

fn default_require_auth() -> bool {
    true
}

impl Default for JwtAuthConfig {
    fn default() -> Self {
        Self {
            secret: String::new(),
            skip_routes: vec![],
            algorithm: default_algorithm(),
            require_auth: true,
        }
    }
}

/// JWT Claims
#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub exp: usize,
    #[serde(default)]
    pub roles: Vec<String>,
}

impl JwtAuthPlugin {
    /// Create a new JWT authentication plugin
    pub fn new() -> Self {
        Self {
            config: JwtAuthConfig::default(),
        }
    }

    /// Check if a route should skip authentication
    fn should_skip(&self, path: &str) -> bool {
        self.config.skip_routes.iter().any(|route| {
            if route.ends_with('*') {
                path.starts_with(route.trim_end_matches('*'))
            } else {
                path == route
            }
        })
    }

    /// Extract JWT token from Authorization header
    fn extract_token(&self, req: &Request<Full<Bytes>>) -> Option<String> {
        req.headers()
            .get("authorization")
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "))
            .map(String::from)
    }

    /// Validate JWT token
    fn validate_token(&self, token: &str) -> Result<Claims, PluginError> {
        let key = DecodingKey::from_secret(self.config.secret.as_bytes());
        let validation = Validation::default();

        decode::<Claims>(token, &key, &validation)
            .map(|data| data.claims)
            .map_err(|e| PluginError::auth(format!("Invalid JWT token: {}", e)))
    }

    /// Create unauthorized response
    fn unauthorized_response(&self, message: &str) -> Response<Full<Bytes>> {
        Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header("content-type", "application/json")
            .body(Full::new(Bytes::from(
                serde_json::json!({
                    "error": "unauthorized",
                    "message": message
                })
                .to_string(),
            )))
            .unwrap()
    }
}

impl Default for JwtAuthPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for JwtAuthPlugin {
    fn name(&self) -> &str {
        "jwt-auth"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn description(&self) -> &str {
        "JWT authentication plugin"
    }

    fn author(&self) -> &str {
        "Octopus Team"
    }

    async fn init(&mut self, config: serde_json::Value) -> Result<(), PluginError> {
        self.config = serde_json::from_value(config).map_err(|e| {
            PluginError::config(format!("Invalid configuration: {}", e))
        })?;

        if self.config.secret.is_empty() {
            return Err(PluginError::config("JWT secret is required"));
        }

        debug!(
            skip_routes = ?self.config.skip_routes,
            require_auth = self.config.require_auth,
            "JWT auth plugin initialized"
        );

        Ok(())
    }

    async fn start(&mut self) -> Result<(), PluginError> {
        debug!("JWT auth plugin started");
        Ok(())
    }

    async fn stop(&mut self) -> Result<(), PluginError> {
        debug!("JWT auth plugin stopped");
        Ok(())
    }
}

#[async_trait]
impl RequestInterceptor for JwtAuthPlugin {
    async fn intercept_request(
        &self,
        req: &mut Request<Full<Bytes>>,
        ctx: &RequestContext,
    ) -> Result<InterceptorAction, PluginError> {
        let path = req.uri().path();

        // Skip authentication for configured routes
        if self.should_skip(path) {
            debug!(path = %path, "Skipping authentication for route");
            return Ok(InterceptorAction::Continue);
        }

        // Extract token from Authorization header
        let token = match self.extract_token(req) {
            Some(token) => token,
            None => {
                if self.config.require_auth {
                    warn!(
                        request_id = %ctx.request_id,
                        path = %path,
                        "Missing authorization header"
                    );
                    return Ok(InterceptorAction::Return(
                        self.unauthorized_response("Missing authorization header"),
                    ));
                } else {
                    return Ok(InterceptorAction::Continue);
                }
            }
        };

        // Validate token
        match self.validate_token(&token) {
            Ok(claims) => {
                debug!(
                    request_id = %ctx.request_id,
                    user = %claims.sub,
                    roles = ?claims.roles,
                    "JWT token validated"
                );

                // Inject user info into request headers for upstream services
                req.headers_mut().insert(
                    "x-auth-user",
                    claims.sub.parse().unwrap(),
                );

                if !claims.roles.is_empty() {
                    req.headers_mut().insert(
                        "x-auth-roles",
                        claims.roles.join(",").parse().unwrap(),
                    );
                }

                Ok(InterceptorAction::Continue)
            }
            Err(e) => {
                warn!(
                    request_id = %ctx.request_id,
                    error = %e,
                    "JWT validation failed"
                );
                Ok(InterceptorAction::Return(
                    self.unauthorized_response(&e.to_string()),
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_plugin_init() {
        let mut plugin = JwtAuthPlugin::new();

        let config = serde_json::json!({
            "secret": "test-secret",
            "skip_routes": ["/health"]
        });

        plugin.init(config).await.unwrap();
        assert_eq!(plugin.config.secret, "test-secret");
        assert_eq!(plugin.config.skip_routes, vec!["/health"]);
    }

    #[test]
    fn test_should_skip() {
        let plugin = JwtAuthPlugin {
            config: JwtAuthConfig {
                secret: "test".to_string(),
                skip_routes: vec!["/health".to_string(), "/api/public/*".to_string()],
                algorithm: "HS256".to_string(),
                require_auth: true,
            },
        };

        assert!(plugin.should_skip("/health"));
        assert!(plugin.should_skip("/api/public/foo"));
        assert!(plugin.should_skip("/api/public/bar/baz"));
        assert!(!plugin.should_skip("/api/private"));
    }

    #[test]
    fn test_extract_token() {
        let plugin = JwtAuthPlugin::new();

        let req = Request::builder()
            .header("authorization", "Bearer token123")
            .body(Full::new(Bytes::new()))
            .unwrap();

        assert_eq!(plugin.extract_token(&req), Some("token123".to_string()));

        let req = Request::builder()
            .body(Full::new(Bytes::new()))
            .unwrap();

        assert_eq!(plugin.extract_token(&req), None);
    }
}

