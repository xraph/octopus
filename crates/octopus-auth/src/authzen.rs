//! OpenID AuthZEN Authorization API 1.0 client (external Policy Decision Point).
//!
//! Sends `{subject, action, resource, context}` evaluation requests to a
//! standards-compliant PDP and reads a boolean `decision`. This lets the gateway
//! delegate authorization to any AuthZEN-compatible engine (e.g. warden) without
//! coupling to a vendor-specific API.

use crate::opa::{AuthzContext, AuthzDecision};
use async_trait::async_trait;
use dashmap::DashMap;
use octopus_config::types::AuthZenConfig;
use serde::{Deserialize, Serialize};
use std::time::Instant;
use tracing::{debug, error, warn};

/// Common authorization interface: a Policy Decision Point that renders an
/// allow/deny decision for an authenticated principal acting on a resource.
///
/// Both [`crate::opa::OpaClient`] and [`AuthZenClient`] implement this, so the
/// gateway's external-authorization path is engine-agnostic.
#[async_trait]
pub trait Authorizer: Send + Sync + std::fmt::Debug {
    /// Render a decision for the given authorization context.
    async fn evaluate(&self, ctx: &AuthzContext) -> anyhow::Result<AuthzDecision>;
    /// Short identifier for logging (e.g. "opa", "authzen").
    fn name(&self) -> &'static str;
}

// ── AuthZEN information model (request/response) ────────────────────────────

#[derive(Debug, Serialize)]
struct Subject {
    #[serde(rename = "type")]
    kind: String,
    id: String,
    #[serde(skip_serializing_if = "serde_json::Map::is_empty")]
    properties: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct Action {
    name: String,
}

#[derive(Debug, Serialize)]
struct Resource {
    #[serde(rename = "type")]
    kind: String,
    id: String,
    #[serde(skip_serializing_if = "serde_json::Map::is_empty")]
    properties: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct EvaluationRequest {
    subject: Subject,
    action: Action,
    resource: Resource,
    context: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct EvaluationResponse {
    #[serde(default)]
    decision: bool,
}

/// AuthZEN PDP client with decision caching.
#[derive(Debug)]
pub struct AuthZenClient {
    endpoint: String,
    subject_type: String,
    resource_type: String,
    client: reqwest::Client,
    cache: DashMap<String, (AuthzDecision, Instant)>,
    cache_ttl: std::time::Duration,
    fail_open: bool,
}

impl AuthZenClient {
    /// Create from config.
    pub fn from_config(config: &AuthZenConfig) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder().timeout(config.timeout).build()?;
        Ok(Self {
            endpoint: config.endpoint.clone(),
            subject_type: config.subject_type.clone(),
            resource_type: config.resource_type.clone(),
            client,
            cache: DashMap::new(),
            cache_ttl: config.cache_ttl,
            fail_open: config.fail_open,
        })
    }

    /// Translate the gateway's authz context into an AuthZEN evaluation request.
    /// The HTTP method is the action and the request path is the resource id.
    fn build_request(&self, ctx: &AuthzContext) -> EvaluationRequest {
        let mut subject_props = serde_json::Map::new();
        subject_props.insert("name".to_string(), ctx.principal.name.clone().into());
        subject_props.insert(
            "roles".to_string(),
            serde_json::to_value(&ctx.principal.roles).unwrap_or_default(),
        );
        subject_props.insert(
            "scopes".to_string(),
            serde_json::to_value(&ctx.principal.scopes).unwrap_or_default(),
        );
        // Carry every authenticated attribute through for ABAC policies.
        for (k, v) in &ctx.principal.attributes {
            subject_props.entry(k.clone()).or_insert_with(|| v.clone());
        }

        let mut resource_props = serde_json::Map::new();
        resource_props.insert("upstream".to_string(), ctx.route.upstream.clone().into());
        for (k, v) in &ctx.route.metadata {
            resource_props.insert(k.clone(), serde_json::Value::String(v.clone()));
        }

        let context = serde_json::json!({
            "method": ctx.request.method,
            "path": ctx.request.path,
            "headers": ctx.request.headers,
        });

        EvaluationRequest {
            subject: Subject {
                kind: self.subject_type.clone(),
                id: ctx.principal.id.clone(),
                properties: subject_props,
            },
            action: Action {
                name: ctx.request.method.clone(),
            },
            resource: Resource {
                kind: self.resource_type.clone(),
                id: ctx.request.path.clone(),
                properties: resource_props,
            },
            context,
        }
    }
}

#[async_trait]
impl Authorizer for AuthZenClient {
    async fn evaluate(&self, ctx: &AuthzContext) -> anyhow::Result<AuthzDecision> {
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

        if let Some(entry) = self.cache.get(&cache_key) {
            let (decision, cached_at) = entry.value();
            if cached_at.elapsed() < self.cache_ttl {
                debug!(cache_key = %cache_key, "AuthZEN cache hit");
                return Ok(decision.clone());
            }
            drop(entry);
            self.cache.remove(&cache_key);
        }

        let request = self.build_request(ctx);

        let result = match self.client.post(&self.endpoint).json(&request).send().await {
            Ok(response) if response.status().is_success() => {
                let parsed: EvaluationResponse = response.json().await?;
                if parsed.decision {
                    AuthzDecision::Allow
                } else {
                    AuthzDecision::Deny("Denied by AuthZEN PDP".to_string())
                }
            }
            Ok(response) => {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                if self.fail_open {
                    warn!(status = %status, "AuthZEN PDP error, fail_open=true, allowing");
                    AuthzDecision::Allow
                } else {
                    AuthzDecision::Deny(format!("AuthZEN PDP error: {status} {body}"))
                }
            }
            Err(e) => {
                error!(error = %e, "AuthZEN PDP request failed");
                if self.fail_open {
                    warn!("AuthZEN PDP unreachable, fail_open=true, allowing");
                    AuthzDecision::Allow
                } else {
                    AuthzDecision::Deny(format!("AuthZEN PDP unreachable: {e}"))
                }
            }
        };

        self.cache
            .insert(cache_key, (result.clone(), Instant::now()));
        Ok(result)
    }

    fn name(&self) -> &'static str {
        "authzen"
    }
}

/// Bridge the existing OPA client into the common [`Authorizer`] interface.
#[async_trait]
impl Authorizer for crate::opa::OpaClient {
    async fn evaluate(&self, ctx: &AuthzContext) -> anyhow::Result<AuthzDecision> {
        Self::evaluate(self, ctx).await
    }

    fn name(&self) -> &'static str {
        "opa"
    }
}
