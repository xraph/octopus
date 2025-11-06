//! Route definition and builder

use http::Method;
use octopus_core::{Error, Result};
use std::collections::HashMap;

/// Route definition
#[derive(Debug, Clone)]
pub struct Route {
    /// HTTP method
    pub method: Method,
    
    /// Path pattern (e.g., "/users/:id")
    pub path: String,
    
    /// Upstream cluster name
    pub upstream_name: String,
    
    /// Priority (higher = matched first)
    pub priority: i32,
    
    /// Route metadata
    pub metadata: HashMap<String, String>,
    
    /// Path prefix to strip before forwarding
    pub strip_prefix: Option<String>,
    
    /// Path prefix to add before forwarding
    pub add_prefix: Option<String>,
}

impl Route {
    /// Create a new route builder
    pub fn builder() -> RouteBuilder {
        RouteBuilder::new()
    }
}

/// Builder for constructing routes
#[derive(Debug, Default)]
pub struct RouteBuilder {
    method: Option<Method>,
    path: Option<String>,
    upstream_name: Option<String>,
    priority: i32,
    metadata: HashMap<String, String>,
    strip_prefix: Option<String>,
    add_prefix: Option<String>,
}

impl RouteBuilder {
    /// Create a new route builder
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the HTTP method
    pub fn method(mut self, method: Method) -> Self {
        self.method = Some(method);
        self
    }

    /// Set the path pattern
    pub fn path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// Set the upstream cluster name
    pub fn upstream_name(mut self, name: impl Into<String>) -> Self {
        self.upstream_name = Some(name.into());
        self
    }

    /// Set the priority
    pub fn priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    /// Add metadata
    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Set strip prefix
    pub fn strip_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.strip_prefix = Some(prefix.into());
        self
    }

    /// Set add prefix
    pub fn add_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.add_prefix = Some(prefix.into());
        self
    }

    /// Build the route
    pub fn build(self) -> Result<Route> {
        let method = self.method
            .ok_or_else(|| Error::Config("method is required".to_string()))?;
        
        let path = self.path
            .ok_or_else(|| Error::Config("path is required".to_string()))?;
        
        let upstream_name = self.upstream_name
            .ok_or_else(|| Error::Config("upstream_name is required".to_string()))?;

        // Validate path
        if !path.starts_with('/') {
            return Err(Error::Config("path must start with '/'".to_string()));
        }

        Ok(Route {
            method,
            path,
            upstream_name,
            priority: self.priority,
            metadata: self.metadata,
            strip_prefix: self.strip_prefix,
            add_prefix: self.add_prefix,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_route_builder() {
        let route = RouteBuilder::new()
            .method(Method::GET)
            .path("/users/:id")
            .upstream_name("user-service")
            .priority(10)
            .metadata("version", "v1")
            .build()
            .unwrap();

        assert_eq!(route.method, Method::GET);
        assert_eq!(route.path, "/users/:id");
        assert_eq!(route.upstream_name, "user-service");
        assert_eq!(route.priority, 10);
        assert_eq!(route.metadata.get("version"), Some(&"v1".to_string()));
    }

    #[test]
    fn test_route_builder_missing_fields() {
        let result = RouteBuilder::new()
            .path("/users")
            .build();

        assert!(result.is_err());
    }

    #[test]
    fn test_route_builder_invalid_path() {
        let result = RouteBuilder::new()
            .method(Method::GET)
            .path("users") // Missing leading slash
            .upstream_name("service")
            .build();

        assert!(result.is_err());
    }

    #[test]
    fn test_route_with_prefix_operations() {
        let route = RouteBuilder::new()
            .method(Method::GET)
            .path("/api/users/*")
            .upstream_name("user-service")
            .strip_prefix("/api")
            .add_prefix("/v1")
            .build()
            .unwrap();

        assert_eq!(route.strip_prefix, Some("/api".to_string()));
        assert_eq!(route.add_prefix, Some("/v1".to_string()));
    }
}


