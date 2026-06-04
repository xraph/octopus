//! Applies a merged [`RoutingTable`] to the live router.
//!
//! This is the single bridge from Kubernetes-derived routing intent to the
//! running gateway: it replaces the router's routes and (re)registers upstream
//! clusters, mirroring the gateway's config hot-reload path.

use crate::ir::{IntermediateRoute, RoutingTable};
use crate::{K8sError, Result};
use octopus_core::UpstreamCluster;
use octopus_router::{
    gateway_scoped_upstream, GatewayPolicy, Route, RouteBuilder, Router, VirtualGatewayIndex,
};
use std::collections::HashMap;

/// Replace the router's routes and upstreams with the contents of `table`.
///
/// Clears existing routes, (re)registers every upstream cluster, then adds each
/// route, filling in defaults inherited from each route's virtual gateway
/// (`gateways`). A single bad route is logged and skipped rather than failing the
/// whole apply.
pub fn apply_to_router(
    router: &Router,
    table: &RoutingTable,
    gateways: &VirtualGatewayIndex,
) -> Result<()> {
    router.clear();

    for cluster in &table.upstreams {
        router.register_upstream(cluster.clone());
    }

    // Original clusters by name, for registering gateway-scoped clones (W4).
    let originals: HashMap<&str, &UpstreamCluster> = table
        .upstreams
        .iter()
        .map(|c| (c.name.as_str(), c))
        .collect();

    for intermediate in &table.routes {
        let policy = intermediate
            .gateway_id
            .as_deref()
            .and_then(|id| gateways.by_id(id))
            .map(|gw| &gw.policy);
        match build_route(intermediate, policy) {
            Ok(route) => {
                // Per-gateway upstream isolation: if the route's upstream was
                // gateway-scoped (`{gw}:name`), register a clone under that name so
                // its load-balancer / circuit-breaker state is independent.
                if route.upstream_name != intermediate.upstream
                    && router.get_upstream(&route.upstream_name).is_none()
                {
                    if let Some(orig) = originals.get(intermediate.upstream.as_str()) {
                        let mut scoped = (*orig).clone();
                        scoped.name = route.upstream_name.clone();
                        router.register_upstream(scoped);
                    }
                }
                if let Err(e) = router.add_route(route) {
                    tracing::error!(
                        path = %intermediate.path,
                        method = %intermediate.method,
                        error = %e,
                        "Failed to add route during apply"
                    );
                }
            }
            Err(e) => {
                tracing::error!(
                    path = %intermediate.path,
                    method = %intermediate.method,
                    error = %e,
                    "Failed to build route during apply"
                );
            }
        }
    }

    Ok(())
}

