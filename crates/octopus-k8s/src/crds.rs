//! Custom resource definitions for Octopus.
//!
//! Group `gateway.octopus.io`, version `v1alpha1`. These expose Octopus-native
//! features (FARP, scripting, plugin chain, auth providers) that don't map onto
//! the standard Gateway API. [`OctopusPolicy`] is a GEP-713 policy attachment
//! that bolts those extras onto a standard Gateway API resource via `target_ref`.

use crate::status::OctopusStatus;
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A token-bucket rate limit.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitSpec {
    /// Requests allowed per window.
    pub requests: u32,
    /// Window length in seconds.
    pub window_seconds: u64,
}

/// A weighted upstream target.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpstreamTarget {
    /// Host or IP.
    pub host: String,
    /// Port.
    pub port: u16,
    /// Relative weight for weighted load balancing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<u32>,
}

/// GEP-713 policy attachment target (a Gateway API or Octopus resource).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PolicyTargetRef {
    /// API group of the target (e.g. `gateway.networking.k8s.io`).
    pub group: String,
    /// Kind of the target (e.g. `HTTPRoute`, `Gateway`).
    pub kind: String,
    /// Name of the target resource.
    pub name: String,
    /// Optional section (e.g. a specific listener or route rule).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section_name: Option<String>,
}

/// Gateway-level configuration: a logical Octopus gateway instance.
#[derive(CustomResource, Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "gateway.octopus.io",
    version = "v1alpha1",
    kind = "OctopusGateway",
    namespaced,
    status = "OctopusStatus",
    shortname = "ogw"
)]
#[serde(rename_all = "camelCase")]
pub struct OctopusGatewaySpec {
    /// Listen address, e.g. `0.0.0.0:8080`.
    pub listen: String,
    /// The GatewayClass this gateway serves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gateway_class_name: Option<String>,
    /// Default auth provider applied to attached routes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_auth_provider: Option<String>,
}

/// A full-fidelity Octopus route.
#[derive(CustomResource, Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "gateway.octopus.io",
    version = "v1alpha1",
    kind = "OctopusRoute",
    namespaced,
    status = "OctopusStatus",
    shortname = "ort"
)]
#[serde(rename_all = "camelCase")]
pub struct OctopusRouteSpec {
    /// Names of `OctopusGateway`/`Gateway` resources this route attaches to.
    #[serde(default)]
    pub parent_refs: Vec<String>,
    /// Match path (supports `:param` and `*wildcard`).
    pub path: String,
    /// HTTP methods this route matches (empty = all).
    #[serde(default)]
    pub methods: Vec<String>,
    /// Target upstream cluster (an `OctopusUpstream` name or discovered service).
    pub upstream: String,
    /// Explicit priority; higher wins on identical path collisions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<i32>,
    /// Strip this prefix before proxying.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strip_prefix: Option<String>,
    /// Add this prefix before proxying.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub add_prefix: Option<String>,
    /// Named auth provider to enforce.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_provider: Option<String>,
    /// Skip authentication for this route.
    #[serde(default)]
    pub skip_auth: bool,
    /// Required roles.
    #[serde(default)]
    pub require_roles: Vec<String>,
    /// Required scopes.
    #[serde(default)]
    pub require_scopes: Vec<String>,
    /// Authorization rule expression.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authz_rule: Option<String>,
    /// Per-route timeout in seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u64>,
    /// Per-route rate limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<RateLimitSpec>,
    /// Ordered plugin chain (plugin ids).
    #[serde(default)]
    pub plugins: Vec<String>,
    /// Reference to a Rhai script (e.g. a ConfigMap name).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script_ref: Option<String>,
    /// Convention for deriving the backend from the request host (multi-tenant
    /// subdomain routing). When set, this route becomes a single wildcard route
    /// (`*.<baseDomain>`) whose upstream is derived per request from the host.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub convention: Option<ConventionSpec>,
}

/// Host-to-backend convention: derive `{namespace, service}` from the request
/// host instead of declaring a route per tenant.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConventionSpec {
    /// Base domain, e.g. `platform.com`. The route matches `*.<baseDomain>`.
    pub base_domain: String,
    /// Label roles for the tenant prefix, left to right. Recognized values:
    /// `service`, `namespace` (alias `tenant`), `ignore`. For example
    /// `["service","namespace"]` maps `orders.acme.platform.com` to Service
    /// `orders` in namespace `acme`.
    pub layout: Vec<String>,
    /// Service name to use when `layout` has no `service` entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_service: Option<String>,
    /// Upstream port for the derived Service (default 80).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// Optional inline Rhai script mapping `host` → `#{namespace, service[, port]}`,
    /// overriding the label `layout` when it returns a mapping (otherwise the
    /// layout is used). Receives the request `host` as a string variable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,
    /// Load the host-resolution Rhai script from a ConfigMap (ignored when an
    /// inline `script` is set). Cross-namespace refs require a `ReferenceGrant`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script_ref: Option<ScriptConfigMapRef>,
    /// Backend load-balancing: `ServiceDNS` (default) routes to the cluster
    /// Service DNS name; `EndpointSlice` watches the Service's EndpointSlices and
    /// balances across pod IPs directly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend_strategy: Option<String>,
}

