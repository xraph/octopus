//! # Octopus Router
//!
//! High-performance trie-based router with support for:
//! - Path parameter extraction (`/users/:id`)
//! - Wildcard matching (`/static/*filepath`)
//! - Method-based routing
//! - Priority-based matching
//! - Dynamic route registration
//!
//! ## Performance
//!
//! - O(k) lookup time where k is the path length
//! - Lock-free reads using DashMap
//! - Pre-compiled regex for wildcards
//! - Zero allocations for static routes

#![forbid(unsafe_code)]
#![warn(
    missing_docs,
    missing_debug_implementations,
    rust_2018_idioms,
    unreachable_pub
)]

pub mod convention;
pub mod host;
pub mod load_balancer;
pub mod matcher;
mod proxy_spec;
pub mod route;
pub mod trie;
pub mod virtual_gateway;

pub use convention::{
    BackendStrategy, Convention, ConventionRouteRule, ConventionTarget, LabelRole, PathRewrite,
};
pub use host::{normalize_request_host, HostMatch};
pub use load_balancer::{new_load_balancer, LoadBalancer};
pub use matcher::{Match, PathMatcher};
pub use proxy_spec::{PathMode, ProxySpec, Scheme, UpstreamOrigin};
pub use route::{Route, RouteBuilder, RouteCorsOverride, RouteOrigin};
pub use trie::{node_key, RouteTrie};
pub use virtual_gateway::{
    gateway_scoped_upstream, GatewayEntry, GatewayPolicy, VirtualGatewayIndex,
};

use dashmap::DashMap;
use http::Method;
use octopus_core::{Error, LoadBalanceStrategy, Result, UpstreamCluster, UpstreamInstance};
use std::collections::HashSet;
use std::sync::Arc;

/// Router for managing and matching routes
#[derive(Debug, Clone)]
pub struct Router {
    /// Trie for each HTTP method
    tries: Arc<DashMap<Method, RouteTrie>>,

    /// Named upstreams
    upstreams: Arc<DashMap<String, UpstreamCluster>>,

    /// Per-upstream load balancers (keyed by upstream name)
    load_balancers: Arc<DashMap<String, Arc<dyn LoadBalancer>>>,

    /// Default load balancer (round-robin)
    default_lb: Arc<dyn LoadBalancer>,
}

impl Router {
    /// Create a new router
    pub fn new() -> Self {
        Self {
            tries: Arc::new(DashMap::new()),
            upstreams: Arc::new(DashMap::new()),
            load_balancers: Arc::new(DashMap::new()),
            default_lb: Arc::from(new_load_balancer(LoadBalanceStrategy::RoundRobin)),
        }
    }

    /// Add a route
    pub fn add_route(&self, route: Route) -> Result<()> {
        let method = route.method.clone();

        // Get or create trie for this method
        let mut trie = self.tries.entry(method.clone()).or_default();

        // Insert route into trie
        trie.insert(route)?;

        tracing::debug!(method = %method, "Route added to router");

        Ok(())
    }

    /// Remove a route
    pub fn remove_route(&self, method: &Method, path: &str) -> Result<()> {
        if let Some(mut trie) = self.tries.get_mut(method) {
            trie.remove(path)?;
            tracing::debug!(method = %method, path = %path, "Route removed from router");
        }

        Ok(())
    }

    /// Remove the one route at exactly `(method, path, host, id)`.
    /// Returns whether a route was removed.
    pub fn remove_route_exact(
        &self,
        method: &Method,
        path: &str,
        host: &HostMatch,
        id: Option<&str>,
    ) -> bool {
        let removed = self
            .tries
            .get_mut(method)
            .is_some_and(|mut trie| trie.remove_exact(path, host, id));
        if removed {
            tracing::debug!(method = %method, path = %path, host = ?host, id = ?id, "Route removed from router");
        }
        removed
    }

    /// Remove every route carrying `id`, across all methods and hosts.
    /// Returns the number of routes removed.
    pub fn remove_routes_by_id(&self, id: &str) -> usize {
        let removed = self
            .tries
            .iter_mut()
            .map(|mut entry| entry.value_mut().remove_by_id(id))
            .sum();
        if removed > 0 {
            tracing::debug!(id = %id, removed, "Routes removed from router by id");
        }
        removed
    }

    /// Every route carrying `id`, across all methods and hosts.
    pub fn routes_by_id(&self, id: &str) -> Vec<Route> {
        self.get_all_routes()
            .into_iter()
            .filter(|route| route.id.as_deref() == Some(id))
            .collect()
    }

