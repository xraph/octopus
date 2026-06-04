//! Convention-aware token introspection.
//!
//! The per-tenant Go gateway validated tokens against each tenant's own identity
//! service. To collapse that hop into the edge Octopus, this provider resolves
//! the request host to a `{namespace}` using the same [`Convention`] (and Rhai
//! host-resolution script) the router uses, then delegates to a per-namespace
//! [`IntrospectionProvider`] whose endpoint is `endpoint_template` with
//! `{namespace}` substituted (e.g. `http://authsome.{namespace}.svc/v1/introspect`).
//!
//! Per-namespace providers are held in a bounded, idle-expiring cache and share a
//! single HTTP client, so unbounded tenant churn can't leak memory or connection
//! pools (an evicted provider is simply rebuilt on the next request).

use crate::introspection_provider::IntrospectionProvider;
use crate::registry::{AuthProviderInstance, AuthRequest, AuthResult};
use async_trait::async_trait;
use octopus_config::types::IntrospectionProviderConfig;
use octopus_router::Convention;
use std::sync::Arc;
use std::time::Duration;
use tracing::warn;

/// Placeholder in `endpoint_template` replaced with the resolved namespace.
const NAMESPACE_PLACEHOLDER: &str = "{namespace}";

/// Max distinct tenant namespaces kept warm before idle eviction.
const PROVIDER_CACHE_CAPACITY: u64 = 10_000;

/// Evict a per-namespace provider after this long without use.
const PROVIDER_CACHE_TTI: Duration = Duration::from_secs(3600);

/// Shared, stateless Rhai engine for convention host-resolution scripts (mirrors
/// the router's host-resolution path so auth derives the same namespace).
fn host_script_engine() -> &'static octopus_scripting::RhaiEngine {
    static ENGINE: std::sync::OnceLock<octopus_scripting::RhaiEngine> = std::sync::OnceLock::new();
    ENGINE.get_or_init(octopus_scripting::RhaiEngine::new)
}

/// Auth provider that introspects tokens against a per-tenant endpoint derived
/// from the request host via a [`Convention`].
#[derive(Debug)]
pub struct ConventionAuthProvider {
    name: String,
    convention: Convention,
    endpoint_template: String,
    base_config: IntrospectionProviderConfig,
    client: reqwest::Client,
    /// Bounded, idle-expiring per-namespace providers (share `client`).
    providers: moka::sync::Cache<String, Arc<IntrospectionProvider>>,
}

impl ConventionAuthProvider {
    /// Create a convention auth provider. `endpoint_template` should contain
    /// `{namespace}`; `base_config.endpoint` is ignored (substituted per
    /// namespace). All per-namespace providers reuse one HTTP client.
    pub fn new(
        name: &str,
        convention: Convention,
        endpoint_template: impl Into<String>,
        base_config: IntrospectionProviderConfig,
    ) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(base_config.timeout)
            .build()?;
        Ok(Self {
            name: name.to_string(),
            convention,
            endpoint_template: endpoint_template.into(),
            base_config,
            client,
            providers: moka::sync::Cache::builder()
                .max_capacity(PROVIDER_CACHE_CAPACITY)
                .time_to_idle(PROVIDER_CACHE_TTI)
                .build(),
        })
    }

    /// The introspection endpoint for `namespace`.
    fn endpoint_for(&self, namespace: &str) -> String {
        self.endpoint_template
            .replace(NAMESPACE_PLACEHOLDER, namespace)
    }

    /// Get (or lazily build, race-safe) the introspection provider for
    /// `namespace`. Built providers share this provider's HTTP client.
    fn provider_for(&self, namespace: &str) -> Arc<IntrospectionProvider> {
        self.providers.get_with(namespace.to_string(), || {
            let mut config = self.base_config.clone();
            config.endpoint = self.endpoint_for(namespace);
            Arc::new(IntrospectionProvider::with_client(
                &format!("{}:{namespace}", self.name),
                &config,
                self.client.clone(),
            ))
        })
    }

    /// Resolve the tenant namespace for `host` the same way the router does:
    /// the convention's optional Rhai script first (which may decline), then the
    /// label layout.
    async fn resolve_namespace(&self, host: &str) -> Option<String> {
        if let Some(script) = &self.convention.script {
            match host_script_engine().resolve_host(script, host).await {
                Ok(Some(res)) => return Some(res.namespace),
                Ok(None) => {} // script declined → fall back to label layout
                Err(e) => {
                    warn!(host = %host, error = %e, "host-resolution script failed in auth; falling back to convention layout");
                }
            }
        }
        self.convention.resolve(host).map(|t| t.namespace)
    }
}