/// Build a router [`Route`] from one [`IntermediateRoute`], filling unset fields
/// from the route's virtual-gateway `policy`. An explicit value on the route
/// always wins over the gateway default.
fn build_route(route: &IntermediateRoute, policy: Option<&GatewayPolicy>) -> Result<Route> {
    // Resolve gateway defaults: route value first, gateway policy as fallback.
    let auth_provider = route
        .auth_provider
        .clone()
        .or_else(|| policy.and_then(|p| p.auth_provider.clone()));
    let timeout = route.timeout.or_else(|| policy.and_then(|p| p.timeout));
    let rate_limit = route
        .rate_limit
        .as_ref()
        .map(|rl| (rl.requests, rl.window))
        .or_else(|| policy.and_then(|p| p.rate_limit));

    let mut builder = RouteBuilder::new()
        .path(&route.path)
        .method(route.method.clone())
        .host(route.host.clone())
        .upstream_name(gateway_scoped_upstream(
            route.gateway_id.as_deref(),
            &route.upstream,
        ))
        .priority(route.priority)
        .gateway_id(route.gateway_id.as_deref())
        .auth_provider(auth_provider.as_deref())
        .skip_auth(route.skip_auth)
        .require_roles(&route.require_roles)
        .require_scopes(&route.require_scopes)
        .authz_rule(route.authz_rule.as_deref())
        .timeout(timeout);

    if let Some(prefix) = &route.strip_prefix {
        builder = builder.strip_prefix(prefix);
    }
    if let Some(prefix) = &route.add_prefix {
        builder = builder.add_prefix(prefix);
    }
    if let Some((requests, window)) = rate_limit {
        builder = builder.rate_limit(requests, window);
    }
    if route.convention.is_some() {
        builder = builder.convention(route.convention.clone());
    }

    builder.build().map_err(|e| K8sError::Apply(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{IntermediateRoute, RouteSource, RoutingTable};
    use http::Method;
    use octopus_core::UpstreamCluster;
    use octopus_router::{GatewayEntry, GatewayPolicy, HostMatch, VirtualGatewayIndex};
    use std::time::Duration;

    fn table(routes: Vec<IntermediateRoute>, upstream: &str) -> RoutingTable {
        RoutingTable {
            routes,
            upstreams: vec![UpstreamCluster::new(upstream)],
        }
    }

    fn index_with_policy(id: &str, policy: GatewayPolicy) -> VirtualGatewayIndex {
        VirtualGatewayIndex::new(vec![GatewayEntry {
            id: id.into(),
            domains: vec![HostMatch::Any],
            policy,
        }])
    }

    #[test]
    fn gateway_default_auth_inherited_when_route_has_none() {
        let router = Router::new();
        let route = IntermediateRoute::new(Method::GET, "/x", "up", RouteSource::OctopusRoute)
            .with_gateway("platform-api");
        let idx = index_with_policy(
            "platform-api",
            GatewayPolicy {
                auth_provider: Some("jwt".into()),
                ..Default::default()
            },
        );
        apply_to_router(&router, &table(vec![route], "up"), &idx).unwrap();

        let m = router
            .match_route("example.com", &Method::GET, "/x")
            .unwrap();
        assert_eq!(m.route.auth_provider.as_deref(), Some("jwt"));
        assert_eq!(m.route.gateway_id.as_deref(), Some("platform-api"));
    }

    #[test]
    fn explicit_route_auth_wins_over_gateway_default() {
        let router = Router::new();
        let mut route = IntermediateRoute::new(Method::GET, "/x", "up", RouteSource::OctopusRoute)
            .with_gateway("platform-api");
        route.auth_provider = Some("explicit".into());
        let idx = index_with_policy(
            "platform-api",
            GatewayPolicy {
                auth_provider: Some("jwt".into()),
                ..Default::default()
            },
        );
        apply_to_router(&router, &table(vec![route], "up"), &idx).unwrap();

        let m = router
            .match_route("example.com", &Method::GET, "/x")
            .unwrap();
        assert_eq!(m.route.auth_provider.as_deref(), Some("explicit"));
    }

    #[test]
    fn route_without_gateway_is_unaffected() {
        let router = Router::new();
        let route = IntermediateRoute::new(Method::GET, "/x", "up", RouteSource::OctopusRoute);
        let idx = index_with_policy(
            "platform-api",
            GatewayPolicy {
                auth_provider: Some("jwt".into()),
                ..Default::default()
            },
        );
        apply_to_router(&router, &table(vec![route], "up"), &idx).unwrap();

        let m = router
            .match_route("example.com", &Method::GET, "/x")
            .unwrap();
        assert!(m.route.auth_provider.is_none());
        assert!(m.route.gateway_id.is_none());
    }

    #[test]
    fn gateway_default_timeout_and_rate_limit_inherited() {
        let router = Router::new();
        let route = IntermediateRoute::new(Method::GET, "/x", "up", RouteSource::OctopusRoute)
            .with_gateway("g");
        let idx = index_with_policy(
            "g",
            GatewayPolicy {
                timeout: Some(Duration::from_secs(7)),
                rate_limit: Some((100, Duration::from_secs(60))),
                ..Default::default()
            },
        );
        apply_to_router(&router, &table(vec![route], "up"), &idx).unwrap();

        let m = router
            .match_route("example.com", &Method::GET, "/x")
            .unwrap();
        assert_eq!(m.route.timeout, Some(Duration::from_secs(7)));
        assert_eq!(m.route.rate_limit, Some((100, Duration::from_secs(60))));
    }

    #[test]
    fn gateway_scoped_route_gets_isolated_upstream() {
        let router = Router::new();
        let route = IntermediateRoute::new(Method::GET, "/x", "up", RouteSource::OctopusRoute)
            .with_gateway("g1");
        apply_to_router(
            &router,
            &table(vec![route], "up"),
            &index_with_policy("g1", GatewayPolicy::default()),
        )
        .unwrap();

        let m = router
            .match_route("example.com", &Method::GET, "/x")
            .unwrap();
        assert_eq!(m.route.upstream_name, "g1:up");
        assert!(
            router.get_upstream("g1:up").is_some(),
            "gateway-scoped upstream registered"
        );
    }

    #[test]
    fn two_gateways_sharing_an_upstream_get_distinct_clusters() {
        let router = Router::new();
        let r1 = IntermediateRoute::new(Method::GET, "/a", "shared", RouteSource::OctopusRoute)
            .with_gateway("g1");
        let r2 = IntermediateRoute::new(Method::GET, "/b", "shared", RouteSource::OctopusRoute)
            .with_gateway("g2");
        let t = RoutingTable {
            routes: vec![r1, r2],
            upstreams: vec![UpstreamCluster::new("shared")],
        };
        apply_to_router(&router, &t, &VirtualGatewayIndex::default()).unwrap();

        assert!(router.get_upstream("g1:shared").is_some());
        assert!(router.get_upstream("g2:shared").is_some());
        // the two gateways' clusters are distinct registrations (isolated state)
        assert_eq!(router.upstream_count(), 3); // shared + g1:shared + g2:shared
    }

    #[test]
    fn applies_routes_and_upstreams() {
        let router = Router::new();
        let t = table(
            vec![
                IntermediateRoute::new(Method::GET, "/api", "up", RouteSource::OctopusRoute),
                IntermediateRoute::new(Method::POST, "/api", "up", RouteSource::OctopusRoute),
            ],
            "up",
        );
        apply_to_router(&router, &t, &VirtualGatewayIndex::default()).unwrap();

        assert!(router.get_upstream("up").is_some(), "upstream registered");
        let m = router
            .match_route("example.com", &Method::GET, "/api")
            .unwrap();
        assert_eq!(m.route.upstream_name, "up");
        assert_eq!(router.get_all_routes().len(), 2);
    }

    #[test]
    fn apply_replaces_previous_routes() {
        let router = Router::new();
        apply_to_router(
            &router,
            &table(
                vec![IntermediateRoute::new(
                    Method::GET,
                    "/old",
                    "up",
                    RouteSource::Farp,
                )],
                "up",
            ),
            &VirtualGatewayIndex::default(),
        )
        .unwrap();
        assert!(router
            .match_route("example.com", &Method::GET, "/old")
            .is_ok());

        apply_to_router(
            &router,
            &table(
                vec![IntermediateRoute::new(
                    Method::GET,
                    "/new",
                    "up",
                    RouteSource::Farp,
                )],
                "up",
            ),
            &VirtualGatewayIndex::default(),
        )
        .unwrap();
        assert!(router
            .match_route("example.com", &Method::GET, "/new")
            .is_ok());
        assert!(
            router
                .match_route("example.com", &Method::GET, "/old")
                .is_err(),
            "previous routes cleared on re-apply"
        );
    }

    #[test]
    fn preserves_route_attributes() {
        let router = Router::new();
        let mut route = IntermediateRoute::new(Method::GET, "/x", "up", RouteSource::OctopusRoute);
        route.strip_prefix = Some("/x".into());
        route.priority = 7;
        apply_to_router(
            &router,
            &table(vec![route], "up"),
            &VirtualGatewayIndex::default(),
        )
        .unwrap();

        let m = router
            .match_route("example.com", &Method::GET, "/x")
            .unwrap();
        assert_eq!(m.route.strip_prefix.as_deref(), Some("/x"));
        assert_eq!(m.route.priority, 7);
    }
}
