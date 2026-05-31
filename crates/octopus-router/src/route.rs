//! Route definition and builder

use crate::convention::Convention;
use crate::host::HostMatch;
use http::Method;
use octopus_core::{Error, Result};
use std::collections::HashMap;
use std::time::Duration;

/// Route definition
#[derive(Debug, Clone)]
pub struct Route {
    /// HTTP method
    pub method: Method,

    /// Host the route is scoped to (defaults to [`HostMatch::Any`]).
    pub host: HostMatch,

    /// Path pattern (e.g., "/users/:id")
    pub path: String,

    /// Upstream cluster name
    pub upstream_name: String,

    /// Priority (higher = matched first)
    pub priority: i32,

    /// Route metadata
    pub metadata: HashMap<String, String>,

    /// Path prefix to strip before forwarding
    pub strip_prefix: Option<String>,

    /// Path prefix to add before forwarding
    pub add_prefix: Option<String>,

    /// Auth provider name (overrides global default)
    pub auth_provider: Option<String>,

    /// Skip authentication for this route
    pub skip_auth: bool,

    /// Required roles for authorization
    pub require_roles: Vec<String>,

    /// Required scopes for authorization
    pub require_scopes: Vec<String>,

    /// Custom authorization rule (Rhai expression)
    pub authz_rule: Option<String>,

    /// Per-route request timeout override
    pub timeout: Option<Duration>,

    /// Per-route rate limit (requests_per_window, window_size)
    pub rate_limit: Option<(u32, Duration)>,

    /// Per-route CORS override
    pub cors: Option<RouteCorsOverride>,

    /// Convention for deriving the upstream from the request host (multi-tenant
    /// subdomain routing). When set, the handler derives `{namespace, service}`
    /// from the host instead of using `upstream_name`.
    pub convention: Option<Convention>,
}

/// Per-route CORS override configuration
#[derive(Debug, Clone)]
pub struct RouteCorsOverride {
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

impl Route {
    /// Create a new route builder
    pub fn builder() -> RouteBuilder {
        RouteBuilder::new()
    }
}

/// Builder for constructing routes
#[derive(Debug, Default)]
pub struct RouteBuilder {
    method: Option<Method>,
    host: HostMatch,
    path: Option<String>,
    upstream_name: Option<String>,
    priority: i32,
    metadata: HashMap<String, String>,
    strip_prefix: Option<String>,
    add_prefix: Option<String>,
    auth_provider: Option<String>,
    skip_auth: bool,
    require_roles: Vec<String>,
    require_scopes: Vec<String>,
    authz_rule: Option<String>,
    timeout: Option<Duration>,
    rate_limit: Option<(u32, Duration)>,
    cors: Option<RouteCorsOverride>,
    convention: Option<Convention>,
}

impl RouteBuilder {
    /// Create a new route builder
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the HTTP method
    pub fn method(mut self, method: Method) -> Self {
        self.method = Some(method);
        self
    }

    /// Set the host this route is scoped to (defaults to [`HostMatch::Any`])
    pub fn host(mut self, host: HostMatch) -> Self {
        self.host = host;
        self
    }

    /// Set the path pattern
    pub fn path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// Set the upstream cluster name
    pub fn upstream_name(mut self, name: impl Into<String>) -> Self {
        self.upstream_name = Some(name.into());
        self
    }

    /// Set the priority
    pub fn priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    /// Add metadata
    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Set strip prefix
    pub fn strip_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.strip_prefix = Some(prefix.into());
        self
    }

