//! Applies a merged [`RoutingTable`] to the live router.
//!
//! This is the single bridge from Kubernetes-derived routing intent to the
//! running gateway: it replaces the router's routes and (re)registers upstream
//! clusters, mirroring the gateway's config hot-reload path.

use crate::ir::{IntermediateRoute, RoutingTable};
use crate::{K8sError, Result};
use octopus_router::{Route, RouteBuilder, Router};

/// Replace the router's routes and upstreams with the contents of `table`.
///
/// Clears existing routes, (re)registers every upstream cluster, then adds each
/// route. A single bad route is logged and skipped rather than failing the whole
/// apply.
pub fn apply_to_router(router: &Router, table: &RoutingTable) -> Result<()> {
    router.clear();

    for cluster in &table.upstreams {
        router.register_upstream(cluster.clone());
    }

    for intermediate in &table.routes {
        match build_route(intermediate) {
            Ok(route) => {
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

/// Build a router [`Route`] from one [`IntermediateRoute`].
fn build_route(route: &IntermediateRoute) -> Result<Route> {
    let mut builder = RouteBuilder::new()
        .path(&route.path)
        .method(route.method.clone())
        .upstream_name(&route.upstream)
        .priority(route.priority)
        .auth_provider(route.auth_provider.as_deref())
        .skip_auth(route.skip_auth)
        .require_roles(&route.require_roles)
        .require_scopes(&route.require_scopes)
        .authz_rule(route.authz_rule.as_deref())
        .timeout(route.timeout);

    if let Some(prefix) = &route.strip_prefix {
        builder = builder.strip_prefix(prefix);
    }
    if let Some(prefix) = &route.add_prefix {
        builder = builder.add_prefix(prefix);
    }
    if let Some(rl) = &route.rate_limit {
        builder = builder.rate_limit(rl.requests, rl.window);
    }

    builder.build().map_err(|e| K8sError::Apply(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{IntermediateRoute, RouteSource, RoutingTable};
    use http::Method;
    use octopus_core::UpstreamCluster;

    fn table(routes: Vec<IntermediateRoute>, upstream: &str) -> RoutingTable {
        RoutingTable {
            routes,
            upstreams: vec![UpstreamCluster::new(upstream)],
        }
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
        apply_to_router(&router, &t).unwrap();

        assert!(router.get_upstream("up").is_some(), "upstream registered");
        let m = router.match_route(&Method::GET, "/api").unwrap();
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
        )
        .unwrap();
        assert!(router.match_route(&Method::GET, "/old").is_ok());

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
        )
        .unwrap();
        assert!(router.match_route(&Method::GET, "/new").is_ok());
        assert!(
            router.match_route(&Method::GET, "/old").is_err(),
            "previous routes cleared on re-apply"
        );
    }

    #[test]
    fn preserves_route_attributes() {
        let router = Router::new();
        let mut route = IntermediateRoute::new(Method::GET, "/x", "up", RouteSource::OctopusRoute);
        route.strip_prefix = Some("/x".into());
        route.priority = 7;
        apply_to_router(&router, &table(vec![route], "up")).unwrap();

        let m = router.match_route(&Method::GET, "/x").unwrap();
        assert_eq!(m.route.strip_prefix.as_deref(), Some("/x"));
        assert_eq!(m.route.priority, 7);
    }
}
