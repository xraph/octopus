//! Translate Gateway API resources into the intermediate routing representation.

use crate::crds::{ConventionSpec, OctopusGatewaySpec, OctopusRouteSpec, OctopusUpstreamSpec};
use crate::gateway_api::{GRPCRouteSpec, GrpcMethodMatch, HTTPRouteSpec, HttpBackendRef};
use crate::ir::{IntermediateRoute, RateLimit, RouteSource};
use http::Method;
use octopus_core::types::LoadBalanceStrategy;
use octopus_core::{UpstreamCluster, UpstreamInstance};
use octopus_router::{
    BackendStrategy, Convention, ConventionRouteRule, GatewayEntry, GatewayPolicy, HostMatch,
    LabelRole, RouteCorsOverride,
};
use std::time::Duration;

/// Wildcard parameter name used to emulate Gateway API `PathPrefix` semantics
/// (the segment-based router matches a registered prefix exactly, so subpaths
/// need a trailing wildcard route).
const PREFIX_WILDCARD: &str = "octopus_prefix";

/// HTTP methods a match with no explicit `method` expands to (Gateway API
/// treats an unset method as "all methods").
const DEFAULT_METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

/// Build a ProxySpec from `octopus.io/*` HTTPRoute annotations. `None` when none present.
pub fn proxy_spec_from_annotations(
    ann: &std::collections::BTreeMap<String, String>,
) -> Option<octopus_router::ProxySpec> {
    let get = |k: &str| ann.get(k).map(|s| s.as_str());
    let any = [
        "octopus.io/path-mode",
        "octopus.io/upstream-origin",
        "octopus.io/rewrite-redirects",
        "octopus.io/rewrite-cookie-path",
    ]
    .iter()
    .any(|k| ann.contains_key(*k));
    if !any {
        return None;
    }
    let tls_verify = get("octopus.io/tls-verify")
        .map(|v| v != "false")
        .unwrap_or(true);
    let origin = get("octopus.io/upstream-origin")
        .and_then(|u| octopus_router::UpstreamOrigin::parse(u, tls_verify));
    Some(octopus_router::ProxySpec {
        origin,
        path_mode: match get("octopus.io/path-mode") {
            Some("passthrough") => octopus_router::PathMode::Passthrough,
            Some("strip") | None => octopus_router::PathMode::Strip,
            Some(other) => {
                tracing::warn!(path_mode = %other, "unrecognized path-mode; defaulting to strip");
                octopus_router::PathMode::Strip
            }
        },
        rewrite_redirects: get("octopus.io/rewrite-redirects") == Some("true"),
        rewrite_cookie_path: get("octopus.io/rewrite-cookie-path") == Some("true"),
    })
}

/// Translate an `HTTPRoute` into intermediate routes and the upstream clusters
/// they target. `name`/`namespace` are the route's own identity;
/// `listener_hostnames` is the union of the parent Gateway listener hostnames the
/// route attaches to (empty = unconstrained), intersected with the route's own.
/// `annotations` are the HTTPRoute's metadata annotations, used to populate
/// proxy-mode fields via `octopus.io/*` keys.
pub fn httproute_to_route(
    name: &str,
    namespace: &str,
    spec: &HTTPRouteSpec,
    listener_hostnames: &[String],
    annotations: &std::collections::BTreeMap<String, String>,
) -> (Vec<IntermediateRoute>, Vec<UpstreamCluster>) {
    let mut routes = Vec::new();
    let mut upstreams = Vec::new();
    // Effective hostnames = route hostnames ∩ parent listener hostnames.
    let hosts = intersect_hostnames(&spec.hostnames, listener_hostnames);
    // Proxy spec derived from annotations — None for routes with no octopus.io/* keys.
    let proxy = proxy_spec_from_annotations(annotations);

    for (idx, rule) in spec.rules.iter().enumerate() {
        if rule.backend_refs.is_empty() {
            tracing::warn!(route = %name, rule = idx, "HTTPRoute rule has no backendRefs; skipping");
            continue;
        }

        // One upstream cluster per rule (instances = its backends).
        let cluster_name = format!("{namespace}/{name}-r{idx}");
        let cluster = build_cluster(&cluster_name, namespace, &rule.backend_refs);
        upstreams.push(cluster);

        // ReplacePrefixMatch URL rewrite → strip the matched prefix, add the new one.
        let replace_prefix = rule.filters.iter().find_map(|f| {
            if f.filter_type == "URLRewrite" {
                f.url_rewrite
                    .as_ref()
                    .and_then(|rw| rw.path.as_ref())
                    .filter(|p| p.modifier_type == "ReplacePrefixMatch")
                    .and_then(|p| p.replace_prefix_match.clone())
            } else {
                None
            }
        });

        // Empty matches = match everything (PathPrefix "/").
        let matches = if rule.matches.is_empty() {
            vec![Default::default()]
        } else {
            rule.matches.clone()
        };

        for m in &matches {
            let (path_type, path_value) = match &m.path {
                Some(p) => (p.path_type.as_str(), p.value.clone()),
                None => ("PathPrefix", "/".to_string()),
            };
            let paths = expand_paths(path_type, &path_value);
            if paths.is_empty() {
                tracing::warn!(route = %name, path_type, "Unsupported path match type; skipping");
                continue;
            }

            let methods = match &m.method {
                Some(method) => vec![method.clone()],
                None => DEFAULT_METHODS.iter().map(|m| m.to_string()).collect(),
            };

            for path in &paths {
                for method_str in &methods {
                    let Ok(method) = method_str.parse::<Method>() else {
                        tracing::warn!(method = %method_str, "Invalid HTTP method; skipping");
                        continue;
                    };
                    for host in &hosts {
                        let mut route = IntermediateRoute::new(
                            method.clone(),
                            path.clone(),
                            &cluster_name,
                            RouteSource::GatewayApi,
                        );
                        route.host = host.clone();
                        route.source_id = format!("{namespace}/{name}");
                        if replace_prefix.is_some() {
                            route.strip_prefix = Some(path_value.clone());
                            route.add_prefix = replace_prefix.clone();
                        }
                        route.proxy = proxy.clone();
                        routes.push(route);
                    }
                }
            }
        }
    }

    (routes, upstreams)
}