    /// Set add prefix
    pub fn add_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.add_prefix = Some(prefix.into());
        self
    }

    /// Set auth provider name
    pub fn auth_provider(mut self, provider: Option<&str>) -> Self {
        self.auth_provider = provider.map(String::from);
        self
    }

    /// Set skip auth flag
    pub fn skip_auth(mut self, skip: bool) -> Self {
        self.skip_auth = skip;
        self
    }

    /// Set required roles
    pub fn require_roles(mut self, roles: &[String]) -> Self {
        self.require_roles = roles.to_vec();
        self
    }

    /// Set required scopes
    pub fn require_scopes(mut self, scopes: &[String]) -> Self {
        self.require_scopes = scopes.to_vec();
        self
    }

    /// Set custom authz rule
    pub fn authz_rule(mut self, rule: Option<&str>) -> Self {
        self.authz_rule = rule.map(String::from);
        self
    }

    /// Set per-route timeout
    pub fn timeout(mut self, timeout: Option<Duration>) -> Self {
        self.timeout = timeout;
        self
    }

    /// Set per-route rate limit
    pub fn rate_limit(mut self, requests: u32, window: Duration) -> Self {
        self.rate_limit = Some((requests, window));
        self
    }

    /// Set per-route CORS override
    pub fn cors(mut self, cors: Option<RouteCorsOverride>) -> Self {
        self.cors = cors;
        self
    }

    /// Set the host-derivation convention for this route.
    pub fn convention(mut self, convention: Option<Convention>) -> Self {
        self.convention = convention;
        self
    }

    /// Build the route
    pub fn build(self) -> Result<Route> {
        let method = self
            .method
            .ok_or_else(|| Error::Config("method is required".to_string()))?;

        let path = self
            .path
            .ok_or_else(|| Error::Config("path is required".to_string()))?;

        let upstream_name = self
            .upstream_name
            .ok_or_else(|| Error::Config("upstream_name is required".to_string()))?;

        // Validate path
        if !path.starts_with('/') {
            return Err(Error::Config("path must start with '/'".to_string()));
        }

        Ok(Route {
            method,
            host: self.host,
            path,
            upstream_name,
            priority: self.priority,
            metadata: self.metadata,
            strip_prefix: self.strip_prefix,
            add_prefix: self.add_prefix,
            auth_provider: self.auth_provider,
            skip_auth: self.skip_auth,
            require_roles: self.require_roles,
            require_scopes: self.require_scopes,
            authz_rule: self.authz_rule,
            timeout: self.timeout,
            rate_limit: self.rate_limit,
            cors: self.cors,
            convention: self.convention,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_defaults_to_any_host() {
        let route = RouteBuilder::new()
            .method(Method::GET)
            .path("/x")
            .upstream_name("u")
            .build()
            .unwrap();
        assert_eq!(route.host, HostMatch::Any);
    }

    #[test]
    fn route_builder_sets_convention() {
        let conv = crate::Convention {
            base_suffix: ".platform.com".into(),
            roles: vec![crate::LabelRole::Service, crate::LabelRole::Namespace],
            default_service: None,
            port: 8080,
            script: None,
            backend: crate::BackendStrategy::default(),
        };
        let route = RouteBuilder::new()
            .method(Method::GET)
            .path("/x")
            .upstream_name("u")
            .convention(Some(conv.clone()))
            .build()
            .unwrap();
        assert_eq!(route.convention, Some(conv));
    }

    #[test]
    fn route_defaults_to_no_convention() {
        let route = RouteBuilder::new()
            .method(Method::GET)
            .path("/x")
            .upstream_name("u")
            .build()
            .unwrap();
        assert_eq!(route.convention, None);
    }

    #[test]
    fn route_builder_sets_host() {
        let route = RouteBuilder::new()
            .method(Method::GET)
            .path("/x")
            .upstream_name("u")
            .host(HostMatch::Exact("a.com".into()))
            .build()
            .unwrap();
        assert_eq!(route.host, HostMatch::Exact("a.com".into()));
    }

    #[test]
    fn test_route_builder() {
        let route = RouteBuilder::new()
            .method(Method::GET)
            .path("/users/:id")
            .upstream_name("user-service")
            .priority(10)
            .metadata("version", "v1")
            .build()
            .unwrap();

        assert_eq!(route.method, Method::GET);
        assert_eq!(route.path, "/users/:id");
        assert_eq!(route.upstream_name, "user-service");
        assert_eq!(route.priority, 10);
        assert_eq!(route.metadata.get("version"), Some(&"v1".to_string()));
    }

    #[test]
    fn test_route_builder_missing_fields() {
        let result = RouteBuilder::new().path("/users").build();

        assert!(result.is_err());
    }

    #[test]
    fn test_route_builder_invalid_path() {
        let result = RouteBuilder::new()
            .method(Method::GET)
            .path("users") // Missing leading slash
            .upstream_name("service")
            .build();

        assert!(result.is_err());
    }

    #[test]
    fn test_route_with_prefix_operations() {
        let route = RouteBuilder::new()
            .method(Method::GET)
            .path("/api/users/*")
            .upstream_name("user-service")
            .strip_prefix("/api")
            .add_prefix("/v1")
            .build()
            .unwrap();

        assert_eq!(route.strip_prefix, Some("/api".to_string()));
        assert_eq!(route.add_prefix, Some("/v1".to_string()));
    }

    #[test]
    fn test_route_with_auth() {
        let route = RouteBuilder::new()
            .method(Method::GET)
            .path("/api/admin/*")
            .upstream_name("admin-service")
            .auth_provider(Some("internal-jwt"))
            .require_roles(&["admin".to_string()])
            .require_scopes(&["read".to_string(), "write".to_string()])
            .authz_rule(Some("has_role(\"admin\")"))
            .build()
            .unwrap();

        assert_eq!(route.auth_provider, Some("internal-jwt".to_string()));
        assert!(!route.skip_auth);
        assert_eq!(route.require_roles, vec!["admin".to_string()]);
        assert_eq!(
            route.require_scopes,
            vec!["read".to_string(), "write".to_string()]
        );
        assert_eq!(route.authz_rule, Some("has_role(\"admin\")".to_string()));
    }

    #[test]
    fn test_route_skip_auth() {
        let route = RouteBuilder::new()
            .method(Method::GET)
            .path("/health")
            .upstream_name("backend")
            .skip_auth(true)
            .build()
            .unwrap();

        assert!(route.skip_auth);
        assert!(route.auth_provider.is_none());
    }
}