/// Reference to a key in a ConfigMap holding a Rhai host-resolution script.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScriptConfigMapRef {
    /// ConfigMap name.
    pub name: String,
    /// Data key within the ConfigMap.
    pub key: String,
    /// ConfigMap namespace (defaults to the route's namespace).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
}

/// An upstream cluster with explicit targets and load-balancing policy.
#[derive(CustomResource, Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "gateway.octopus.io",
    version = "v1alpha1",
    kind = "OctopusUpstream",
    namespaced,
    status = "OctopusStatus",
    shortname = "oup"
)]
#[serde(rename_all = "camelCase")]
pub struct OctopusUpstreamSpec {
    /// Explicit upstream targets.
    #[serde(default)]
    pub targets: Vec<UpstreamTarget>,
    /// Load-balancing strategy (e.g. `round_robin`, `least_conn`, `ip_hash`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lb_strategy: Option<String>,
}

/// A GEP-713 policy attaching Octopus-specific behavior onto a target resource
/// (typically a standard Gateway API `HTTPRoute`/`Gateway`).
#[derive(CustomResource, Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "gateway.octopus.io",
    version = "v1alpha1",
    kind = "OctopusPolicy",
    namespaced,
    status = "OctopusStatus",
    shortname = "opol"
)]
#[serde(rename_all = "camelCase")]
pub struct OctopusPolicySpec {
    /// The resource this policy attaches to.
    pub target_ref: PolicyTargetRef,
    /// Auth provider to enforce on the target's routes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_provider: Option<String>,
    /// Ordered plugin chain to apply.
    #[serde(default)]
    pub plugins: Vec<String>,
    /// Rate limit to apply.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<RateLimitSpec>,
    /// Reference to a Rhai script.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script_ref: Option<String>,
    /// Override semantics (true = override route-inline settings; default = fill gaps).
    #[serde(default)]
    pub override_route: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use kube::CustomResourceExt;

    #[test]
    fn octopus_route_crd_metadata() {
        let crd = OctopusRoute::crd();
        assert_eq!(crd.spec.group, "gateway.octopus.io");
        assert_eq!(crd.spec.names.kind, "OctopusRoute");
        assert!(crd.spec.versions.iter().any(|v| v.name == "v1alpha1"));
    }

    #[test]
    fn all_four_kinds_present() {
        assert_eq!(OctopusGateway::crd().spec.names.kind, "OctopusGateway");
        assert_eq!(OctopusUpstream::crd().spec.names.kind, "OctopusUpstream");
        assert_eq!(OctopusPolicy::crd().spec.names.kind, "OctopusPolicy");
    }

    #[test]
    fn all_octopus_crds_have_status_subresource() {
        let crds = [
            OctopusGateway::crd(),
            OctopusRoute::crd(),
            OctopusUpstream::crd(),
            OctopusPolicy::crd(),
        ];
        for crd in crds {
            let v = crd
                .spec
                .versions
                .iter()
                .find(|v| v.name == "v1alpha1")
                .expect("v1alpha1 version present");
            assert!(
                v.subresources
                    .as_ref()
                    .and_then(|s| s.status.as_ref())
                    .is_some(),
                "{} must declare a status subresource",
                crd.spec.names.kind
            );
        }
    }

    #[test]
    fn octopus_route_spec_round_trips() {
        let spec = OctopusRouteSpec {
            parent_refs: vec!["my-gw".into()],
            path: "/api".into(),
            methods: vec!["GET".into()],
            upstream: "my-upstream".into(),
            priority: Some(5),
            ..Default::default()
        };
        let json = serde_json::to_string(&spec).unwrap();
        let back: OctopusRouteSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(back.path, "/api");
        assert_eq!(back.upstream, "my-upstream");
        assert_eq!(back.priority, Some(5));
    }

    #[test]
    fn policy_targets_a_gateway_api_resource() {
        let spec = OctopusPolicySpec {
            target_ref: PolicyTargetRef {
                group: "gateway.networking.k8s.io".into(),
                kind: "HTTPRoute".into(),
                name: "my-route".into(),
                section_name: None,
            },
            auth_provider: Some("jwt".into()),
            ..Default::default()
        };
        let back: OctopusPolicySpec =
            serde_json::from_str(&serde_json::to_string(&spec).unwrap()).unwrap();
        assert_eq!(back.target_ref.kind, "HTTPRoute");
        assert_eq!(back.auth_provider.as_deref(), Some("jwt"));
    }
}