/// Build a router [`Convention`] from a CRD [`ConventionSpec`].
fn convention_from_spec(spec: &ConventionSpec) -> Convention {
    let roles = spec
        .layout
        .iter()
        .map(|r| match r.as_str() {
            "service" => LabelRole::Service,
            "namespace" | "tenant" => LabelRole::Namespace,
            _ => LabelRole::Ignore,
        })
        .collect();
    let backend = match spec.backend_strategy.as_deref() {
        Some(s) if s.eq_ignore_ascii_case("endpointslice") => BackendStrategy::EndpointSlice,
        _ => BackendStrategy::ServiceDns,
    };
    Convention {
        base_suffix: format!(".{}", spec.base_domain.trim_matches('.')),
        roles,
        default_service: spec.default_service.clone(),
        port: spec.port.unwrap_or(80),
        script: spec.script.clone(),
        backend,
        route_rules: spec.route_rules.iter().map(convention_route_rule).collect(),
    }
}

/// Translate a CRD [`ConventionRouteRuleSpec`] into the router's
/// [`ConventionRouteRule`].
fn convention_route_rule(spec: &crate::crds::ConventionRouteRuleSpec) -> ConventionRouteRule {
    ConventionRouteRule {
        path_prefix: spec.path_prefix.clone(),
        strip_prefix: spec.strip_prefix,
        service_override: spec.service_override.clone(),
        port_override: spec.port_override,
        add_prefix: spec.add_prefix.clone(),
    }
}

/// Gateway API hostname overlap of two hostnames (exact or wildcard). Returns the
/// more specific hostname when they intersect, else `None`. `*.x` matches strict
/// subdomains of `x` (not the apex `x`).
fn host_overlap(a: &str, b: &str) -> Option<String> {
    if a == b {
        return Some(a.to_string());
    }
    match (a.strip_prefix("*."), b.strip_prefix("*.")) {
        // Both wildcards: the one whose suffix is a subdomain of the other wins.
        (Some(asfx), Some(bsfx)) => {
            if asfx.ends_with(&format!(".{bsfx}")) {
                Some(a.to_string())
            } else if bsfx.ends_with(&format!(".{asfx}")) {
                Some(b.to_string())
            } else {
                None
            }
        }
        // a is a wildcard, b exact: keep b if it is a strict subdomain of a's suffix.
        (Some(asfx), None) => b.ends_with(&format!(".{asfx}")).then(|| b.to_string()),
        (None, Some(bsfx)) => a.ends_with(&format!(".{bsfx}")).then(|| a.to_string()),
        // Both exact and unequal → no overlap.
        (None, None) => None,
    }
}

/// Effective host matchers for a route: the Gateway-API intersection of the
/// route's `hostnames` with its parent listener `hostnames`.
///
/// - both empty → match any host;
/// - one empty → the other (the non-empty side constrains);
/// - both set → pairwise intersection (empty result = no attachment → no routes).
fn intersect_hostnames(route_hosts: &[String], listener_hosts: &[String]) -> Vec<HostMatch> {
    match (route_hosts.is_empty(), listener_hosts.is_empty()) {
        (true, true) => vec![HostMatch::Any],
        (true, false) => listener_hosts.iter().map(|h| HostMatch::parse(h)).collect(),
        (false, true) => route_hosts.iter().map(|h| HostMatch::parse(h)).collect(),
        (false, false) => {
            let mut out: Vec<HostMatch> = Vec::new();
            for r in route_hosts {
                for l in listener_hosts {
                    if let Some(h) = host_overlap(r, l) {
                        let m = HostMatch::parse(&h);
                        if !out.contains(&m) {
                            out.push(m);
                        }
                    }
                }
            }
            out
        }
    }
}

/// Build an upstream cluster from a set of backend references.
fn build_cluster(
    cluster_name: &str,
    route_namespace: &str,
    backends: &[HttpBackendRef],
) -> UpstreamCluster {
    let mut cluster = UpstreamCluster::new(cluster_name);
    if backends.len() > 1 {
        cluster.strategy = LoadBalanceStrategy::WeightedRoundRobin;
    }
    for backend in backends {
        let backend_ns = backend.namespace.as_deref().unwrap_or(route_namespace);
        let host = format!("{}.{}.svc", backend.name, backend_ns);
        let port = backend.port.unwrap_or(80);
        let mut instance = UpstreamInstance::new(format!("{host}:{port}"), &host, port);
        instance.weight = backend.weight.unwrap_or(1).max(0) as u32;
        cluster.add_instance(instance);
    }
    cluster
}

