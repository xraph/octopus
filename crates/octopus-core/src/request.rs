//! Request context and utilities

use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

/// Context attached to each request
#[derive(Debug, Clone)]
pub struct RequestContext {
    /// Unique request ID for tracing
    pub request_id: String,

    /// Route parameters extracted from path
    pub params: HashMap<String, String>,

    /// Matched route metadata
    pub route: Option<RouteInfo>,

    /// Upstream instance selected for this request
    pub upstream: Option<UpstreamInfo>,

    /// Custom metadata that middleware can attach
    pub metadata: Arc<HashMap<String, serde_json::Value>>,

    /// Authentication context (if authenticated)
    pub auth: Option<AuthContext>,
}

impl RequestContext {
    /// Create a new request context
    pub fn new() -> Self {
        Self {
            request_id: Uuid::new_v4().to_string(),
            params: HashMap::new(),
            route: None,
            upstream: None,
            metadata: Arc::new(HashMap::new()),
            auth: None,
        }
    }

    /// Get a path parameter
    pub fn param(&self, key: &str) -> Option<&str> {
        self.params.get(key).map(|s| s.as_str())
    }

    /// Set a path parameter
    pub fn set_param(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.params.insert(key.into(), value.into());
    }

    /// Get metadata value
    pub fn get_metadata(&self, key: &str) -> Option<&serde_json::Value> {
        self.metadata.get(key)
    }

    /// Set metadata value (creates new Arc)
    pub fn set_metadata(&mut self, key: impl Into<String>, value: serde_json::Value) {
        let mut metadata = (*self.metadata).clone();
        metadata.insert(key.into(), value);
        self.metadata = Arc::new(metadata);
    }
}

impl Default for RequestContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Route information
#[derive(Debug, Clone)]
pub struct RouteInfo {
    /// Route path pattern
    pub path: String,

    /// HTTP method
    pub method: String,

    /// Operation ID from schema
    pub operation_id: Option<String>,

    /// Tags from schema
    pub tags: Vec<String>,
}

/// Upstream instance information
#[derive(Debug, Clone)]
pub struct UpstreamInfo {
    /// Upstream cluster name
    pub cluster: String,

    /// Instance address
    pub address: String,

    /// Instance port
    pub port: u16,

    /// Load balancing weight
    pub weight: u32,
}

/// Authentication context
#[derive(Debug, Clone)]
pub struct AuthContext {
    /// Subject (user ID, service account, etc.)
    pub subject: String,

    /// Authentication provider name
    pub provider: String,

    /// Scopes/permissions
    pub scopes: Vec<String>,

    /// Additional claims
    pub claims: HashMap<String, serde_json::Value>,
}

impl AuthContext {
    /// Check if has a specific scope
    pub fn has_scope(&self, scope: &str) -> bool {
        self.scopes.iter().any(|s| s == scope)
    }

    /// Check if has all required scopes
    pub fn has_scopes(&self, required: &[impl AsRef<str>]) -> bool {
        required.iter().all(|s| self.has_scope(s.as_ref()))
    }

    /// Get a claim value
    pub fn get_claim(&self, key: &str) -> Option<&serde_json::Value> {
        self.claims.get(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_context() {
        let mut ctx = RequestContext::new();
        assert!(!ctx.request_id.is_empty());

        ctx.set_param("user_id", "123");
        assert_eq!(ctx.param("user_id"), Some("123"));
    }

    #[test]
    fn test_auth_context_scopes() {
        let auth = AuthContext {
            subject: "user-123".to_string(),
            provider: "jwt".to_string(),
            scopes: vec!["read".to_string(), "write".to_string()],
            claims: HashMap::new(),
        };

        assert!(auth.has_scope("read"));
        assert!(auth.has_scope("write"));
        assert!(!auth.has_scope("admin"));
        assert!(auth.has_scopes(&["read", "write"]));
        assert!(!auth.has_scopes(&["read", "admin"]));
    }
}


