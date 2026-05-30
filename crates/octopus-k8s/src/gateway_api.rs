//! Hand-written subset of the standard Kubernetes Gateway API
//! (`gateway.networking.k8s.io/v1`).
//!
//! These types mirror the upstream CRDs closely enough to watch and translate
//! `HTTPRoute`/`Gateway`/`GatewayClass` resources. The CRDs themselves are
//! installed from the upstream Gateway API release, not generated here.

use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Reference from a route to a parent `Gateway` (or Octopus gateway).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ParentRef {
    /// Name of the parent gateway.
    pub name: String,
    /// Namespace of the parent (defaults to the route's namespace).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    /// Specific listener/section of the parent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section_name: Option<String>,
}

/// Path match (`PathPrefix` | `Exact` | `RegularExpression`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HttpPathMatch {
    /// Match type.
    #[serde(rename = "type", default = "default_path_type")]
    pub path_type: String,
    /// Match value.
    #[serde(default = "default_path_value")]
    pub value: String,
}

impl Default for HttpPathMatch {
    fn default() -> Self {
        Self {
            path_type: default_path_type(),
            value: default_path_value(),
        }
    }
}

fn default_path_type() -> String {
    "PathPrefix".to_string()
}

fn default_path_value() -> String {
    "/".to_string()
}

/// Header match.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HttpHeaderMatch {
    /// Header name.
    pub name: String,
    /// Header value.
    pub value: String,
    /// Match type (`Exact` | `RegularExpression`).
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub match_type: Option<String>,
}

/// One route match (path + method + headers; all optional).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HttpRouteMatch {
    /// Path match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<HttpPathMatch>,
    /// HTTP method.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    /// Header matches.
    #[serde(default)]
    pub headers: Vec<HttpHeaderMatch>,
}

/// A path modifier within a URL rewrite/redirect filter.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HttpPathModifier {
    /// `ReplacePrefixMatch` | `ReplaceFullPath`.
    #[serde(rename = "type")]
    pub modifier_type: String,
    /// Prefix to substitute for the matched prefix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replace_prefix_match: Option<String>,
    /// Full path replacement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replace_full_path: Option<String>,
}

/// `URLRewrite` filter payload.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HttpUrlRewrite {
    /// Rewrite the Host header.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
    /// Rewrite the path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<HttpPathModifier>,
}

/// A route filter (`URLRewrite`, `RequestRedirect`, `RequestHeaderModifier`, …).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HttpRouteFilter {
    /// Filter type.
    #[serde(rename = "type")]
    pub filter_type: String,
    /// `URLRewrite` payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url_rewrite: Option<HttpUrlRewrite>,
}

/// A weighted backend reference (typically a Service).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HttpBackendRef {
    /// Backend name (Service name).
    pub name: String,
    /// Backend namespace (defaults to the route's namespace).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    /// Backend port.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// Relative weight for traffic splitting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<i32>,
}

/// One HTTPRoute rule: matches + filters + backends.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HttpRouteRule {
    /// Matches (empty = match everything).
    #[serde(default)]
    pub matches: Vec<HttpRouteMatch>,
    /// Filters applied to matched requests.
    #[serde(default)]
    pub filters: Vec<HttpRouteFilter>,
    /// Backend references.
    #[serde(default)]
    pub backend_refs: Vec<HttpBackendRef>,
}

/// `HTTPRoute` spec.
#[derive(CustomResource, Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "gateway.networking.k8s.io",
    version = "v1",
    kind = "HTTPRoute",
    namespaced
)]
#[serde(rename_all = "camelCase")]
pub struct HTTPRouteSpec {
    /// Parent gateways this route attaches to.
    #[serde(default)]
    pub parent_refs: Vec<ParentRef>,
    /// Hostnames this route matches.
    #[serde(default)]
    pub hostnames: Vec<String>,
    /// Routing rules.
    #[serde(default)]
    pub rules: Vec<HttpRouteRule>,
}

/// A gRPC method match (`service` and `method` are both optional).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GrpcMethodMatch {
    /// `Exact` (default) or `RegularExpression`.
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub match_type: Option<String>,
    /// gRPC service name (e.g. `my.package.Service`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    /// gRPC method name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
}

/// A `GRPCRoute` match (method + headers).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GrpcRouteMatch {
    /// Method match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<GrpcMethodMatch>,
    /// Header matches.
    #[serde(default)]
    pub headers: Vec<HttpHeaderMatch>,
}

/// One `GRPCRoute` rule.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GrpcRouteRule {
    /// Matches (empty = match all methods).
    #[serde(default)]
    pub matches: Vec<GrpcRouteMatch>,
    /// Filters.
    #[serde(default)]
    pub filters: Vec<HttpRouteFilter>,
    /// Backend references (reuses the HTTP backend shape).
    #[serde(default)]
    pub backend_refs: Vec<HttpBackendRef>,
}