/// Lowercased request host: the URI authority if present, else the `Host`
/// header (port stripped).
fn request_host(req: &AuthRequest<'_>) -> Option<String> {
    if let Some(host) = req.uri.host() {
        return Some(host.to_ascii_lowercase());
    }
    req.headers
        .get(http::header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(|h| h.split(':').next().unwrap_or(h).to_ascii_lowercase())
}

#[async_trait]
impl AuthProviderInstance for ConventionAuthProvider {
    async fn authenticate(&self, req: &AuthRequest<'_>) -> anyhow::Result<AuthResult> {
        let Some(host) = request_host(req) else {
            return Ok(AuthResult::Unauthenticated);
        };
        let Some(namespace) = self.resolve_namespace(&host).await else {
            return Ok(AuthResult::Failed(format!(
                "host '{host}' is not within the convention domain"
            )));
        };
        self.provider_for(&namespace).authenticate(req).await
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn provider_type(&self) -> &'static str {
        "convention"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use octopus_router::{BackendStrategy, LabelRole};

    fn tenants_convention() -> Convention {
        Convention {
            base_suffix: ".twinos.cloud".into(),
            roles: vec![LabelRole::Namespace],
            default_service: Some("studio".into()),
            port: 3000,
            script: None,
            backend: BackendStrategy::default(),
            route_rules: Vec::new(),
        }
    }

    fn base_cfg() -> IntrospectionProviderConfig {
        IntrospectionProviderConfig {
            endpoint: String::new(),
            header_name: "authorization".into(),
            token_prefix: "Bearer ".into(),
            client_id: None,
            client_secret: None,
            subject_field: "sub".into(),
            roles_field: None,
            scope_field: "scope".into(),
            timeout: Duration::from_secs(5),
        }
    }

    fn provider_with(convention: Convention) -> ConventionAuthProvider {
        ConventionAuthProvider::new(
            "tenant-auth",
            convention,
            "http://authsome.{namespace}.svc/v1/introspect",
            base_cfg(),
        )
        .unwrap()
    }

    fn provider() -> ConventionAuthProvider {
        provider_with(tenants_convention())
    }

    #[test]
    fn endpoint_template_substitutes_namespace() {
        let p = provider();
        assert_eq!(
            p.endpoint_for("customer-a"),
            "http://authsome.customer-a.svc/v1/introspect"
        );
    }

    #[test]
    fn per_namespace_providers_are_cached_and_distinct() {
        let p = provider();
        let a1 = p.provider_for("customer-a");
        let a2 = p.provider_for("customer-a");
        assert!(
            Arc::ptr_eq(&a1, &a2),
            "same namespace returns the cached provider"
        );
        let b = p.provider_for("customer-b");
        assert!(
            !Arc::ptr_eq(&a1, &b),
            "different namespaces get distinct providers"
        );
    }

    #[tokio::test]
    async fn host_outside_convention_domain_fails() {
        let p = provider();
        let headers = http::HeaderMap::new();
        let method = http::Method::GET;
        let uri: http::Uri = "http://evil.com/x".parse().unwrap();
        let req = AuthRequest {
            headers: &headers,
            method: &method,
            uri: &uri,
            tls_client_cn: None,
        };
        match p.authenticate(&req).await.unwrap() {
            AuthResult::Failed(_) => {}
            other => panic!("expected Failed for out-of-domain host, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn missing_host_is_unauthenticated() {
        let p = provider();
        let headers = http::HeaderMap::new();
        let method = http::Method::GET;
        let uri: http::Uri = "/just/a/path".parse().unwrap();
        let req = AuthRequest {
            headers: &headers,
            method: &method,
            uri: &uri,
            tls_client_cn: None,
        };
        assert!(matches!(
            p.authenticate(&req).await.unwrap(),
            AuthResult::Unauthenticated
        ));
    }

    #[tokio::test]
    async fn host_from_header_resolves_namespace() {
        // No authority in the URI (origin-form) → host comes from the Host header.
        let p = provider();
        let mut headers = http::HeaderMap::new();
        headers.insert(
            http::header::HOST,
            "customer-a.twinos.cloud".parse().unwrap(),
        );
        let method = http::Method::GET;
        let uri: http::Uri = "/api/orders".parse().unwrap();
        let req = AuthRequest {
            headers: &headers,
            method: &method,
            uri: &uri,
            tls_client_cn: None,
        };
        // No token present → the per-namespace provider reports Unauthenticated
        // (never reaching the network), proving the host resolved into the domain.
        assert!(matches!(
            p.authenticate(&req).await.unwrap(),
            AuthResult::Unauthenticated
        ));
    }

    #[tokio::test]
    async fn script_derived_namespace_is_used_for_auth() {
        // The convention's Rhai script maps the host to a namespace the label
        // layout would NOT produce, proving auth uses the same script-aware
        // resolution as the router.
        let mut convention = tenants_convention();
        convention.script = Some(r#"#{ namespace: "scripted-ns", service: "api" }"#.into());
        let p = provider_with(convention);
        assert_eq!(
            p.resolve_namespace("customer-a.twinos.cloud")
                .await
                .as_deref(),
            Some("scripted-ns")
        );
    }

    #[tokio::test]
    async fn script_decline_falls_back_to_label_namespace() {
        // A script that returns `()` declines → fall back to the label layout.
        let mut convention = tenants_convention();
        convention.script = Some("()".into());
        let p = provider_with(convention);
        assert_eq!(
            p.resolve_namespace("customer-a.twinos.cloud")
                .await
                .as_deref(),
            Some("customer-a")
        );
    }
}
