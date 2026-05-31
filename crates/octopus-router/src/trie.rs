//! Trie-based route storage for efficient lookups

use crate::matcher::{Match, PathMatcher};
use crate::route::Route;
use octopus_core::{Error, Result};
use std::collections::HashMap;

/// Node in the route trie
#[derive(Debug)]
struct TrieNode {
    /// Static children (exact match)
    children: HashMap<String, TrieNode>,

    /// Parameter child (e.g., :id)
    param_child: Option<Box<TrieNode>>,

    /// Wildcard child (e.g., *filepath)
    wildcard_child: Option<Box<TrieNode>>,

    /// Routes at this node (terminal). Multiple routes may share a method+path
    /// when they are scoped to different hosts; selection picks the most
    /// specific host at match time.
    routes: Vec<Route>,

    /// Path matcher for this node (shared by all routes here — same path)
    matcher: Option<PathMatcher>,
}

impl TrieNode {
    fn new() -> Self {
        Self {
            children: HashMap::new(),
            param_child: None,
            wildcard_child: None,
            routes: Vec::new(),
            matcher: None,
        }
    }
}

/// Trie for storing and matching routes
#[derive(Debug)]
pub struct RouteTrie {
    root: TrieNode,
    count: usize,
}

impl RouteTrie {
    /// Create a new route trie
    pub fn new() -> Self {
        Self {
            root: TrieNode::new(),
            count: 0,
        }
    }

    /// Insert a route into the trie
    pub fn insert(&mut self, route: Route) -> Result<()> {
        let segments: Vec<&str> = route.path.split('/').filter(|s| !s.is_empty()).collect();

        let mut current = &mut self.root;

        for segment in &segments {
            if segment.starts_with(':') {
                // Parameter segment
                if current.param_child.is_none() {
                    current.param_child = Some(Box::new(TrieNode::new()));
                }
                current = current.param_child.as_mut().unwrap();
            } else if segment.starts_with('*') {
                // Wildcard segment (must be last)
                if current.wildcard_child.is_none() {
                    current.wildcard_child = Some(Box::new(TrieNode::new()));
                }
                current = current.wildcard_child.as_mut().unwrap();
                break; // Wildcard must be terminal
            } else {
                // Static segment
                current = current
                    .children
                    .entry(segment.to_string())
                    .or_insert_with(TrieNode::new);
            }
        }

        // Store route and matcher at terminal node. The same path may host
        // several routes (one per host), but a given (path, host) is unique.
        if current.routes.iter().any(|r| r.host == route.host) {
            return Err(Error::Config(format!(
                "Route already exists: {} (host {:?})",
                route.path, route.host
            )));
        }

        if current.matcher.is_none() {
            current.matcher = Some(PathMatcher::new(route.path.clone()));
        }
        current.routes.push(route);
        self.count += 1;

        Ok(())
    }

    /// Remove a route from the trie
    pub fn remove(&mut self, path: &str) -> Result<()> {
        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

        let removed = Self::remove_recursive(&mut self.root, &segments, 0);
        if removed > 0 {
            self.count -= removed;
            Ok(())
        } else {
            Err(Error::RouteNotFound(path.to_string()))
        }
    }

    /// Returns the number of routes removed at the matched terminal node.
    fn remove_recursive(node: &mut TrieNode, segments: &[&str], index: usize) -> usize {
        if index == segments.len() {
            // Reached end of path — drop every route registered here.
            if !node.routes.is_empty() {
                let removed = node.routes.len();
                node.routes.clear();
                node.matcher = None;
                return removed;
            }
            return 0;
        }

        let segment = segments[index];

        if segment.starts_with(':') {
            if let Some(ref mut child) = node.param_child {
                return Self::remove_recursive(child, segments, index + 1);
            }
        } else if segment.starts_with('*') {
            if let Some(ref mut child) = node.wildcard_child {
                return Self::remove_recursive(child, segments, index + 1);
            }
        } else if let Some(child) = node.children.get_mut(segment) {
            return Self::remove_recursive(child, segments, index + 1);
        }

        0
    }

    /// Match a request `host` + `path` against routes in the trie.
    ///
    /// Only routes whose host matches are considered; among those, the most
    /// specific host wins (exact > wildcard > any), then higher priority.
    /// `host` must be lowercased by the caller.
    pub fn match_path(&self, host: &str, path: &str) -> Option<Match> {
        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

        let mut matches = Vec::new();
        Self::match_recursive(&self.root, host, &segments, 0, &mut matches);

        // Most specific host first, then highest priority.
        matches.sort_by(|a, b| {
            b.route
                .host
                .specificity()
                .cmp(&a.route.host.specificity())
                .then(b.route.priority.cmp(&a.route.priority))
        });
        matches.into_iter().next()
    }