    /// Move every route of `origin` onto `hosts`.
    ///
    /// The routes are deduplicated by `(id, method, path)`, removed, and
    /// re-inserted once per distinct host (once as [`HostMatch::Any`] when
    /// `hosts` is empty). Repeated entries in `hosts` count once. Routes of
    /// other origins are untouched. Returns the number of routes inserted.
    pub fn rehost_routes(&self, origin: RouteOrigin, hosts: &[HostMatch]) -> usize {
        let mut targets: Vec<HostMatch> = Vec::with_capacity(hosts.len().max(1));
        for host in hosts {
            if !targets.contains(host) {
                targets.push(host.clone());
            }
        }
        if targets.is_empty() {
            targets.push(HostMatch::Any);
        }
        let mut inserted = 0;

        for mut entry in self.tries.iter_mut() {
            let trie = entry.value_mut();
            let mut seen = HashSet::new();
            let mut originals = Vec::new();
            for route in trie.get_all_routes() {
                if route.origin != origin {
                    continue;
                }
                trie.remove_exact(&route.path, &route.host, route.id.as_deref());
                if seen.insert((route.id.clone(), route.path.clone())) {
                    originals.push(route);
                }
            }

            for route in originals {
                for host in &targets {
                    let mut copy = route.clone();
                    copy.host = host.clone();
                    match trie.insert(copy) {
                        Ok(()) => inserted += 1,
                        Err(err) => tracing::warn!(
                            method = %route.method,
                            path = %route.path,
                            host = ?host,
                            error = %err,
                            "Route could not be rehosted"
                        ),
                    }
                }
            }
        }

        tracing::debug!(origin = origin.as_str(), inserted, "Routes rehosted");
        inserted
    }

    /// Match a request `host` + path. `host` must be lowercased by the caller;
    /// pass `""` (or any value) when host-scoping is irrelevant — host-agnostic
    /// routes match every host.
    pub fn match_route(&self, host: &str, method: &Method, path: &str) -> Result<Match> {
        let trie = self
            .tries
            .get(method)
            .ok_or_else(|| Error::RouteNotFound(format!("No routes for method {method}")))?;

        trie.match_path(host, path)
            .ok_or_else(|| Error::RouteNotFound(path.to_string()))
    }

    /// Register an upstream cluster
    pub fn register_upstream(&self, cluster: UpstreamCluster) {
        let name = cluster.name.clone();
        let strategy = cluster.strategy;
        self.upstreams.insert(name.clone(), cluster);

        // Create and cache the load balancer for this upstream's strategy
        let lb = new_load_balancer(strategy);
        self.load_balancers.insert(name.clone(), Arc::from(lb));

        tracing::debug!(upstream = %name, strategy = ?strategy, "Upstream registered");
    }

    /// Register an upstream cluster lazily and race-free: if `name` is not yet
    /// known, `build` is invoked once to create it (and its load balancer).
    /// Concurrent callers for the same name register exactly one cluster.
    ///
    /// Used for convention-derived upstreams (e.g. `k8s://<ns>/<svc>`) that are
    /// materialized on first request rather than declared up front.
    pub fn ensure_upstream(&self, name: &str, build: impl FnOnce() -> UpstreamCluster) {
        use dashmap::mapref::entry::Entry;
        if let Entry::Vacant(slot) = self.upstreams.entry(name.to_string()) {
            let cluster = build();
            let lb = new_load_balancer(cluster.strategy);
            self.load_balancers.insert(name.to_string(), Arc::from(lb));
            slot.insert(cluster);
        }
    }

    /// Get an upstream cluster
    pub fn get_upstream(&self, name: &str) -> Option<UpstreamCluster> {
        self.upstreams.get(name).map(|r| r.clone())
    }

    /// Remove an upstream cluster
    pub fn remove_upstream(&self, name: &str) -> bool {
        let removed = self.upstreams.remove(name).is_some();
        if removed {
            self.load_balancers.remove(name);
            tracing::debug!(upstream = %name, "Upstream removed");
        }
        removed
    }

    /// Get route count for a method
    pub fn route_count(&self, method: &Method) -> usize {
        self.tries.get(method).map(|trie| trie.len()).unwrap_or(0)
    }

    /// Get total route count across all methods
    pub fn total_route_count(&self) -> usize {
        self.tries.iter().map(|entry| entry.value().len()).sum()
    }

    /// Get upstream count
    pub fn upstream_count(&self) -> usize {
        self.upstreams.len()
    }

    /// Clear all routes
    pub fn clear(&self) {
        self.tries.clear();
        tracing::debug!("All routes cleared");
    }

    /// Find a route for a given host, method and path (convenience for the handler)
    pub fn find_route(&self, host: &str, method: &Method, path: &str) -> Result<Route> {
        let matched = self.match_route(host, method, path)?;
        Ok(matched.route)
    }