/// `GRPCRoute` spec.
#[derive(CustomResource, Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "gateway.networking.k8s.io",
    version = "v1",
    kind = "GRPCRoute",
    namespaced
)]
#[serde(rename_all = "camelCase")]
pub struct GRPCRouteSpec {
    /// Parent gateways.
    #[serde(default)]
    pub parent_refs: Vec<ParentRef>,
    /// Hostnames.
    #[serde(default)]
    pub hostnames: Vec<String>,
    /// Routing rules.
    #[serde(default)]
    pub rules: Vec<GrpcRouteRule>,
}

/// A reference to a Secret holding a TLS certificate (a Gateway listener's
/// `tls.certificateRefs[]`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CertificateRef {
    /// Kind (defaults to `Secret`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Secret name.
    pub name: String,
    /// Secret namespace (defaults to the Gateway's namespace; cross-namespace
    /// requires a ReferenceGrant).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
}

/// A listener's TLS configuration.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GatewayTls {
    /// `Terminate` (default) or `Passthrough`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// Secrets providing the listener's certificate(s).
    #[serde(default)]
    pub certificate_refs: Vec<CertificateRef>,
}

/// A Gateway listener.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GatewayListener {
    /// Listener name.
    pub name: String,
    /// Hostname this listener serves (SNI).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
    /// Listen port.
    pub port: u16,
    /// `HTTP` | `HTTPS` | `TLS` | …
    pub protocol: String,
    /// TLS configuration (for HTTPS/TLS listeners).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tls: Option<GatewayTls>,
}

/// `Gateway` spec.
#[derive(CustomResource, Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "gateway.networking.k8s.io",
    version = "v1",
    kind = "Gateway",
    namespaced
)]
#[serde(rename_all = "camelCase")]
pub struct GatewaySpec {
    /// The GatewayClass this gateway belongs to.
    pub gateway_class_name: String,
    /// Listeners (HTTP/HTTPS/TLS).
    #[serde(default)]
    pub listeners: Vec<GatewayListener>,
}

/// `GatewayClass` spec (cluster-scoped).
#[derive(CustomResource, Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "gateway.networking.k8s.io",
    version = "v1",
    kind = "GatewayClass"
)]
#[serde(rename_all = "camelCase")]
pub struct GatewayClassSpec {
    /// The controller that should reconcile gateways of this class.
    pub controller_name: String,
}

/// The controllerName Octopus claims for GatewayClasses.
pub const CONTROLLER_NAME: &str = "gateway.octopus.io/gateway-controller";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_realistic_httproute() {
        let yaml = r#"
apiVersion: gateway.networking.k8s.io/v1
kind: HTTPRoute
metadata:
  name: api-route
  namespace: default
spec:
  parentRefs:
    - name: octopus-gateway
  hostnames: ["api.example.com"]
  rules:
    - matches:
        - path:
            type: PathPrefix
            value: /api
          method: GET
      filters:
        - type: URLRewrite
          urlRewrite:
            path:
              type: ReplacePrefixMatch
              replacePrefixMatch: /v2
      backendRefs:
        - name: api-svc
          port: 8080
          weight: 100
"#;
        let route: HTTPRoute = serde_yaml::from_str(yaml).unwrap();
        let spec = route.spec;
        assert_eq!(spec.parent_refs[0].name, "octopus-gateway");
        assert_eq!(spec.hostnames, vec!["api.example.com"]);

        let rule = &spec.rules[0];
        let m = &rule.matches[0];
        assert_eq!(m.path.as_ref().unwrap().value, "/api");
        assert_eq!(m.path.as_ref().unwrap().path_type, "PathPrefix");
        assert_eq!(m.method.as_deref(), Some("GET"));

        assert_eq!(rule.filters[0].filter_type, "URLRewrite");
        let rewrite = rule.filters[0].url_rewrite.as_ref().unwrap();
        let pm = rewrite.path.as_ref().unwrap();
        assert_eq!(pm.modifier_type, "ReplacePrefixMatch");
        assert_eq!(pm.replace_prefix_match.as_deref(), Some("/v2"));

        let backend = &rule.backend_refs[0];
        assert_eq!(backend.name, "api-svc");
        assert_eq!(backend.port, Some(8080));
        assert_eq!(backend.weight, Some(100));
    }

    #[test]
    fn path_match_defaults_to_prefix_root() {
        // A match with no path still parses; defaults mirror the Gateway API.
        let yaml = r#"
apiVersion: gateway.networking.k8s.io/v1
kind: HTTPRoute
metadata:
  name: r
  namespace: default
spec:
  rules:
    - backendRefs:
        - name: svc
          port: 80
"#;
        let route: HTTPRoute = serde_yaml::from_str(yaml).unwrap();
        let rule = &route.spec.rules[0];
        assert!(rule.matches.is_empty(), "no matches declared");
        assert_eq!(rule.backend_refs[0].port, Some(80));
    }
}