    fn match_recursive(
        node: &TrieNode,
        host: &str,
        segments: &[&str],
        index: usize,
        matches: &mut Vec<Match>,
    ) {
        if index == segments.len() {
            // Reached end of path — collect every host-matching route here.
            if let Some(matcher) = &node.matcher {
                let path = format!("/{}", segments.join("/"));
                if let Some(params) = matcher.matches(&path) {
                    for route in &node.routes {
                        if route.host.matches(host) {
                            matches.push(Match {
                                route: route.clone(),
                                params: params.clone(),
                                wildcard: None,
                            });
                        }
                    }
                }
            }
            return;
        }

        let segment = segments[index];

        // Try static match first (highest priority)
        if let Some(child) = node.children.get(segment) {
            Self::match_recursive(child, host, segments, index + 1, matches);
        }

        // Try parameter match
        if let Some(ref child) = node.param_child {
            Self::match_recursive(child, host, segments, index + 1, matches);
        }

        // Try wildcard match (lowest priority)
        if let Some(ref child) = node.wildcard_child {
            if let Some(matcher) = &child.matcher {
                let path = format!("/{}", segments.join("/"));
                if let Some(params) = matcher.matches(&path) {
                    for route in &child.routes {
                        if route.host.matches(host) {
                            matches.push(Match {
                                route: route.clone(),
                                params: params.clone(),
                                wildcard: Some(segments[index..].join("/")),
                            });
                        }
                    }
                }
            }
        }
    }

    /// Get number of routes in the trie
    pub fn len(&self) -> usize {
        self.count
    }

    /// Check if trie is empty
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Get all routes from the trie
    pub fn get_all_routes(&self) -> Vec<Route> {
        let mut routes = Vec::new();
        Self::collect_routes(&self.root, &mut routes);
        routes
    }

    fn collect_routes(node: &TrieNode, routes: &mut Vec<Route>) {
        for route in &node.routes {
            routes.push(route.clone());
        }

        for child in node.children.values() {
            Self::collect_routes(child, routes);
        }

        if let Some(ref child) = node.param_child {
            Self::collect_routes(child, routes);
        }

        if let Some(ref child) = node.wildcard_child {
            Self::collect_routes(child, routes);
        }
    }
}

impl Default for RouteTrie {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::HostMatch;
    use crate::RouteBuilder;
    use http::Method;

    fn route_h(path: &str, upstream: &str, host: HostMatch) -> Route {
        RouteBuilder::new()
            .method(Method::GET)
            .path(path)
            .upstream_name(upstream)
            .host(host)
            .build()
            .unwrap()
    }

    #[test]
    fn same_path_different_hosts_coexist() {
        let mut trie = RouteTrie::new();
        trie.insert(route_h(
            "/api",
            "acme-up",
            HostMatch::Exact("acme.example.com".into()),
        ))
        .unwrap();
        trie.insert(route_h(
            "/api",
            "globex-up",
            HostMatch::Exact("globex.example.com".into()),
        ))
        .unwrap();

        assert_eq!(trie.len(), 2);
        assert_eq!(
            trie.match_path("acme.example.com", "/api")
                .unwrap()
                .route
                .upstream_name,
            "acme-up"
        );
        assert_eq!(
            trie.match_path("globex.example.com", "/api")
                .unwrap()
                .route
                .upstream_name,
            "globex-up"
        );
    }

    #[test]
    fn exact_host_beats_wildcard_beats_any_same_path() {
        let mut trie = RouteTrie::new();
        trie.insert(route_h("/api", "any-up", HostMatch::Any))
            .unwrap();
        trie.insert(route_h(
            "/api",
            "wild-up",
            HostMatch::Wildcard(".example.com".into()),
        ))
        .unwrap();
        trie.insert(route_h(
            "/api",
            "exact-up",
            HostMatch::Exact("api.example.com".into()),
        ))
        .unwrap();

        // exact host is the most specific
        assert_eq!(
            trie.match_path("api.example.com", "/api")
                .unwrap()
                .route
                .upstream_name,
            "exact-up"
        );
        // a different subdomain falls through to the wildcard
        assert_eq!(
            trie.match_path("foo.example.com", "/api")
                .unwrap()
                .route
                .upstream_name,
            "wild-up"
        );
        // an unrelated host falls through to the any-host route
        assert_eq!(
            trie.match_path("other.org", "/api")
                .unwrap()
                .route
                .upstream_name,
            "any-up"
        );
    }

