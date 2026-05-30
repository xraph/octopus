//! Auth gateway middleware - orchestrates authentication and authorization
//!
//! This middleware:
//! 1. Skips OPTIONS preflight requests (CORS compatibility)
//! 2. Checks global skip paths
//! 3. Checks per-route skip_auth flag
//! 4. Selects the appropriate auth provider (route-level or default)
//! 5. Authenticates via the selected provider (with token caching)
//! 6. Injects principal headers for upstream services
//! 7. Runs authorization checks (roles, scopes, Rhai rules, OPA)
//! 8. Returns structured JSON error responses on 401/403

use bytes::Bytes;
use http::{Method, Request, Response, StatusCode};
use http_body_util::Full;
use octopus_auth::{
    AuthProviderRegistry, AuthRequest, AuthResult, AuthzDecision, AuthzEvaluator, RouteAuthzContext,
};
use octopus_config::types::AuthConfig;
use octopus_core::middleware::{Middleware, Next};
use octopus_core::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, warn};

/// Matched route auth context (stored in request extensions by the handler)
#[derive(Debug, Clone)]
pub struct MatchedRouteAuth {
    /// Auth provider name (route-level override)
    pub auth_provider: Option<String>,
    /// Skip authentication
    pub skip_auth: bool,
    /// Required roles
    pub require_roles: Vec<String>,
    /// Required scopes
    pub require_scopes: Vec<String>,
    /// Custom authz rule
    pub authz_rule: Option<String>,
    /// Upstream name
    pub upstream: String,
    /// Route metadata
    pub metadata: HashMap<String, String>,
}

/// Rate limit key set by auth middleware for downstream rate limiter
#[derive(Debug, Clone)]
pub struct AuthRateLimitKey(pub String);

/// Per-route CORS override (stored in request extensions by the handler)
#[derive(Debug, Clone)]
pub struct MatchedRouteCors {
    /// Allowed origins
    pub allowed_origins: Vec<String>,
    /// Allowed HTTP methods
    pub allowed_methods: Vec<String>,
    /// Allowed request headers
    pub allowed_headers: Vec<String>,
    /// Allow credentials
    pub allow_credentials: bool,
    /// Preflight cache max age in seconds
    pub max_age: u64,
}

/// Auth gateway middleware
pub struct AuthGatewayMiddleware {
    registry: Arc<AuthProviderRegistry>,
    authz: Arc<AuthzEvaluator>,
    config: AuthConfig,
}

impl std::fmt::Debug for AuthGatewayMiddleware {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthGatewayMiddleware")
            .field("default_provider", &self.config.default_provider)
            .field("global_enforce", &self.config.global_enforce)
            .finish()
    }
}

impl AuthGatewayMiddleware {
    /// Create a new auth gateway middleware
    pub fn new(
        registry: Arc<AuthProviderRegistry>,
        authz: Arc<AuthzEvaluator>,
        config: AuthConfig,
    ) -> Self {
        Self {
            registry,
            authz,
            config,
        }
    }

    /// Check if a path matches any skip pattern
    fn is_skip_path(&self, path: &str) -> bool {
        for pattern in &self.config.skip_paths {
            if pattern.ends_with('*') {
                let prefix = &pattern[..pattern.len() - 1];
                if path.starts_with(prefix) {
                    return true;
                }
            } else if path == pattern {
                return true;
            }
        }
        false
    }

    /// Build a structured JSON error response
    fn error_response(
        &self,
        status: StatusCode,
        error: &str,
        message: &str,
        provider: Option<&str>,
        extra: Option<serde_json::Value>,
    ) -> Response<Full<Bytes>> {
        let mut body = serde_json::json!({
            "error": error,
            "message": message,
        });

        if let Some(p) = provider {
            body["provider"] = serde_json::json!(p);
        }
        if let Some(extra) = extra {
            if let Some(obj) = extra.as_object() {
                for (k, v) in obj {
                    body[k] = v.clone();
                }
            }
        }

        let body_bytes = serde_json::to_vec(&body).unwrap_or_default();

        let mut builder = Response::builder()
            .status(status)
            .header("Content-Type", "application/json");

        // Only add WWW-Authenticate for 401 with Bearer-based providers
        if status == StatusCode::UNAUTHORIZED {
            let challenge = match provider.and_then(|p| self.registry.get(p)) {
                Some(prov) => match prov.provider_type() {
                    "jwt" | "oidc" => Some("Bearer"),
                    "api_key" => Some("ApiKey"),
                    "forward_auth" => Some("Bearer"),
                    _ => None, // mTLS - no WWW-Authenticate
                },
                None => Some("Bearer"), // default
            };
            if let Some(c) = challenge {
                builder = builder.header("WWW-Authenticate", c);
            }
        }

        builder
            .body(Full::new(Bytes::from(body_bytes)))
            .unwrap_or_else(|_| {
                Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Full::new(Bytes::new()))
                    .unwrap()
            })
    }
}