/// Expand a Gateway API path match into router path patterns.
///
/// - `Exact` → the path itself.
/// - `PathPrefix` → the path plus a trailing wildcard so subpaths match too.
/// - anything else (e.g. `RegularExpression`) → unsupported (empty).
fn expand_paths(path_type: &str, value: &str) -> Vec<String> {
    match path_type {
        "Exact" => vec![value.to_string()],
        "PathPrefix" => {
            let base = value.trim_end_matches('/');
            let wildcard = if base.is_empty() {
                format!("/*{PREFIX_WILDCARD}")
            } else {
                format!("{base}/*{PREFIX_WILDCARD}")
            };
            let exact = if base.is_empty() {
                "/".to_string()
            } else {
                base.to_string()
            };
            vec![exact, wildcard]
        }
        _ => Vec::new(),
    }
}

/// Build a [`octopus_router::ProxySpec`] from the route's proxy fields, or
/// `None` when none are set (so legacy routes are unaffected).
fn octopus_route_proxy_spec(spec: &OctopusRouteSpec) -> Option<octopus_router::ProxySpec> {
    let any = spec.path_mode.is_some()
        || spec.upstream_origin.is_some()
        || spec.rewrite_redirects.is_some()
        || spec.rewrite_cookie_path.is_some();
    if !any {
        return None;
    }
    let tls_verify = spec.tls_verify.unwrap_or(true);
    let origin = spec
        .upstream_origin
        .as_deref()
        .and_then(|u| octopus_router::UpstreamOrigin::parse(u, tls_verify));
    Some(octopus_router::ProxySpec {
        origin,
        path_mode: match spec.path_mode.as_deref() {
            Some("passthrough") => octopus_router::PathMode::Passthrough,
            Some("strip") | None => octopus_router::PathMode::Strip,
            Some(other) => {
                tracing::warn!(path_mode = %other, "unrecognized path-mode; defaulting to strip");
                octopus_router::PathMode::Strip
            }
        },
        rewrite_redirects: spec.rewrite_redirects.unwrap_or(false),
        rewrite_cookie_path: spec.rewrite_cookie_path.unwrap_or(false),
    })
}

/// Translate an `OctopusRoute` into intermediate routes. Unlike Gateway API
/// routes, the path is used as-is (it is already a native router pattern) and
/// the upstream is referenced by name (resolved from an `OctopusUpstream` or
/// discovery), so no upstream cluster is produced here.
pub fn octopus_route_to_route(
    name: &str,
    namespace: &str,
    spec: &OctopusRouteSpec,
) -> Vec<IntermediateRoute> {
    let source_id = format!("{namespace}/{name}");
    let methods: Vec<String> = if spec.methods.is_empty() {
        DEFAULT_METHODS.iter().map(|m| m.to_string()).collect()
    } else {
        spec.methods.clone()
    };
    // A convention turns this route into one wildcard-host route whose upstream
    // is derived per request from the host.
    let convention = spec.convention.as_ref().map(convention_from_spec);

    let mut routes = Vec::new();
    for method_str in &methods {
        let Ok(method) = method_str.parse::<Method>() else {
            tracing::warn!(route = %name, method = %method_str, "Invalid HTTP method; skipping");
            continue;
        };
        let mut route = IntermediateRoute::new(
            method,
            &spec.path,
            &spec.upstream,
            RouteSource::OctopusRoute,
        );
        route.source_id = source_id.clone();
        route.priority = spec.priority.unwrap_or(0);
        route.strip_prefix = spec.strip_prefix.clone();
        route.add_prefix = spec.add_prefix.clone();
        route.proxy = octopus_route_proxy_spec(spec);
        route.auth_provider = spec.auth_provider.clone();
        route.skip_auth = spec.skip_auth;
        route.require_roles = spec.require_roles.clone();
        route.require_scopes = spec.require_scopes.clone();
        route.authz_rule = spec.authz_rule.clone();
        route.timeout = spec.timeout_seconds.map(Duration::from_secs);
        route.rate_limit = spec.rate_limit.as_ref().map(|rl| RateLimit {
            requests: rl.requests,
            window: Duration::from_secs(rl.window_seconds),
        });
        if let Some(conv) = &convention {
            route.host = HostMatch::Wildcard(conv.base_suffix.clone());
            route.convention = Some(conv.clone());
        }
        routes.push(route);
    }
    routes
}

/// Translate an `OctopusUpstream` into an upstream cluster named after the
/// resource (so `OctopusRoute.upstream` can reference it by name).
pub fn octopus_upstream_to_cluster(
    name: &str,
    _namespace: &str,
    spec: &OctopusUpstreamSpec,
) -> UpstreamCluster {
    let mut cluster = UpstreamCluster::new(name);
    cluster.strategy = parse_lb_strategy(spec.lb_strategy.as_deref());
    for target in &spec.targets {
        let mut instance = UpstreamInstance::new(
            format!("{}:{}", target.host, target.port),
            &target.host,
            target.port,
        );
        instance.weight = target.weight.unwrap_or(1);
        cluster.add_instance(instance);
    }
    cluster
}