    #[test]
    fn no_host_match_returns_none_when_no_any_route() {
        let mut trie = RouteTrie::new();
        trie.insert(route_h(
            "/api",
            "acme-up",
            HostMatch::Exact("acme.example.com".into()),
        ))
        .unwrap();
        assert!(trie.match_path("other.com", "/api").is_none());
    }

    #[test]
    fn duplicate_path_and_host_is_rejected() {
        let mut trie = RouteTrie::new();
        trie.insert(route_h("/api", "u1", HostMatch::Exact("a.com".into())))
            .unwrap();
        let dup = trie.insert(route_h("/api", "u2", HostMatch::Exact("a.com".into())));
        assert!(dup.is_err(), "same (path, host) must be rejected");
    }

    #[test]
    fn test_insert_and_match_static() {
        let mut trie = RouteTrie::new();

        let route = RouteBuilder::new()
            .method(Method::GET)
            .path("/users")
            .upstream_name("user-service")
            .build()
            .unwrap();

        trie.insert(route).unwrap();
        assert_eq!(trie.len(), 1);

        let matched = trie.match_path("example.com", "/users").unwrap();
        assert_eq!(matched.route.path, "/users");
    }

    #[test]
    fn test_insert_and_match_param() {
        let mut trie = RouteTrie::new();

        let route = RouteBuilder::new()
            .method(Method::GET)
            .path("/users/:id")
            .upstream_name("user-service")
            .build()
            .unwrap();

        trie.insert(route).unwrap();

        let matched = trie.match_path("example.com", "/users/123").unwrap();
        assert_eq!(matched.route.path, "/users/:id");
        assert_eq!(matched.params.get("id"), Some(&"123".to_string()));
    }

    #[test]
    fn test_priority_matching() {
        let mut trie = RouteTrie::new();

        // Add low priority wildcard
        let route1 = RouteBuilder::new()
            .method(Method::GET)
            .path("/api/*path")
            .upstream_name("default")
            .priority(1)
            .build()
            .unwrap();

        // Add high priority specific route
        let route2 = RouteBuilder::new()
            .method(Method::GET)
            .path("/api/users")
            .upstream_name("users")
            .priority(10)
            .build()
            .unwrap();

        trie.insert(route1).unwrap();
        trie.insert(route2).unwrap();

        // Should match high priority route
        let matched = trie.match_path("example.com", "/api/users").unwrap();
        assert_eq!(matched.route.upstream_name, "users");
    }

    #[test]
    fn test_remove_route() {
        let mut trie = RouteTrie::new();

        let route = RouteBuilder::new()
            .method(Method::GET)
            .path("/users/:id")
            .upstream_name("user-service")
            .build()
            .unwrap();

        trie.insert(route).unwrap();
        assert_eq!(trie.len(), 1);

        trie.remove("/users/:id").unwrap();
        assert_eq!(trie.len(), 0);

        assert!(trie.match_path("example.com", "/users/123").is_none());
    }

    #[test]
    fn test_complex_routing() {
        let mut trie = RouteTrie::new();

        let routes = vec![
            ("/api/users", "users-static"),
            ("/api/users/:id", "users-detail"),
            ("/api/users/:id/posts", "user-posts"),
            ("/api/posts/:id", "posts-detail"),
            ("/static/*filepath", "static-files"),
        ];

        for (path, upstream) in routes {
            let route = RouteBuilder::new()
                .method(Method::GET)
                .path(path)
                .upstream_name(upstream)
                .build()
                .unwrap();
            trie.insert(route).unwrap();
        }

        assert_eq!(trie.len(), 5);

        // Test various matches
        assert_eq!(
            trie.match_path("example.com", "/api/users")
                .unwrap()
                .route
                .upstream_name,
            "users-static"
        );
        assert_eq!(
            trie.match_path("example.com", "/api/users/123")
                .unwrap()
                .route
                .upstream_name,
            "users-detail"
        );
        assert_eq!(
            trie.match_path("example.com", "/api/users/123/posts")
                .unwrap()
                .route
                .upstream_name,
            "user-posts"
        );
        assert_eq!(
            trie.match_path("example.com", "/static/css/main.css")
                .unwrap()
                .route
                .upstream_name,
            "static-files"
        );
    }
}