#[async_trait::async_trait]
impl Middleware for AuthGatewayMiddleware {
    async fn call(
        &self,
        mut req: Request<Full<Bytes>>,
        next: Next,
    ) -> Result<Response<Full<Bytes>>> {
        let method = req.method().clone();
        let path = req.uri().path().to_string();

        // 1. Skip OPTIONS preflight (CORS compatibility)
        if method == Method::OPTIONS {
            debug!(path = %path, "Skipping auth for OPTIONS preflight");
            return next.run(req).await;
        }

        // 2. Check global skip paths
        if self.is_skip_path(&path) {
            debug!(path = %path, "Skipping auth for skip path");
            return next.run(req).await;
        }

        // 3. Check per-route auth config
        let route_auth = req.extensions().get::<MatchedRouteAuth>().cloned();

        if let Some(ref ra) = route_auth {
            if ra.skip_auth {
                debug!(path = %path, "Skipping auth for route with skip_auth=true");
                return next.run(req).await;
            }
        }

        // 4. Determine provider
        let provider_name = route_auth
            .as_ref()
            .and_then(|ra| ra.auth_provider.as_deref())
            .or(self.config.default_provider.as_deref());

        let provider_name = match provider_name {
            Some(p) => p,
            None => {
                if self.config.global_enforce {
                    // No provider configured but enforcement is on
                    return Ok(self.error_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "configuration_error",
                        "Authentication required but no provider configured",
                        None,
                        None,
                    ));
                }
                // No enforcement and no provider - pass through
                return next.run(req).await;
            }
        };

        // 5. Authenticate
        let tls_cn = req
            .extensions()
            .get::<octopus_tls::TlsClientCn>()
            .and_then(|cn| cn.0.clone());
        let auth_request = AuthRequest {
            headers: req.headers(),
            method: req.method(),
            uri: req.uri(),
            tls_client_cn: tls_cn.as_deref(),
        };

        let auth_result = match self
            .registry
            .authenticate(provider_name, &auth_request)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                warn!(error = %e, provider = %provider_name, "Auth provider error");
                return Ok(self.error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "auth_error",
                    &format!("Authentication error: {e}"),
                    Some(provider_name),
                    None,
                ));
            }
        };

        match auth_result {
            AuthResult::Authenticated(principal) => {
                debug!(
                    principal_id = %principal.id,
                    provider = %provider_name,
                    "Authentication successful"
                );

                // 6. Run authorization
                let route_authz = route_auth.as_ref().map(|ra| RouteAuthzContext {
                    require_roles: ra.require_roles.clone(),
                    require_scopes: ra.require_scopes.clone(),
                    custom_rule: ra.authz_rule.clone(),
                });

                if let Some(authz_ctx) = route_authz {
                    let request_headers: HashMap<String, String> = req
                        .headers()
                        .iter()
                        .filter_map(|(k, v)| {
                            v.to_str().ok().map(|v| (k.to_string(), v.to_string()))
                        })
                        .collect();

                    let empty_metadata = HashMap::new();
                    let (upstream, metadata) = route_auth
                        .as_ref()
                        .map(|ra| (ra.upstream.as_str(), &ra.metadata))
                        .unwrap_or(("", &empty_metadata));

                    let decision = self
                        .authz
                        .evaluate(
                            &principal,
                            &authz_ctx,
                            method.as_str(),
                            &path,
                            &request_headers,
                            upstream,
                            metadata,
                        )
                        .await
                        .unwrap_or(AuthzDecision::Deny("Authz evaluation error".to_string()));

                    if let AuthzDecision::Deny(reason) = decision {
                        return Ok(self.error_response(
                            StatusCode::FORBIDDEN,
                            "forbidden",
                            &reason,
                            Some(provider_name),
                            Some(serde_json::json!({
                                "principal_roles": principal.roles,
                                "principal_scopes": principal.scopes,
                            })),
                        ));
                    }
                }

                // 7. Inject principal headers
                let headers = req.headers_mut();
                if let (Ok(name), Ok(val)) = (
                    http::header::HeaderName::from_bytes(self.config.principal_header.as_bytes()),
                    http::HeaderValue::from_str(&principal.id),
                ) {
                    headers.insert(name, val);
                }
                if !principal.roles.is_empty() {
                    if let (Ok(name), Ok(val)) = (
                        http::header::HeaderName::from_bytes(self.config.roles_header.as_bytes()),
                        http::HeaderValue::from_str(&principal.roles.join(",")),
                    ) {
                        headers.insert(name, val);
                    }
                }
                if !principal.scopes.is_empty() {
                    if let (Ok(name), Ok(val)) = (
                        http::header::HeaderName::from_bytes(self.config.scopes_header.as_bytes()),
                        http::HeaderValue::from_str(&principal.scopes.join(",")),
                    ) {
                        headers.insert(name, val);
                    }
                }

                // Store principal in request extensions
                req.extensions_mut().insert(principal.clone());

                // Set rate limit key by identity
                req.extensions_mut()
                    .insert(AuthRateLimitKey(format!("user:{}", principal.id)));

                next.run(req).await
            }
            AuthResult::Unauthenticated => {
                if self.config.global_enforce {
                    Ok(self.error_response(
                        StatusCode::UNAUTHORIZED,
                        "unauthorized",
                        "Authentication required",
                        Some(provider_name),
                        None,
                    ))
                } else {
                    // No credentials but not enforcing - pass through
                    next.run(req).await
                }
            }
            AuthResult::Failed(reason) => Ok(self.error_response(
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                &reason,
                Some(provider_name),
                None,
            )),
        }
    }
}