    /// Select an upstream instance from a cluster using its configured load balancing strategy.
    ///
    /// The `key` parameter is used by hash-based strategies (e.g., IP hash) for
    /// deterministic selection. Pass the client IP or an empty string if not relevant.
    pub fn select_instance_with_key(
        &self,
        upstream_name: &str,
        key: &str,
    ) -> Result<UpstreamInstance> {
        let cluster = self.get_upstream(upstream_name).ok_or_else(|| {
            Error::UpstreamConnection(format!("Upstream '{upstream_name}' not found"))
        })?;

        let healthy = cluster.healthy_instances();

        if healthy.is_empty() {
            return Err(Error::UpstreamConnection(format!(
                "No healthy instances for upstream '{upstream_name}'"
            )));
        }

        let lb = self
            .load_balancers
            .get(upstream_name)
            .map(|r| Arc::clone(r.value()))
            .unwrap_or_else(|| Arc::clone(&self.default_lb));

        let index = lb.select(&healthy, key).unwrap_or(0);
        Ok(healthy[index].clone())
    }

    /// Select an upstream instance from a cluster (convenience method, uses empty key).
    pub fn select_instance(&self, upstream_name: &str) -> Result<UpstreamInstance> {
        self.select_instance_with_key(upstream_name, "")
    }

    /// Get all routes across all methods
    pub fn get_all_routes(&self) -> Vec<Route> {
        let mut all_routes = Vec::new();

        for entry in self.tries.iter() {
            let _method = entry.key();
            let trie = entry.value();
            let mut routes = trie.get_all_routes();
            all_routes.append(&mut routes);
        }

        all_routes
    }

    /// Get all upstreams
    pub fn get_all_upstreams(&self) -> Vec<UpstreamCluster> {
        self.upstreams
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }
}

impl Default for Router {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_router_new() {
        let router = Router::new();
        assert_eq!(router.total_route_count(), 0);
        assert_eq!(router.upstream_count(), 0);
    }

    #[test]
    fn test_add_route() {
        let router = Router::new();

        let route = RouteBuilder::new()
            .path("/users/:id")
            .method(Method::GET)
            .upstream_name("user-service")
            .build()
            .unwrap();

        router.add_route(route).unwrap();

        assert_eq!(router.route_count(&Method::GET), 1);
        assert_eq!(router.total_route_count(), 1);
    }

    #[test]
    fn test_match_route() {
        let router = Router::new();

        let route = RouteBuilder::new()
            .path("/users/:id")
            .method(Method::GET)
            .upstream_name("user-service")
            .build()
            .unwrap();

        router.add_route(route).unwrap();

        let matched = router
            .match_route("example.com", &Method::GET, "/users/123")
            .unwrap();
        assert_eq!(matched.route.path, "/users/:id");
        assert_eq!(matched.params.get("id"), Some(&"123".to_string()));
    }

    #[test]
    fn ensure_upstream_registers_once_and_is_idempotent() {
        use std::cell::Cell;
        let router = Router::new();
        let calls = Cell::new(0u32);

        router.ensure_upstream("k8s://acme/orders", || {
            calls.set(calls.get() + 1);
            UpstreamCluster::new("k8s://acme/orders")
        });
        assert!(router.get_upstream("k8s://acme/orders").is_some());
        assert_eq!(calls.get(), 1);

        // Second call for the same key is a no-op (builder not invoked again).
        router.ensure_upstream("k8s://acme/orders", || {
            calls.set(calls.get() + 1);
            UpstreamCluster::new("k8s://acme/orders")
        });
        assert_eq!(calls.get(), 1, "builder runs only on first ensure");
        assert_eq!(router.upstream_count(), 1);
    }

    #[test]
    fn test_upstream_management() {
        let router = Router::new();

        let cluster = UpstreamCluster::new("test-service");
        router.register_upstream(cluster);

        assert_eq!(router.upstream_count(), 1);
        assert!(router.get_upstream("test-service").is_some());

        assert!(router.remove_upstream("test-service"));
        assert_eq!(router.upstream_count(), 0);
    }

    fn origin_route(
        method: Method,
        path: &str,
        upstream: &str,
        host: HostMatch,
        id: Option<&str>,
        origin: RouteOrigin,
    ) -> Route {
        RouteBuilder::new()
            .method(method)
            .path(path)
            .upstream_name(upstream)
            .host(host)
            .id(id)
            .origin(origin)
            .build()
            .unwrap()
    }

    fn matches(router: &Router, host: &str, method: &Method, path: &str) -> bool {
        router.match_route(host, method, path).is_ok()
    }

