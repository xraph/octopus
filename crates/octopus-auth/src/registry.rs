//! Auth provider registry - manages named auth provider instances with token caching

use async_trait::async_trait;
use dashmap::DashMap;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Authenticated principal identity
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Principal {
    /// Unique identifier (user ID, API key name, client cert CN)
    pub id: String,
    /// Display name
    pub name: String,
    /// Assigned roles
    pub roles: Vec<String>,
    /// Granted scopes
    pub scopes: Vec<String>,
    /// Provider that authenticated this principal
    pub provider: String,
    /// Additional attributes
    pub attributes: std::collections::HashMap<String, serde_json::Value>,
}

/// Authentication result
#[derive(Debug, Clone)]
pub enum AuthResult {
    /// Successfully authenticated
    Authenticated(Principal),
    /// No credentials provided
    Unauthenticated,
    /// Credentials provided but invalid
    Failed(String),
}

/// Request data needed for authentication
#[derive(Debug)]
pub struct AuthRequest<'a> {
    /// Request headers
    pub headers: &'a http::HeaderMap,
    /// HTTP method
    pub method: &'a http::Method,
    /// Request URI
    pub uri: &'a http::Uri,
    /// TLS client certificate CN (for mTLS)
    pub tls_client_cn: Option<&'a str>,
}

/// Trait for auth provider implementations
#[async_trait]
pub trait AuthProviderInstance: Send + Sync + fmt::Debug {
    /// Authenticate a request
    async fn authenticate(&self, req: &AuthRequest<'_>) -> anyhow::Result<AuthResult>;
    /// Provider name
    fn name(&self) -> &str;
    /// Provider type (jwt, oidc, api_key, forward_auth, mtls)
    fn provider_type(&self) -> &str;
}

/// Registry of named auth providers with token caching
#[derive(Debug)]
pub struct AuthProviderRegistry {
    providers: DashMap<String, Arc<dyn AuthProviderInstance>>,
    default_provider: Option<String>,
    token_cache: DashMap<String, (Principal, Instant)>,
    cache_ttl: Duration,
}

impl AuthProviderRegistry {
    /// Create a new registry
    pub fn new(default_provider: Option<String>, cache_ttl: Duration) -> Self {
        Self {
            providers: DashMap::new(),
            default_provider,
            token_cache: DashMap::new(),
            cache_ttl,
        }
    }

    /// Register a named auth provider
    pub fn register(&self, name: impl Into<String>, provider: Arc<dyn AuthProviderInstance>) {
        self.providers.insert(name.into(), provider);
    }

    /// Get the default provider name
    pub fn default_provider(&self) -> Option<&String> {
        self.default_provider.as_ref()
    }

    /// Get a provider by name
    pub fn get(&self, name: &str) -> Option<Arc<dyn AuthProviderInstance>> {
        self.providers.get(name).map(|p| Arc::clone(p.value()))
    }

    /// List all registered provider names
    pub fn list(&self) -> Vec<String> {
        self.providers.iter().map(|e| e.key().clone()).collect()
    }

    /// Authenticate using a specific provider, with caching
    pub async fn authenticate(
        &self,
        provider_name: &str,
        req: &AuthRequest<'_>,
    ) -> anyhow::Result<AuthResult> {
        // Build a cache key from the provider name + auth header value
        let cache_key = self.build_cache_key(provider_name, req);

        // Check cache
        if let Some(cache_key) = &cache_key {
            if let Some(entry) = self.token_cache.get(cache_key) {
                let (principal, cached_at) = entry.value();
                if cached_at.elapsed() < self.cache_ttl {
                    return Ok(AuthResult::Authenticated(principal.clone()));
                }
                // Expired - remove
                drop(entry);
                self.token_cache.remove(cache_key);
            }
        }

        // Authenticate
        let provider = self
            .providers
            .get(provider_name)
            .ok_or_else(|| anyhow::anyhow!("Auth provider '{}' not found", provider_name))?;

        let result = provider.authenticate(req).await?;

        // Cache successful auth
        if let AuthResult::Authenticated(ref principal) = result {
            if let Some(cache_key) = cache_key {
                self.token_cache
                    .insert(cache_key, (principal.clone(), Instant::now()));
            }
        }

        Ok(result)
    }

    /// Build cache key from provider name and request auth header
    fn build_cache_key(&self, provider_name: &str, req: &AuthRequest<'_>) -> Option<String> {
        // Use the Authorization header value as part of the cache key
        if let Some(auth) = req.headers.get(http::header::AUTHORIZATION) {
            if let Ok(value) = auth.to_str() {
                // Hash the token to keep cache keys manageable
                use sha2::{Digest, Sha256};
                let mut hasher = Sha256::new();
                hasher.update(provider_name.as_bytes());
                hasher.update(value.as_bytes());
                let hash = hasher.finalize();
                return Some(format!("{:x}", hash));
            }
        }
        // For API key in custom header or mTLS, use those values
        if let Some(cn) = req.tls_client_cn {
            return Some(format!("mtls:{}:{}", provider_name, cn));
        }
        None
    }

    /// Cleanup expired cache entries
    pub fn cleanup_cache(&self) {
        self.token_cache
            .retain(|_, (_, cached_at)| cached_at.elapsed() < self.cache_ttl);
    }

    /// Number of cached entries
    pub fn cache_size(&self) -> usize {
        self.token_cache.len()
    }

    /// Number of registered providers
    pub fn provider_count(&self) -> usize {
        self.providers.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct MockProvider {
        name: String,
    }

    #[async_trait]
    impl AuthProviderInstance for MockProvider {
        async fn authenticate(&self, _req: &AuthRequest<'_>) -> anyhow::Result<AuthResult> {
            Ok(AuthResult::Authenticated(Principal {
                id: "test-user".to_string(),
                name: "Test User".to_string(),
                roles: vec!["user".to_string()],
                scopes: vec!["read".to_string()],
                provider: self.name.clone(),
                attributes: std::collections::HashMap::new(),
            }))
        }

        fn name(&self) -> &str {
            &self.name
        }

        fn provider_type(&self) -> &str {
            "mock"
        }
    }

    #[tokio::test]
    async fn test_registry_basic() {
        let registry = AuthProviderRegistry::new(Some("default".to_string()), Duration::from_secs(60));
        let provider = Arc::new(MockProvider {
            name: "test".to_string(),
        });
        registry.register("test", provider);

        assert_eq!(registry.provider_count(), 1);
        assert!(registry.get("test").is_some());
        assert!(registry.get("nonexistent").is_none());
    }
}
