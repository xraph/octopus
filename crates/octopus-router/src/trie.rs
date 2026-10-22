//! Trie-based route storage for efficient lookups

use crate::host::HostMatch;
use crate::matcher::{Match, PathMatcher};
use crate::route::{Route, RouteOrigin};
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
    /// when they are scoped to different hosts or carry different ids;
    /// selection picks the most specific host at match time.
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

    /// Drop the matcher once the last route at this node is gone.
    fn clear_matcher_if_empty(&mut self) {
        if self.routes.is_empty() {
            self.matcher = None;
        }
    }
}

/// One segment of a route path as the trie stores it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NodeSegment<'a> {
    /// A literal segment, matched exactly.
    Static(&'a str),
    /// A `:name` parameter. Every parameter name shares one child.
    Param,
    /// A `*name` wildcard. It ends the path; later segments are ignored.
    Wildcard,
}

/// The segments of `path` as the trie walks them: empty segments dropped,
/// parameters and wildcards stripped of their names, nothing after a
/// wildcard.
fn node_segments(path: &str) -> Vec<NodeSegment<'_>> {
    let mut segments = Vec::new();
    for segment in path.split('/').filter(|s| !s.is_empty()) {
        if segment.starts_with(':') {
            segments.push(NodeSegment::Param);
        } else if segment.starts_with('*') {
            segments.push(NodeSegment::Wildcard);
            break;
        } else {
            segments.push(NodeSegment::Static(segment));
        }
    }
    segments
}

/// The key of the trie node a route with `path` is stored at.
///
/// Two paths with the same key share a node, and so compete for the same
/// requests: `/x` and `/x/`, `/users/:id` and `/users/:uid`, `/files/*` and
/// `/files/*rest` all do. Parameters become `:`, wildcards become `*`, and
/// the root is `/`.
pub fn node_key(path: &str) -> String {
    let mut key = String::new();
    for segment in node_segments(path) {
        key.push('/');
        key.push_str(match segment {
            NodeSegment::Static(s) => s,
            NodeSegment::Param => ":",
            NodeSegment::Wildcard => "*",
        });
    }
    if key.is_empty() {
        key.push('/');
    }
    key
}

