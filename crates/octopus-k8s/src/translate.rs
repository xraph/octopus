//! Translate Gateway API resources into the intermediate routing representation.

use crate::crds::{OctopusRouteSpec, OctopusUpstreamSpec};
use crate::gateway_api::{GRPCRouteSpec, GrpcMethodMatch, HTTPRouteSpec, HttpBackendRef};
use crate::ir::{IntermediateRoute, RateLimit, RouteSource};
use http::Method;
use octopus_core::types::LoadBalanceStrategy;
use octopus_core::{UpstreamCluster, UpstreamInstance};
use std::time::Duration;

/// Wildcard parameter name used to emulate Gateway API `PathPrefix` semantics
/// (the segment-based router matches a registered prefix exactly, so subpaths
/// need a trailing wildcard route).
const PREFIX_WILDCARD: &str = "octopus_prefix";

/// HTTP methods a match with no explicit `method` expands to (Gateway API
/// treats an unset method as "all methods").
const DEFAULT_METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

/// Translate an `HTTPRoute` into intermediate routes and the upstream clusters
/// they target. `name`/`namespace` are the route's own identity.
pub fn httproute_to_route(
    name: &str,
    namespace: &str,
    spec: &HTTPRouteSpec,
) -> (Vec<IntermediateRoute>, Vec<UpstreamCluster>) {
    let mut routes = Vec::new();
    let mut upstreams = Vec::new();

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
                    let mut route = IntermediateRoute::new(
                        method,
                        path.clone(),
                        &cluster_name,
                        RouteSource::GatewayApi,
                    );
                    route.source_id = format!("{namespace}/{name}");
                    if replace_prefix.is_some() {
                        route.strip_prefix = Some(path_value.clone());
                        route.add_prefix = replace_prefix.clone();
                    }
                    routes.push(route);
                }
            }
        }
    }

    (routes, upstreams)
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

/// Translate a `GRPCRoute` into intermediate routes + upstream clusters. gRPC
/// requests are HTTP/2 POSTs to `/{service}/{method}`.
pub fn grpcroute_to_route(
    name: &str,
    namespace: &str,
    spec: &GRPCRouteSpec,
) -> (Vec<IntermediateRoute>, Vec<UpstreamCluster>) {
    let mut routes = Vec::new();
    let mut upstreams = Vec::new();
    let source_id = format!("{namespace}/{name}");

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
            let mut route = IntermediateRoute::new(
                Method::POST,
                grpc_path(m.method.as_ref()),
                &cluster_name,
                RouteSource::GatewayApi,
            );
            route.source_id = source_id.clone();
            routes.push(route);
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

        let (routes, upstreams) = httproute_to_route("api-route", "default", &s);

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

        let (routes, _) = httproute_to_route("api-route", "default", &s);
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

        let (_, upstreams) = httproute_to_route("split", "prod", &s);
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

        let (routes, _) = httproute_to_route("r", "default", &s);
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

        let (routes, _) = httproute_to_route("r", "default", &s);
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
        );
        assert_eq!(routes[0].path, "/echo.Echo/*octopus_prefix");
    }

    #[test]
    fn grpc_empty_matches_is_catch_all() {
        let (routes, _) = grpcroute_to_route("echo", "default", &grpc_spec(vec![]));
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].path, "/*octopus_prefix");
        assert_eq!(routes[0].method, Method::POST);
    }
}