/// Translate an `OctopusGateway` into a [`GatewayEntry`] for the virtual-gateway
/// index: its `hostnames` become the domain set (empty → match-any) and its
/// `defaultPolicy` becomes the inherited [`GatewayPolicy`].
pub fn octopus_gateway_to_entry(name: &str, spec: &OctopusGatewaySpec) -> GatewayEntry {
    let domains = if spec.hostnames.is_empty() {
        vec![HostMatch::Any]
    } else {
        spec.hostnames.iter().map(|h| HostMatch::parse(h)).collect()
    };

    let dp = spec.default_policy.as_ref();
    let policy = GatewayPolicy {
        // defaultPolicy.authProvider supersedes the legacy default_auth_provider.
        auth_provider: dp
            .and_then(|p| p.auth_provider.clone())
            .or_else(|| spec.default_auth_provider.clone()),
        base_path_prefix: None,
        cors: dp.and_then(|p| p.cors.as_ref()).map(|c| RouteCorsOverride {
            allowed_origins: c.allowed_origins.clone(),
            allowed_methods: c.allowed_methods.clone(),
            allowed_headers: c.allowed_headers.clone(),
            allow_credentials: c.allow_credentials,
            max_age: c.max_age,
        }),
        rate_limit: dp.and_then(|p| {
            p.rate_limit
                .as_ref()
                .map(|rl| (rl.requests, Duration::from_secs(rl.window_seconds)))
        }),
        timeout: dp.and_then(|p| p.timeout_seconds).map(Duration::from_secs),
    };

    GatewayEntry {
        id: name.into(),
        domains,
        policy,
    }
}

/// Translate a `GRPCRoute` into intermediate routes + upstream clusters. gRPC
/// requests are HTTP/2 POSTs to `/{service}/{method}`.
pub fn grpcroute_to_route(
    name: &str,
    namespace: &str,
    spec: &GRPCRouteSpec,
    listener_hostnames: &[String],
) -> (Vec<IntermediateRoute>, Vec<UpstreamCluster>) {
    let mut routes = Vec::new();
    let mut upstreams = Vec::new();
    let source_id = format!("{namespace}/{name}");
    let hosts = intersect_hostnames(&spec.hostnames, listener_hostnames);

    for (idx, rule) in spec.rules.iter().enumerate() {
        if rule.backend_refs.is_empty() {
            tracing::warn!(route = %name, rule = idx, "GRPCRoute rule has no backendRefs; skipping");
            continue;
        }
        let cluster_name = format!("{namespace}/{name}-grpc-r{idx}");
        upstreams.push(build_cluster(&cluster_name, namespace, &rule.backend_refs));

        let matches = if rule.matches.is_empty() {
            vec![Default::default()]
        } else {
            rule.matches.clone()
        };
        for m in &matches {
            for host in &hosts {
                let mut route = IntermediateRoute::new(
                    Method::POST,
                    grpc_path(m.method.as_ref()),
                    &cluster_name,
                    RouteSource::GatewayApi,
                );
                route.host = host.clone();
                route.source_id = source_id.clone();
                routes.push(route);
            }
        }
    }

    (routes, upstreams)
}

/// Build the router path for a gRPC method match.
fn grpc_path(method: Option<&GrpcMethodMatch>) -> String {
    match method {
        Some(m) => match (&m.service, &m.method) {
            (Some(service), Some(method)) => format!("/{service}/{method}"),
            (Some(service), None) => format!("/{service}/*{PREFIX_WILDCARD}"),
            _ => format!("/*{PREFIX_WILDCARD}"),
        },
        None => format!("/*{PREFIX_WILDCARD}"),
    }
}