/// Routes at one terminal node that are visible to a request for `host`.
///
/// A route must match the host, and a discovered route (FARP, Kubernetes) is
/// hidden by any declared route (config, admin) at the same node with an
/// equal [`HostMatch`]. Host specificity and priority are applied afterwards.
fn visible_routes<'a>(routes: &'a [Route], host: &'a str) -> impl Iterator<Item = &'a Route> {
    routes.iter().filter(move |route| {
        route.host.matches(host)
            && (route.origin.is_declared()
                || !routes
                    .iter()
                    .any(|other| other.origin.is_declared() && other.host == route.host))
    })
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
        let mut current = &mut self.root;

        for segment in node_segments(&route.path) {
            current = match segment {
                NodeSegment::Param => current
                    .param_child
                    .get_or_insert_with(|| Box::new(TrieNode::new())),
                NodeSegment::Wildcard => current
                    .wildcard_child
                    .get_or_insert_with(|| Box::new(TrieNode::new())),
                NodeSegment::Static(segment) => current
                    .children
                    .entry(segment.to_string())
                    .or_insert_with(TrieNode::new),
            };
        }

        // Store route and matcher at terminal node. The same path may host
        // several routes, but a given (path, host, id) is unique. Two `None`
        // ids are equal, so legacy duplicates are still rejected.
        if current
            .routes
            .iter()
            .any(|r| r.host == route.host && r.id == route.id)
        {
            return Err(Error::Config(format!(
                "Route already exists: {} (host {:?}, id {:?})",
                route.path, route.host, route.id
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

    /// Remove every route carrying `id`, wherever it sits in the trie.
    /// Returns the number of routes removed.
    pub fn remove_by_id(&mut self, id: &str) -> usize {
        self.remove_where(&|r| r.id.as_deref() == Some(id))
    }

    /// Remove every route carrying `id` that came from `origin`. Routes of
    /// other origins keep the id. Returns the number of routes removed.
    pub fn remove_by_id_and_origin(&mut self, id: &str, origin: RouteOrigin) -> usize {
        self.remove_where(&|r| r.origin == origin && r.id.as_deref() == Some(id))
    }

    /// Remove every route `doomed` selects. Returns the number removed.
    fn remove_where(&mut self, doomed: &dyn Fn(&Route) -> bool) -> usize {
        let removed = Self::remove_where_recursive(&mut self.root, doomed);
        self.count -= removed;
        removed
    }

    fn remove_where_recursive(node: &mut TrieNode, doomed: &dyn Fn(&Route) -> bool) -> usize {
        let before = node.routes.len();
        node.routes.retain(|r| !doomed(r));
        let mut removed = before - node.routes.len();
        if removed > 0 {
            node.clear_matcher_if_empty();
        }

        for child in node.children.values_mut() {
            removed += Self::remove_where_recursive(child, doomed);
        }
        if let Some(ref mut child) = node.param_child {
            removed += Self::remove_where_recursive(child, doomed);
        }
        if let Some(ref mut child) = node.wildcard_child {
            removed += Self::remove_where_recursive(child, doomed);
        }
        removed
    }

    /// Remove the one route stored at exactly `(path, host, id)`.
    /// Returns whether a route was removed.
    pub fn remove_exact(&mut self, path: &str, host: &HostMatch, id: Option<&str>) -> bool {
        let Some(node) = self.node_mut(path) else {
            return false;
        };
        let Some(pos) = node
            .routes
            .iter()
            .position(|r| &r.host == host && r.id.as_deref() == id)
        else {
            return false;
        };
        node.routes.remove(pos);
        node.clear_matcher_if_empty();
        self.count -= 1;
        true
    }

    /// The terminal node a route with `path` would be stored at, if it exists.
    fn node_mut(&mut self, path: &str) -> Option<&mut TrieNode> {
        let mut current = &mut self.root;
        for segment in node_segments(path) {
            current = match segment {
                NodeSegment::Param => current.param_child.as_deref_mut()?,
                NodeSegment::Wildcard => current.wildcard_child.as_deref_mut()?,
                NodeSegment::Static(segment) => current.children.get_mut(segment)?,
            };
        }
        Some(current)
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
                    for route in visible_routes(&node.routes, host) {
                        matches.push(Match {
                            route: route.clone(),
                            params: params.clone(),
                            wildcard: None,
                        });
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
                    for route in visible_routes(&child.routes, host) {
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
    use crate::{RouteBuilder, RouteOrigin};
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

    fn route_o(
        path: &str,
        upstream: &str,
        host: HostMatch,
        id: Option<&str>,
        origin: RouteOrigin,
        priority: i32,
    ) -> Route {
        RouteBuilder::new()
            .method(Method::GET)
            .path(path)
            .upstream_name(upstream)
            .host(host)
            .id(id)
            .origin(origin)
            .priority(priority)
            .build()
            .unwrap()
    }

    fn upstream_for(trie: &RouteTrie, host: &str, path: &str) -> Option<String> {
        trie.match_path(host, path).map(|m| m.route.upstream_name)
    }

    #[test]
    fn declared_route_shadows_discovered_route_on_same_host_and_path() {
        let host = HostMatch::Exact("api.acme.dev".into());
        let mut trie = RouteTrie::new();
        trie.insert(route_o(
            "/billing/invoices",
            "billing",
            host.clone(),
            Some("farp:billing"),
            RouteOrigin::Farp,
            100,
        ))
        .unwrap();
        trie.insert(route_o(
            "/billing/invoices",
            "manual",
            host,
            Some("lpr-1"),
            RouteOrigin::Admin,
            0,
        ))
        .unwrap();

        assert_eq!(
            upstream_for(&trie, "api.acme.dev", "/billing/invoices").as_deref(),
            Some("manual")
        );
        assert_eq!(trie.remove_by_id("lpr-1"), 1);
        assert_eq!(
            upstream_for(&trie, "api.acme.dev", "/billing/invoices").as_deref(),
            Some("billing")
        );
    }

    #[test]
    fn declared_route_does_not_shadow_discovered_route_on_another_host() {
        let mut trie = RouteTrie::new();
        trie.insert(route_o(
            "/p",
            "billing",
            HostMatch::Exact("api.acme.dev".into()),
            Some("farp:billing"),
            RouteOrigin::Farp,
            0,
        ))
        .unwrap();
        trie.insert(route_o(
            "/p",
            "manual",
            HostMatch::Wildcard(".acme.dev".into()),
            Some("lpr-1"),
            RouteOrigin::Config,
            0,
        ))
        .unwrap();
        // Different HostMatch: no shadowing, the exact host still wins.
        assert_eq!(
            upstream_for(&trie, "api.acme.dev", "/p").as_deref(),
            Some("billing")
        );
        assert_eq!(
            upstream_for(&trie, "www.acme.dev", "/p").as_deref(),
            Some("manual")
        );
    }

    #[test]
    fn discovered_route_still_wins_on_a_more_specific_path() {
        let host = HostMatch::Exact("api.acme.dev".into());
        let mut trie = RouteTrie::new();
        trie.insert(route_o(
            "/billing/*",
            "manual",
            host.clone(),
            Some("lpr-1"),
            RouteOrigin::Admin,
            0,
        ))
        .unwrap();
        trie.insert(route_o(
            "/billing/invoices",
            "billing",
            host,
            Some("farp:billing"),
            RouteOrigin::Farp,
            100,
        ))
        .unwrap();

        assert_eq!(
            upstream_for(&trie, "api.acme.dev", "/billing/invoices").as_deref(),
            Some("billing")
        );
        assert_eq!(
            upstream_for(&trie, "api.acme.dev", "/billing/other").as_deref(),
            Some("manual")
        );
    }

    #[test]
    fn same_path_host_and_id_is_rejected_but_different_ids_coexist() {
        let host = HostMatch::Exact("a.com".into());
        let mut trie = RouteTrie::new();
        trie.insert(route_o(
            "/p",
            "u1",
            host.clone(),
            Some("a"),
            RouteOrigin::Admin,
            0,
        ))
        .unwrap();
        assert!(trie
            .insert(route_o(
                "/p",
                "u2",
                host.clone(),
                Some("a"),
                RouteOrigin::Admin,
                0
            ))
            .is_err());
        trie.insert(route_o("/p", "u3", host, Some("b"), RouteOrigin::Admin, 0))
            .unwrap();
        assert_eq!(trie.len(), 2);
    }

    #[test]
    fn remove_by_id_leaves_other_routes_at_the_same_path() {
        let mut trie = RouteTrie::new();
        trie.insert(route_o(
            "/p",
            "ux",
            HostMatch::Exact("x.com".into()),
            Some("a"),
            RouteOrigin::Admin,
            0,
        ))
        .unwrap();
        trie.insert(route_o(
            "/p",
            "uy",
            HostMatch::Exact("y.com".into()),
            Some("b"),
            RouteOrigin::Admin,
            0,
        ))
        .unwrap();

        assert_eq!(trie.remove_by_id("a"), 1);
        assert_eq!(trie.len(), 1);
        assert!(trie.match_path("x.com", "/p").is_none());
        assert_eq!(upstream_for(&trie, "y.com", "/p").as_deref(), Some("uy"));
        assert_eq!(trie.remove_by_id("a"), 0);
    }

    #[test]
    fn remove_exact_removes_only_the_named_route() {
        let x = HostMatch::Exact("x.com".into());
        let mut trie = RouteTrie::new();
        trie.insert(route_o(
            "/p",
            "u1",
            x.clone(),
            Some("a"),
            RouteOrigin::Admin,
            0,
        ))
        .unwrap();
        trie.insert(route_o("/p", "u2", x.clone(), None, RouteOrigin::Config, 0))
            .unwrap();

        assert!(!trie.remove_exact("/p", &x, Some("zzz")));
        assert!(!trie.remove_exact("/q", &x, Some("a")));
        assert!(trie.remove_exact("/p", &x, Some("a")));
        assert_eq!(trie.len(), 1);
        assert_eq!(upstream_for(&trie, "x.com", "/p").as_deref(), Some("u2"));
        assert!(trie.remove_exact("/p", &x, None));
        assert!(trie.is_empty());
        assert!(trie.match_path("x.com", "/p").is_none());
    }

    #[test]
    fn node_key_names_the_node_a_path_is_stored_at() {
        assert_eq!(node_key("/x"), node_key("/x/"));
        assert_eq!(node_key("//x//"), "/x");
        assert_eq!(node_key("/users/:id"), node_key("/users/:uid"));
        assert_eq!(node_key("/billing/*"), node_key("/billing/*rest"));
        assert_eq!(node_key("/billing/*rest/ignored"), "/billing/*");
        assert_eq!(node_key("/"), "/");
        assert_eq!(node_key(""), "/");
        assert_ne!(node_key("/x"), node_key("/x/:id"));
        assert_ne!(node_key("/billing"), node_key("/billing/*"));
        assert_ne!(node_key("/a/:b"), node_key("/a/b"));
    }

    #[test]
    fn paths_with_one_node_key_share_a_node() {
        // The trie's uniqueness check is per node, so an equal node key with
        // an equal host and id is a duplicate whatever the path spelling.
        for (a, b) in [
            ("/x", "/x/"),
            ("/users/:id", "/users/:uid"),
            ("/billing/*", "/billing/*rest"),
        ] {
            assert_eq!(node_key(a), node_key(b));
            let mut trie = RouteTrie::new();
            trie.insert(route_o(
                a,
                "u1",
                HostMatch::Any,
                None,
                RouteOrigin::Config,
                0,
            ))
            .unwrap();
            assert!(
                trie.insert(route_o(
                    b,
                    "u2",
                    HostMatch::Any,
                    None,
                    RouteOrigin::Config,
                    0
                ))
                .is_err(),
                "{a} and {b} share a node"
            );
            assert!(trie.remove_exact(b, &HostMatch::Any, None));
            assert!(trie.is_empty());
        }
    }
}