    #[test]
    fn rehost_routes_moves_only_that_origin() {
        let router = Router::new();
        let old = HostMatch::Exact("old.dev".into());
        for method in [Method::GET, Method::POST] {
            router
                .add_route(origin_route(
                    method,
                    "/svc/x",
                    "svc",
                    old.clone(),
                    Some("farp:svc"),
                    RouteOrigin::Farp,
                ))
                .unwrap();
        }
        router
            .add_route(origin_route(
                Method::GET,
                "/y",
                "cfg",
                old,
                None,
                RouteOrigin::Config,
            ))
            .unwrap();

        let inserted = router.rehost_routes(
            RouteOrigin::Farp,
            &[
                HostMatch::Exact("a.dev".into()),
                HostMatch::Exact("b.dev".into()),
            ],
        );
        assert_eq!(inserted, 4);
        assert_eq!(router.total_route_count(), 5);

        for method in [Method::GET, Method::POST] {
            assert!(matches(&router, "a.dev", &method, "/svc/x"));
            assert!(matches(&router, "b.dev", &method, "/svc/x"));
            assert!(!matches(&router, "old.dev", &method, "/svc/x"));
        }
        assert!(matches(&router, "old.dev", &Method::GET, "/y"));
        assert!(!matches(&router, "a.dev", &Method::GET, "/y"));
    }

    #[test]
    fn rehost_routes_with_no_hosts_makes_routes_host_agnostic() {
        let router = Router::new();
        router
            .add_route(origin_route(
                Method::GET,
                "/svc/x",
                "svc",
                HostMatch::Exact("a.dev".into()),
                Some("farp:svc"),
                RouteOrigin::Farp,
            ))
            .unwrap();
        router
            .add_route(origin_route(
                Method::GET,
                "/svc/x",
                "svc",
                HostMatch::Exact("b.dev".into()),
                Some("farp:svc"),
                RouteOrigin::Farp,
            ))
            .unwrap();

        assert_eq!(router.rehost_routes(RouteOrigin::Farp, &[]), 1);
        assert_eq!(router.total_route_count(), 1);
        assert!(matches(&router, "anything.dev", &Method::GET, "/svc/x"));
    }

    #[test]
    fn rehost_routes_ignores_duplicate_hosts() {
        let router = Router::new();
        router
            .add_route(origin_route(
                Method::GET,
                "/svc/x",
                "svc",
                HostMatch::Exact("old.dev".into()),
                Some("farp:svc"),
                RouteOrigin::Farp,
            ))
            .unwrap();

        let a = HostMatch::Exact("a.dev".into());
        let b = HostMatch::Exact("b.dev".into());
        let inserted = router.rehost_routes(RouteOrigin::Farp, &[a.clone(), b, a]);
        assert_eq!(inserted, 2);
        assert_eq!(router.total_route_count(), 2);
        assert!(matches(&router, "a.dev", &Method::GET, "/svc/x"));
        assert!(matches(&router, "b.dev", &Method::GET, "/svc/x"));
    }

    #[test]
    fn remove_route_exact_removes_one_host_only() {
        let router = Router::new();
        let a = HostMatch::Exact("a.dev".into());
        let b = HostMatch::Exact("b.dev".into());
        for host in [a.clone(), b] {
            router
                .add_route(origin_route(
                    Method::GET,
                    "/p",
                    "u",
                    host,
                    Some("lpr-1"),
                    RouteOrigin::Admin,
                ))
                .unwrap();
        }

        assert!(router.remove_route_exact(&Method::GET, "/p", &a, Some("lpr-1")));
        assert!(!router.remove_route_exact(&Method::GET, "/p", &a, Some("lpr-1")));
        assert!(!router.remove_route_exact(&Method::POST, "/p", &a, Some("lpr-1")));
        assert!(!matches(&router, "a.dev", &Method::GET, "/p"));
        assert!(matches(&router, "b.dev", &Method::GET, "/p"));
    }

    #[test]
    fn routes_by_id_and_remove_routes_by_id_span_methods() {
        let router = Router::new();
        for method in [Method::GET, Method::POST] {
            router
                .add_route(origin_route(
                    method,
                    "/p",
                    "u",
                    HostMatch::Any,
                    Some("lpr-1"),
                    RouteOrigin::Admin,
                ))
                .unwrap();
        }
        router
            .add_route(origin_route(
                Method::GET,
                "/q",
                "u",
                HostMatch::Any,
                Some("lpr-2"),
                RouteOrigin::Admin,
            ))
            .unwrap();

        assert_eq!(router.routes_by_id("lpr-1").len(), 2);
        assert_eq!(router.remove_routes_by_id("lpr-1"), 2);
        assert!(router.routes_by_id("lpr-1").is_empty());
        assert_eq!(router.total_route_count(), 1);
        assert_eq!(router.remove_routes_by_id("missing"), 0);
    }
}
