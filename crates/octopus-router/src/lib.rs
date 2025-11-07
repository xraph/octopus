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

pub mod matcher;
pub mod route;
pub mod trie;

pub use matcher::{Match, PathMatcher};
pub use route::{Route, RouteBuilder};
pub use trie::RouteTrie;

use dashmap::DashMap;
use http::Method;
use octopus_core::{Error, Result, UpstreamCluster, UpstreamInstance};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Router for managing and matching routes
#[derive(Debug, Clone)]
pub struct Router {
    /// Trie for each HTTP method
    tries: Arc<DashMap<Method, RouteTrie>>,

    /// Named upstreams
    upstreams: Arc<DashMap<String, UpstreamCluster>>,

    /// Round-robin counter for load balancing
    rr_counter: Arc<AtomicUsize>,
}

impl Router {
    /// Create a new router
    pub fn new() -> Self {
        Self {
            tries: Arc::new(DashMap::new()),
            upstreams: Arc::new(DashMap::new()),
            rr_counter: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Add a route
    pub fn add_route(&self, route: Route) -> Result<()> {
        let method = route.method.clone();

        // Get or create trie for this method
        let mut trie = self
            .tries
            .entry(method.clone())
            .or_insert_with(RouteTrie::new);

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

    /// Match a request path
    pub fn match_route(&self, method: &Method, path: &str) -> Result<Match> {
        let trie = self
            .tries
            .get(method)
            .ok_or_else(|| Error::RouteNotFound(format!("No routes for method {}", method)))?;

        trie.match_path(path)
            .ok_or_else(|| Error::RouteNotFound(path.to_string()))
    }

    /// Register an upstream cluster
    pub fn register_upstream(&self, cluster: UpstreamCluster) {
        let name = cluster.name.clone();
        self.upstreams.insert(name.clone(), cluster);
        tracing::debug!(upstream = %name, "Upstream registered");
    }

    /// Get an upstream cluster
    pub fn get_upstream(&self, name: &str) -> Option<UpstreamCluster> {
        self.upstreams.get(name).map(|r| r.clone())
    }

    /// Remove an upstream cluster
    pub fn remove_upstream(&self, name: &str) -> bool {
        let removed = self.upstreams.remove(name).is_some();
        if removed {
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

    /// Find a route for a given method and path (convenience method for handler)
    pub fn find_route(&self, method: &Method, path: &str) -> Result<Route> {
        let matched = self.match_route(method, path)?;
        Ok(matched.route)
    }

    /// Select an upstream instance from a cluster (with simple round-robin)
    pub fn select_instance(&self, upstream_name: &str) -> Result<UpstreamInstance> {
        let cluster = self.get_upstream(upstream_name).ok_or_else(|| {
            Error::UpstreamConnection(format!("Upstream '{}' not found", upstream_name))
        })?;

        let healthy = cluster.healthy_instances();

        if healthy.is_empty() {
            return Err(Error::UpstreamConnection(format!(
                "No healthy instances for upstream '{}'",
                upstream_name
            )));
        }

        // Simple round-robin selection
        let index = self.rr_counter.fetch_add(1, Ordering::Relaxed) % healthy.len();
        Ok(healthy[index].clone())
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

        let matched = router.match_route(&Method::GET, "/users/123").unwrap();
        assert_eq!(matched.route.path, "/users/:id");
        assert_eq!(matched.params.get("id"), Some(&"123".to_string()));
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
}
