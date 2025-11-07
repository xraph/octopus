//! Authentication provider traits

use crate::error::{PluginError, Result};
use crate::interceptor::Body;
use crate::plugin::Plugin;
use async_trait::async_trait;
use http::Request;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Authentication provider plugin
///
/// Provides custom authentication schemes.
#[async_trait]
pub trait AuthProvider: Plugin {
    /// Authenticate a request
    ///
    /// Returns authentication result indicating whether the
    /// request is authenticated and who the principal is.
    async fn authenticate(&self, req: &Request<Body>) -> Result<AuthResult>;

    /// Validate credentials
    ///
    /// Validates credentials and returns the authenticated principal.
    async fn validate(&self, credentials: &Credentials) -> Result<Principal>;

    /// Extract credentials from request
    ///
    /// Default implementation extracts from Authorization header.
    async fn extract_credentials(&self, req: &Request<Body>) -> Result<Option<Credentials>> {
        if let Some(auth_header) = req.headers().get("authorization") {
            let auth_str = auth_header
                .to_str()
                .map_err(|e| PluginError::auth(format!("Invalid authorization header: {e}")))?;

            if let Some(token) = auth_str.strip_prefix("Bearer ") {
                return Ok(Some(Credentials::Bearer(token.to_string())));
            }

            if let Some(_basic) = auth_str.strip_prefix("Basic ") {
                // Decode base64 basic auth
                return Ok(Some(Credentials::Basic {
                    username: String::new(),
                    password: String::new(),
                }));
            }
        }

        Ok(None)
    }
}

/// Authentication result
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "lowercase")]
pub enum AuthResult {
    /// Request is authenticated with this principal
    Authenticated(Principal),

    /// Request is not authenticated (no credentials provided)
    Unauthenticated,

    /// Authentication failed (invalid credentials)
    Failed(String),
}

impl AuthResult {
    /// Check if authenticated
    pub fn is_authenticated(&self) -> bool {
        matches!(self, AuthResult::Authenticated(_))
    }

    /// Check if unauthenticated
    pub fn is_unauthenticated(&self) -> bool {
        matches!(self, AuthResult::Unauthenticated)
    }

    /// Check if failed
    pub fn is_failed(&self) -> bool {
        matches!(self, AuthResult::Failed(_))
    }

    /// Get the principal if authenticated
    pub fn principal(&self) -> Option<&Principal> {
        match self {
            AuthResult::Authenticated(principal) => Some(principal),
            _ => None,
        }
    }

    /// Get the failure reason if failed
    pub fn failure_reason(&self) -> Option<&str> {
        match self {
            AuthResult::Failed(reason) => Some(reason),
            _ => None,
        }
    }
}

/// Authenticated principal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Principal {
    /// Unique principal ID
    pub id: String,

    /// Principal name/username
    pub name: String,

    /// Roles assigned to this principal
    pub roles: Vec<String>,

    /// Additional attributes
    pub attributes: HashMap<String, serde_json::Value>,
}

impl Principal {
    /// Create a new principal
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            roles: Vec::new(),
            attributes: HashMap::new(),
        }
    }

    /// Add a role
    pub fn with_role(mut self, role: impl Into<String>) -> Self {
        self.roles.push(role.into());
        self
    }

    /// Add an attribute
    pub fn with_attribute(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.attributes.insert(key.into(), value);
        self
    }

    /// Check if principal has a role
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role)
    }

    /// Get an attribute
    pub fn get_attribute(&self, key: &str) -> Option<&serde_json::Value> {
        self.attributes.get(key)
    }
}

/// Credentials (extensible)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Credentials {
    /// Bearer token (JWT, OAuth)
    Bearer(String),

    /// Basic auth credentials
    /// Basic authentication
    Basic {
        /// Username
        username: String,
        /// Password
        password: String,
    },

    /// API key
    ApiKey(String),

    /// Custom credentials
    Custom(serde_json::Value),
}

impl Credentials {
    /// Check if credentials are bearer token
    pub fn is_bearer(&self) -> bool {
        matches!(self, Credentials::Bearer(_))
    }

    /// Check if credentials are basic auth
    pub fn is_basic(&self) -> bool {
        matches!(self, Credentials::Basic { .. })
    }

    /// Check if credentials are API key
    pub fn is_api_key(&self) -> bool {
        matches!(self, Credentials::ApiKey(_))
    }

    /// Get bearer token if applicable
    pub fn as_bearer(&self) -> Option<&str> {
        match self {
            Credentials::Bearer(token) => Some(token),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_result() {
        let principal = Principal::new("user123", "Alice");
        let result = AuthResult::Authenticated(principal);
        assert!(result.is_authenticated());
        assert!(!result.is_unauthenticated());
        assert!(!result.is_failed());
        assert!(result.principal().is_some());

        let result = AuthResult::Unauthenticated;
        assert!(result.is_unauthenticated());

        let result = AuthResult::Failed("invalid token".to_string());
        assert!(result.is_failed());
        assert_eq!(result.failure_reason(), Some("invalid token"));
    }

    #[test]
    fn test_principal() {
        let principal = Principal::new("user123", "Alice")
            .with_role("admin")
            .with_role("user")
            .with_attribute("email", serde_json::json!("alice@example.com"));

        assert_eq!(principal.id, "user123");
        assert_eq!(principal.name, "Alice");
        assert!(principal.has_role("admin"));
        assert!(principal.has_role("user"));
        assert!(!principal.has_role("guest"));
        assert_eq!(
            principal.get_attribute("email"),
            Some(&serde_json::json!("alice@example.com"))
        );
    }

    #[test]
    fn test_credentials() {
        let creds = Credentials::Bearer("token123".to_string());
        assert!(creds.is_bearer());
        assert_eq!(creds.as_bearer(), Some("token123"));

        let creds = Credentials::Basic {
            username: "user".to_string(),
            password: "pass".to_string(),
        };
        assert!(creds.is_basic());

        let creds = Credentials::ApiKey("key123".to_string());
        assert!(creds.is_api_key());
    }
}
