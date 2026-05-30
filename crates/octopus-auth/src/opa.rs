//! Open Policy Agent (OPA) client for authorization decisions

use crate::registry::Principal;
use dashmap::DashMap;
use octopus_config::types::OpaConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Instant;
use tracing::{debug, error, warn};

/// OPA authorization decision
#[derive(Debug, Clone, PartialEq)]
pub enum AuthzDecision {
    /// Request is allowed
    Allow,
    /// Request is denied with reason
    Deny(String),
}

/// Context passed to the authz engine
#[derive(Debug, Clone, Serialize)]
pub struct AuthzContext {
    pub principal: AuthzPrincipal,
    pub request: AuthzRequest,
    pub route: AuthzRoute,
}

/// Principal info for authz evaluation
#[derive(Debug, Clone, Serialize)]
pub struct AuthzPrincipal {
    pub id: String,
    pub name: String,
    pub roles: Vec<String>,
    pub scopes: Vec<String>,
    pub attributes: HashMap<String, serde_json::Value>,
}

impl From<&Principal> for AuthzPrincipal {
    fn from(p: &Principal) -> Self {
        Self {
            id: p.id.clone(),
            name: p.name.clone(),
            roles: p.roles.clone(),
            scopes: p.scopes.clone(),
            attributes: p.attributes.clone(),
        }
    }
}

/// Request info for authz evaluation
#[derive(Debug, Clone, Serialize)]
pub struct AuthzRequest {
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
}

/// Route info for authz evaluation
#[derive(Debug, Clone, Serialize)]
pub struct AuthzRoute {
    pub upstream: String,
    pub path: String,
    pub metadata: HashMap<String, String>,
}

/// OPA input wrapper
#[derive(Debug, Serialize)]
struct OpaInput {
    input: AuthzContext,
}

/// OPA response
#[derive(Debug, Deserialize)]
struct OpaResponse {
    result: Option<bool>,
}

/// OPA client with response caching
#[derive(Debug)]
pub struct OpaClient {
    endpoint: String,
    client: reqwest::Client,
    cache: DashMap<String, (AuthzDecision, Instant)>,
    cache_ttl: std::time::Duration,
    fail_open: bool,
}

impl OpaClient {
    /// Create from config
    pub fn from_config(config: &OpaConfig) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(config.timeout)
            .build()?;

        Ok(Self {
            endpoint: config.endpoint.clone(),
            client,
            cache: DashMap::new(),
            cache_ttl: config.cache_ttl,
            fail_open: config.fail_open,
        })
    }

    /// Evaluate an authorization decision
    pub async fn evaluate(&self, ctx: &AuthzContext) -> anyhow::Result<AuthzDecision> {
        // Build cache key (includes header hash to avoid cross-tenant cache hits)
        let header_hash = {
            use sha2::{Digest, Sha256};
            let mut h = Sha256::new();
            for (k, v) in &ctx.request.headers {
                h.update(k.as_bytes());
                h.update(v.as_bytes());
            }
            format!("{:x}", h.finalize())[..8].to_string()
        };
        let cache_key = format!(
            "{}:{}:{}:{}",
            ctx.principal.id, ctx.request.method, ctx.request.path, header_hash
        );

        // Check cache
        if let Some(entry) = self.cache.get(&cache_key) {
            let (decision, cached_at) = entry.value();
            if cached_at.elapsed() < self.cache_ttl {
                debug!(cache_key = %cache_key, "OPA cache hit");
                return Ok(decision.clone());
            }
            drop(entry);
            self.cache.remove(&cache_key);
        }

        // Query OPA
        let input = OpaInput {
            input: ctx.clone(),
        };

        let result = match self.client.post(&self.endpoint).json(&input).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    let opa_response: OpaResponse = response.json().await?;
                    if opa_response.result.unwrap_or(false) {
                        AuthzDecision::Allow
                    } else {
                        AuthzDecision::Deny("Denied by OPA policy".to_string())
                    }
                } else {
                    let status = response.status();
                    let body = response.text().await.unwrap_or_default();
                    if self.fail_open {
                        warn!(status = %status, "OPA returned error, fail_open=true, allowing");
                        AuthzDecision::Allow
                    } else {
                        AuthzDecision::Deny(format!("OPA error: {} {}", status, body))
                    }
                }
            }
            Err(e) => {
                error!(error = %e, "OPA request failed");
                if self.fail_open {
                    warn!("OPA unreachable, fail_open=true, allowing");
                    AuthzDecision::Allow
                } else {
                    AuthzDecision::Deny(format!("OPA unreachable: {}", e))
                }
            }
        };

        // Cache the decision
        self.cache
            .insert(cache_key, (result.clone(), Instant::now()));

        Ok(result)
    }

    /// Cleanup expired cache entries
    pub fn cleanup_cache(&self) {
        self.cache
            .retain(|_, (_, cached_at)| cached_at.elapsed() < self.cache_ttl);
    }
}