/// Map an `OctopusUpstream.lbStrategy` string to a [`LoadBalanceStrategy`].
fn parse_lb_strategy(s: Option<&str>) -> LoadBalanceStrategy {
    match s.unwrap_or("round_robin") {
        "least_conn" | "least_connections" => LoadBalanceStrategy::LeastConnections,
        "weighted" | "weighted_round_robin" => LoadBalanceStrategy::WeightedRoundRobin,
        "random" => LoadBalanceStrategy::Random,
        "ip_hash" => LoadBalanceStrategy::IpHash,
        _ => LoadBalanceStrategy::RoundRobin,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway_api::{
        HttpBackendRef, HttpPathMatch, HttpPathModifier, HttpRouteFilter, HttpRouteMatch,
        HttpRouteRule, HttpUrlRewrite,
    };
    use http::Method;

    fn prefix_match(value: &str, method: Option<&str>) -> HttpRouteMatch {
        HttpRouteMatch {
            path: Some(HttpPathMatch {
                path_type: "PathPrefix".into(),
                value: value.into(),
            }),
            method: method.map(|m| m.into()),
            headers: vec![],
        }
    }

    fn backend(name: &str, port: u16, weight: Option<i32>) -> HttpBackendRef {
        HttpBackendRef {
            name: name.into(),
            namespace: None,
            port: Some(port),
            weight,
        }
    }

    fn spec(rules: Vec<HttpRouteRule>) -> HTTPRouteSpec {
        HTTPRouteSpec {
            parent_refs: vec![],
            hostnames: vec![],
            rules,
        }
    }

    #[test]
    fn prefix_match_emits_exact_and_wildcard_routes() {
        let s = spec(vec![HttpRouteRule {
            matches: vec![prefix_match("/api", Some("GET"))],
            filters: vec![],
            backend_refs: vec![backend("api-svc", 8080, None)],
        }]);

        let (routes, upstreams) = httproute_to_route("api-route", "default", &s, &[], &std::collections::BTreeMap::new());

        assert_eq!(upstreams.len(), 1, "one upstream cluster for the rule");
        let cluster = &upstreams[0];
        assert_eq!(cluster.instances.len(), 1);
        assert_eq!(cluster.instances[0].address, "api-svc.default.svc");
        assert_eq!(cluster.instances[0].port, 8080);

        let paths: Vec<&str> = routes.iter().map(|r| r.path.as_str()).collect();
        assert!(paths.contains(&"/api"), "exact prefix route");
        assert!(
            paths.contains(&"/api/*octopus_prefix"),
            "subpath wildcard route"
        );
        for r in &routes {
            assert_eq!(r.method, Method::GET);
            assert_eq!(r.upstream, cluster.name);
            assert_eq!(r.source, RouteSource::GatewayApi);
        }
    }

    #[test]
    fn hostnames_become_per_host_routes() {
        let mut s = spec(vec![HttpRouteRule {
            matches: vec![prefix_match("/api", Some("GET"))],
            filters: vec![],
            backend_refs: vec![backend("api-svc", 8080, None)],
        }]);
        s.hostnames = vec!["api.example.com".into(), "*.acme.com".into()];

        let (routes, _) = httproute_to_route("api-route", "default", &s, &[], &std::collections::BTreeMap::new());

        let hosts: std::collections::HashSet<_> = routes.iter().map(|r| r.host.clone()).collect();
        assert!(
            hosts.contains(&HostMatch::Exact("api.example.com".into())),
            "exact hostname mapped"
        );
        assert!(
            hosts.contains(&HostMatch::Wildcard(".acme.com".into())),
            "wildcard hostname mapped"
        );
        assert!(
            !hosts.contains(&HostMatch::Any),
            "explicit hostnames must not yield Any"
        );
    }

    #[test]
    fn no_hostnames_yields_any_host() {
        let s = spec(vec![HttpRouteRule {
            matches: vec![prefix_match("/api", Some("GET"))],
            filters: vec![],
            backend_refs: vec![backend("api-svc", 8080, None)],
        }]);
        let (routes, _) = httproute_to_route("api-route", "default", &s, &[], &std::collections::BTreeMap::new());
        assert!(
            routes.iter().all(|r| r.host == HostMatch::Any),
            "no hostnames → any-host routes (legacy behavior)"
        );
    }

    #[test]
    fn octopus_route_convention_becomes_wildcard_host_route() {
        let spec = OctopusRouteSpec {
            path: "/*rest".into(),
            upstream: "unused".into(),
            methods: vec!["GET".into()],
            convention: Some(ConventionSpec {
                base_domain: "platform.com".into(),
                layout: vec!["service".into(), "namespace".into()],
                default_service: None,
                port: Some(8080),
                script: None,
                script_ref: None,
                backend_strategy: None,
                route_rules: vec![],
            }),
            ..Default::default()
        };

        let routes = octopus_route_to_route("tenants", "default", &spec);

        assert!(!routes.is_empty());
        for r in &routes {
            assert_eq!(
                r.host,
                HostMatch::Wildcard(".platform.com".into()),
                "convention route is wildcard-scoped to the base domain"
            );
            let conv = r.convention.as_ref().expect("convention carried on route");
            let t = conv.resolve("orders.acme.platform.com").unwrap();
            assert_eq!(t.service, "orders");
            assert_eq!(t.namespace, "acme");
            assert_eq!(t.port, 8080);
        }
    }

    #[test]
    fn convention_backend_strategy_parses() {
        let mut spec = ConventionSpec {
            base_domain: "x.com".into(),
            layout: vec!["service".into(), "namespace".into()],
            default_service: None,
            port: None,
            script: None,
            script_ref: None,
            backend_strategy: Some("EndpointSlice".into()),
            route_rules: vec![],
        };
        assert_eq!(
            convention_from_spec(&spec).backend,
            BackendStrategy::EndpointSlice
        );
        spec.backend_strategy = Some("servicedns".into());
        assert_eq!(
            convention_from_spec(&spec).backend,
            BackendStrategy::ServiceDns
        );
        spec.backend_strategy = None;
        assert_eq!(
            convention_from_spec(&spec).backend,
            BackendStrategy::ServiceDns
        );
    }

    #[test]
    fn intersect_both_empty_matches_any() {
        assert_eq!(intersect_hostnames(&[], &[]), vec![HostMatch::Any]);
    }

    #[test]
    fn intersect_route_empty_inherits_listener() {
        assert_eq!(
            intersect_hostnames(&[], &["*.a.com".to_string()]),
            vec![HostMatch::Wildcard(".a.com".into())]
        );
    }

    #[test]
    fn intersect_listener_empty_uses_route() {
        assert_eq!(
            intersect_hostnames(&["api.a.com".to_string()], &[]),
            vec![HostMatch::Exact("api.a.com".into())]
        );
    }

    #[test]
    fn intersect_wildcard_listener_narrows_to_exact_route() {
        assert_eq!(
            intersect_hostnames(&["api.a.com".to_string()], &["*.a.com".to_string()]),
            vec![HostMatch::Exact("api.a.com".into())]
        );
    }

    #[test]
    fn intersect_disjoint_yields_no_hosts() {
        assert!(
            intersect_hostnames(&["api.a.com".to_string()], &["api.b.com".to_string()]).is_empty(),
            "non-overlapping hostnames attach nothing"
        );
    }

    #[test]
    fn url_rewrite_replace_prefix_sets_strip_and_add() {
        let s = spec(vec![HttpRouteRule {
            matches: vec![prefix_match("/api", Some("GET"))],
            filters: vec![HttpRouteFilter {
                filter_type: "URLRewrite".into(),
                url_rewrite: Some(HttpUrlRewrite {
                    hostname: None,
                    path: Some(HttpPathModifier {
                        modifier_type: "ReplacePrefixMatch".into(),
                        replace_prefix_match: Some("/v2".into()),
                        replace_full_path: None,
                    }),
                }),
            }],
            backend_refs: vec![backend("api-svc", 8080, None)],
        }]);

        let (routes, _) = httproute_to_route("api-route", "default", &s, &[], &std::collections::BTreeMap::new());
        assert!(!routes.is_empty());
        for r in &routes {
            assert_eq!(r.strip_prefix.as_deref(), Some("/api"));
            assert_eq!(r.add_prefix.as_deref(), Some("/v2"));
        }
    }

    #[test]
    fn weighted_backends_build_multi_instance_cluster() {
        let s = spec(vec![HttpRouteRule {
            matches: vec![prefix_match("/", Some("GET"))],
            filters: vec![],
            backend_refs: vec![backend("v1", 80, Some(80)), backend("v2", 80, Some(20))],
        }]);

        let (_, upstreams) = httproute_to_route("split", "prod", &s, &[], &std::collections::BTreeMap::new());
        assert_eq!(upstreams.len(), 1);
        let cluster = &upstreams[0];
        assert_eq!(cluster.instances.len(), 2, "one instance per backend");
        let v1 = cluster
            .instances
            .iter()
            .find(|i| i.address.starts_with("v1."))
            .unwrap();
        let v2 = cluster
            .instances
            .iter()
            .find(|i| i.address.starts_with("v2."))
            .unwrap();
        assert_eq!(v1.weight, 80);
        assert_eq!(v2.weight, 20);
        assert_eq!(
            cluster.strategy,
            octopus_core::types::LoadBalanceStrategy::WeightedRoundRobin
        );
    }

    #[test]
    fn unset_method_expands_to_common_methods() {
        let s = spec(vec![HttpRouteRule {
            matches: vec![HttpRouteMatch {
                path: Some(HttpPathMatch {
                    path_type: "Exact".into(),
                    value: "/x".into(),
                }),
                method: None,
                headers: vec![],
            }],
            filters: vec![],
            backend_refs: vec![backend("svc", 80, None)],
        }]);

        let (routes, _) = httproute_to_route("r", "default", &s, &[], &std::collections::BTreeMap::new());
        let methods: std::collections::HashSet<String> =
            routes.iter().map(|r| r.method.to_string()).collect();
        assert!(methods.contains("GET") && methods.contains("POST") && methods.contains("DELETE"));
        // Exact path → no wildcard route.
        assert!(routes.iter().all(|r| r.path == "/x"));
    }

    #[test]
    fn empty_matches_is_catch_all() {
        let s = spec(vec![HttpRouteRule {
            matches: vec![],
            filters: vec![],
            backend_refs: vec![backend("svc", 80, None)],
        }]);

        let (routes, _) = httproute_to_route("r", "default", &s, &[], &std::collections::BTreeMap::new());
        let paths: std::collections::HashSet<&str> =
            routes.iter().map(|r| r.path.as_str()).collect();
        assert!(paths.contains("/"), "catch-all root");
        assert!(paths.contains("/*octopus_prefix"), "catch-all subpaths");
    }

    #[test]
    fn octopus_route_maps_fields_per_method() {
        use crate::crds::{OctopusRouteSpec, RateLimitSpec};
        let spec = OctopusRouteSpec {
            path: "/orders".into(),
            methods: vec!["GET".into(), "POST".into()],
            upstream: "orders-up".into(),
            priority: Some(5),
            auth_provider: Some("jwt".into()),
            strip_prefix: Some("/orders".into()),
            timeout_seconds: Some(30),
            rate_limit: Some(RateLimitSpec {
                requests: 100,
                window_seconds: 60,
            }),
            ..Default::default()
        };

        let routes = octopus_route_to_route("orders", "shop", &spec);
        assert_eq!(routes.len(), 2, "one route per method");
        for r in &routes {
            assert_eq!(r.path, "/orders");
            assert_eq!(r.upstream, "orders-up");
            assert_eq!(r.priority, 5);
            assert_eq!(r.source, RouteSource::OctopusRoute);
            assert_eq!(r.source_id, "shop/orders");
            assert_eq!(r.auth_provider.as_deref(), Some("jwt"));
            assert_eq!(r.strip_prefix.as_deref(), Some("/orders"));
            assert_eq!(r.timeout, Some(Duration::from_secs(30)));
            assert_eq!(
                r.rate_limit,
                Some(RateLimit {
                    requests: 100,
                    window: Duration::from_secs(60)
                })
            );
        }
        let methods: std::collections::HashSet<String> =
            routes.iter().map(|r| r.method.to_string()).collect();
        assert!(methods.contains("GET") && methods.contains("POST"));
    }

    #[test]
    fn octopus_route_maps_proxy_fields() {
        use crate::crds::OctopusRouteSpec;
        let spec = OctopusRouteSpec {
            parent_refs: vec!["gw".into()],
            path: "/twinos".into(),
            methods: vec!["GET".into()],
            upstream: "twinos".into(),
            strip_prefix: Some("/twinos".into()),
            path_mode: Some("strip".into()),
            rewrite_redirects: Some(true),
            upstream_origin: None,
            rewrite_cookie_path: None,
            tls_verify: None,
            ..Default::default()
        };
        let routes = octopus_route_to_route("r", "ns", &spec);
        let p = routes[0].proxy.as_ref().unwrap();
        assert!(p.rewrite_redirects);
        assert_eq!(p.path_mode, octopus_router::PathMode::Strip);
    }

    #[test]
    fn octopus_route_empty_methods_expands_all() {
        use crate::crds::OctopusRouteSpec;
        let spec = OctopusRouteSpec {
            path: "/x".into(),
            methods: vec![],
            upstream: "up".into(),
            ..Default::default()
        };
        let routes = octopus_route_to_route("r", "default", &spec);
        assert_eq!(routes.len(), DEFAULT_METHODS.len());
    }

    #[test]
    fn octopus_upstream_builds_cluster_with_weights() {
        use crate::crds::{OctopusUpstreamSpec, UpstreamTarget};
        let spec = OctopusUpstreamSpec {
            targets: vec![
                UpstreamTarget {
                    host: "10.0.0.1".into(),
                    port: 8080,
                    weight: Some(3),
                },
                UpstreamTarget {
                    host: "10.0.0.2".into(),
                    port: 8080,
                    weight: None,
                },
            ],
            lb_strategy: Some("least_conn".into()),
        };

        let cluster = octopus_upstream_to_cluster("orders-up", "shop", &spec);
        assert_eq!(cluster.name, "orders-up");
        assert_eq!(cluster.instances.len(), 2);
        assert_eq!(cluster.strategy, LoadBalanceStrategy::LeastConnections);
        let i1 = cluster
            .instances
            .iter()
            .find(|i| i.address == "10.0.0.1")
            .unwrap();
        let i2 = cluster
            .instances
            .iter()
            .find(|i| i.address == "10.0.0.2")
            .unwrap();
        assert_eq!(i1.weight, 3);
        assert_eq!(i2.weight, 1, "missing weight defaults to 1");
    }

    #[test]
    fn octopus_gateway_maps_hostnames_and_policy() {
        use crate::crds::{GatewayDefaultPolicy, GatewayIsolation, RateLimitSpec};
        let spec = OctopusGatewaySpec {
            listen: "0.0.0.0:8080".into(),
            gateway_class_name: None,
            default_auth_provider: None,
            hostnames: vec!["api.twinos.cloud".into()],
            default_policy: Some(GatewayDefaultPolicy {
                auth_provider: Some("jwt".into()),
                timeout_seconds: Some(5),
                rate_limit: Some(RateLimitSpec {
                    requests: 100,
                    window_seconds: 60,
                }),
                cors: None,
            }),
            isolation: GatewayIsolation::Shared,
            farp_binding: false,
        };

        let entry = octopus_gateway_to_entry("platform-api", &spec);
        assert_eq!(&*entry.id, "platform-api");
        assert_eq!(
            entry.domains,
            vec![HostMatch::Exact("api.twinos.cloud".into())]
        );
        assert_eq!(entry.policy.auth_provider.as_deref(), Some("jwt"));
        assert_eq!(entry.policy.timeout, Some(Duration::from_secs(5)));
        assert_eq!(
            entry.policy.rate_limit,
            Some((100, Duration::from_secs(60)))
        );
    }

    #[test]
    fn octopus_gateway_maps_cors_policy() {
        use crate::crds::{GatewayCorsSpec, GatewayDefaultPolicy};
        let spec = OctopusGatewaySpec {
            listen: "0.0.0.0:8080".into(),
            gateway_class_name: None,
            default_auth_provider: None,
            hostnames: vec!["api.twinos.cloud".into()],
            default_policy: Some(GatewayDefaultPolicy {
                auth_provider: None,
                timeout_seconds: None,
                rate_limit: None,
                cors: Some(GatewayCorsSpec {
                    allowed_origins: vec!["https://app.twinos.cloud".into()],
                    allowed_methods: vec!["GET".into(), "POST".into()],
                    allowed_headers: vec!["authorization".into()],
                    allow_credentials: true,
                    max_age: 600,
                }),
            }),
            isolation: Default::default(),
            farp_binding: false,
        };
        let entry = octopus_gateway_to_entry("platform-api", &spec);
        let cors = entry.policy.cors.expect("cors mapped onto gateway policy");
        assert_eq!(cors.allowed_origins, vec!["https://app.twinos.cloud"]);
        assert_eq!(cors.allowed_methods, vec!["GET", "POST"]);
        assert!(cors.allow_credentials);
        assert_eq!(cors.max_age, 600);
    }

    #[test]
    fn octopus_gateway_empty_hostnames_match_any() {
        let spec = OctopusGatewaySpec {
            listen: "0.0.0.0:8080".into(),
            gateway_class_name: None,
            default_auth_provider: None,
            hostnames: vec![],
            default_policy: None,
            isolation: Default::default(),
            farp_binding: false,
        };
        let entry = octopus_gateway_to_entry("default", &spec);
        assert_eq!(entry.domains, vec![HostMatch::Any]);
        assert!(entry.policy.auth_provider.is_none());
    }

    #[test]
    fn convention_route_rules_are_mapped_and_resolve() {
        use crate::crds::ConventionRouteRuleSpec;
        let spec = ConventionSpec {
            base_domain: "twinos.cloud".into(),
            layout: vec!["namespace".into()],
            default_service: Some("studio".into()),
            port: Some(3000),
            script: None,
            script_ref: None,
            backend_strategy: None,
            route_rules: vec![ConventionRouteRuleSpec {
                path_prefix: "/api".into(),
                strip_prefix: true,
                service_override: Some("api".into()),
                port_override: Some(7900),
                add_prefix: None,
            }],
        };
        let conv = convention_from_spec(&spec);
        assert_eq!(conv.route_rules.len(), 1);
        let (target, rewrite) = conv
            .resolve_with_path("customer-a.twinos.cloud", "/api/orders")
            .unwrap();
        assert_eq!(target.namespace, "customer-a");
        assert_eq!(target.service, "api");
        assert_eq!(target.port, 7900);
        assert_eq!(rewrite.unwrap().strip.as_deref(), Some("/api"));
    }

    #[test]
    fn octopus_gateway_legacy_default_auth_used_when_no_policy() {
        let spec = OctopusGatewaySpec {
            listen: "0.0.0.0:8080".into(),
            gateway_class_name: None,
            default_auth_provider: Some("legacy".into()),
            hostnames: vec!["api.twinos.cloud".into()],
            default_policy: None,
            isolation: Default::default(),
            farp_binding: false,
        };
        let entry = octopus_gateway_to_entry("platform-api", &spec);
        assert_eq!(entry.policy.auth_provider.as_deref(), Some("legacy"));
    }

    fn grpc_spec(matches: Vec<crate::gateway_api::GrpcRouteMatch>) -> GRPCRouteSpec {
        GRPCRouteSpec {
            parent_refs: vec![],
            hostnames: vec![],
            rules: vec![crate::gateway_api::GrpcRouteRule {
                matches,
                filters: vec![],
                backend_refs: vec![backend("grpc-svc", 50051, None)],
            }],
        }
    }

    fn grpc_match(
        service: Option<&str>,
        method: Option<&str>,
    ) -> crate::gateway_api::GrpcRouteMatch {
        crate::gateway_api::GrpcRouteMatch {
            method: Some(GrpcMethodMatch {
                match_type: None,
                service: service.map(|s| s.into()),
                method: method.map(|m| m.into()),
            }),
            headers: vec![],
        }
    }

    #[test]
    fn grpc_full_method_maps_to_path() {
        let (routes, upstreams) = grpcroute_to_route(
            "echo",
            "default",
            &grpc_spec(vec![grpc_match(Some("echo.Echo"), Some("Ping"))]),
            &[],
        );
        assert_eq!(upstreams.len(), 1);
        assert_eq!(upstreams[0].instances[0].address, "grpc-svc.default.svc");
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].path, "/echo.Echo/Ping");
        assert_eq!(routes[0].method, Method::POST);
        assert_eq!(routes[0].source, RouteSource::GatewayApi);
        assert_eq!(routes[0].source_id, "default/echo");
    }

    #[test]
    fn grpc_service_only_uses_wildcard() {
        let (routes, _) = grpcroute_to_route(
            "echo",
            "default",
            &grpc_spec(vec![grpc_match(Some("echo.Echo"), None)]),
            &[],
        );
        assert_eq!(routes[0].path, "/echo.Echo/*octopus_prefix");
    }

    #[test]
    fn grpc_empty_matches_is_catch_all() {
        let (routes, _) = grpcroute_to_route("echo", "default", &grpc_spec(vec![]), &[]);
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].path, "/*octopus_prefix");
        assert_eq!(routes[0].method, Method::POST);
    }

    #[test]
    fn httproute_annotations_map_to_proxy() {
        use std::collections::BTreeMap;
        let mut ann = BTreeMap::new();
        ann.insert("octopus.io/path-mode".to_string(), "strip".to_string());
        ann.insert("octopus.io/rewrite-redirects".to_string(), "true".to_string());
        let spec = proxy_spec_from_annotations(&ann);
        let p = spec.unwrap();
        assert!(p.rewrite_redirects);
        assert_eq!(p.path_mode, octopus_router::PathMode::Strip);
    }

    #[test]
    fn no_annotations_means_no_proxy() {
        use std::collections::BTreeMap;
        assert!(proxy_spec_from_annotations(&BTreeMap::new()).is_none());
    }
}
