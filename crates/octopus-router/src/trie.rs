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
    
    /// Route at this node (if terminal)
    route: Option<Route>,
    
    /// Path matcher for this node
    matcher: Option<PathMatcher>,
}

impl TrieNode {
    fn new() -> Self {
        Self {
            children: HashMap::new(),
            param_child: None,
            wildcard_child: None,
            route: None,
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
        
        // Store route and matcher at terminal node
        if current.route.is_some() {
            return Err(Error::Config(format!(
                "Route already exists: {}",
                route.path
            )));
        }
        
        current.matcher = Some(PathMatcher::new(route.path.clone()));
        current.route = Some(route);
        self.count += 1;
        
        Ok(())
    }

    /// Remove a route from the trie
    pub fn remove(&mut self, path: &str) -> Result<()> {
        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        
        if Self::remove_recursive(&mut self.root, &segments, 0) {
            self.count -= 1;
            Ok(())
        } else {
            Err(Error::RouteNotFound(path.to_string()))
        }
    }

    fn remove_recursive(node: &mut TrieNode, segments: &[&str], index: usize) -> bool {
        if index == segments.len() {
            // Reached end of path
            if node.route.is_some() {
                node.route = None;
                node.matcher = None;
                return true;
            }
            return false;
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
        
        false
    }

    /// Match a path against routes in the trie
    pub fn match_path(&self, path: &str) -> Option<Match> {
        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        
        let mut matches = Vec::new();
        self.match_recursive(&self.root, &segments, 0, &mut matches);
        
        // Return highest priority match
        matches.sort_by(|a, b| b.route.priority.cmp(&a.route.priority));
        matches.into_iter().next()
    }

    fn match_recursive(
        &self,
        node: &TrieNode,
        segments: &[&str],
        index: usize,
        matches: &mut Vec<Match>,
    ) {
        if index == segments.len() {
            // Reached end of path, check if there's a route here
            if let Some(route) = &node.route {
                if let Some(matcher) = &node.matcher {
                    let path = format!("/{}", segments.join("/"));
                    if let Some(params) = matcher.matches(&path) {
                        matches.push(Match {
                            route: route.clone(),
                            params,
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
            self.match_recursive(child, segments, index + 1, matches);
        }
        
        // Try parameter match
        if let Some(ref child) = node.param_child {
            self.match_recursive(child, segments, index + 1, matches);
        }
        
        // Try wildcard match (lowest priority)
        if let Some(ref child) = node.wildcard_child {
            if let Some(route) = &child.route {
                if let Some(matcher) = &child.matcher {
                    let path = format!("/{}", segments.join("/"));
                    if let Some(params) = matcher.matches(&path) {
                        matches.push(Match {
                            route: route.clone(),
                            params,
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
        self.collect_routes(&self.root, &mut routes);
        routes
    }

    fn collect_routes(&self, node: &TrieNode, routes: &mut Vec<Route>) {
        if let Some(ref route) = node.route {
            routes.push(route.clone());
        }
        
        for child in node.children.values() {
            self.collect_routes(child, routes);
        }
        
        if let Some(ref child) = node.param_child {
            self.collect_routes(child, routes);
        }
        
        if let Some(ref child) = node.wildcard_child {
            self.collect_routes(child, routes);
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
    use crate::RouteBuilder;
    use http::Method;

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
        
        let matched = trie.match_path("/users").unwrap();
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
        
        let matched = trie.match_path("/users/123").unwrap();
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
        let matched = trie.match_path("/api/users").unwrap();
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
        
        assert!(trie.match_path("/users/123").is_none());
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
            trie.match_path("/api/users").unwrap().route.upstream_name,
            "users-static"
        );
        assert_eq!(
            trie.match_path("/api/users/123").unwrap().route.upstream_name,
            "users-detail"
        );
        assert_eq!(
            trie.match_path("/api/users/123/posts").unwrap().route.upstream_name,
            "user-posts"
        );
        assert_eq!(
            trie.match_path("/static/css/main.css").unwrap().route.upstream_name,
            "static-files"
        );
    }
}


