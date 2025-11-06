//! Dynamic route generation from service schemas

use crate::schema::{SchemaDescriptor, SchemaFormat};
use octopus_core::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Route generator for creating routes from schemas
#[derive(Debug, Clone)]
pub struct RouteGenerator;

/// Generated route information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedRoute {
    /// HTTP method
    pub method: String,
    
    /// Path pattern
    pub path: String,
    
    /// Upstream service name
    pub upstream: String,
    
    /// Route metadata
    pub metadata: RouteMetadata,
}

/// Route metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteMetadata {
    /// Operation ID (if available)
    pub operation_id: Option<String>,
    
    /// Summary/description
    pub summary: Option<String>,
    
    /// Tags
    pub tags: Vec<String>,
    
    /// Required authentication
    pub requires_auth: bool,
    
    /// Rate limit (requests per second)
    pub rate_limit: Option<u32>,
}

impl Default for RouteMetadata {
    fn default() -> Self {
        Self {
            operation_id: None,
            summary: None,
            tags: Vec::new(),
            requires_auth: false,
            rate_limit: None,
        }
    }
}

impl RouteGenerator {
    /// Create a new route generator
    pub fn new() -> Self {
        Self
    }

    /// Generate routes from a schema
    pub fn generate_routes(&self, schema: &SchemaDescriptor) -> Result<Vec<GeneratedRoute>> {
        match schema.format {
            SchemaFormat::OpenApi => self.generate_from_openapi(schema),
            SchemaFormat::AsyncApi => self.generate_from_asyncapi(schema),
            SchemaFormat::GraphQL => self.generate_from_graphql(schema),
            _ => Err(Error::Farp(format!(
                "Unsupported schema format for route generation: {:?}",
                schema.format
            ))),
        }
    }

    /// Generate routes from OpenAPI schema
    fn generate_from_openapi(&self, schema: &SchemaDescriptor) -> Result<Vec<GeneratedRoute>> {
        let spec: Value = serde_json::from_str(&schema.content)
            .map_err(|e| Error::Farp(format!("Invalid OpenAPI schema: {}", e)))?;

        let mut routes = Vec::new();

        if let Some(paths) = spec.get("paths").and_then(|p| p.as_object()) {
            for (path, methods) in paths {
                if let Some(methods_obj) = methods.as_object() {
                    for (method, operation) in methods_obj {
                        // Skip non-HTTP methods
                        if !["get", "post", "put", "delete", "patch", "options", "head"].contains(&method.as_str()) {
                            continue;
                        }

                        let metadata = self.extract_openapi_metadata(operation);

                        routes.push(GeneratedRoute {
                            method: method.to_uppercase(),
                            path: path.clone(),
                            upstream: schema.service.clone(),
                            metadata,
                        });
                    }
                }
            }
        }

        Ok(routes)
    }

    /// Extract metadata from OpenAPI operation
    fn extract_openapi_metadata(&self, operation: &Value) -> RouteMetadata {
        RouteMetadata {
            operation_id: operation.get("operationId")
                .and_then(|v| v.as_str())
                .map(String::from),
            summary: operation.get("summary")
                .and_then(|v| v.as_str())
                .map(String::from),
            tags: operation.get("tags")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            requires_auth: operation.get("security").is_some(),
            rate_limit: operation.get("x-rate-limit")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32),
        }
    }

    /// Generate routes from AsyncAPI schema
    fn generate_from_asyncapi(&self, schema: &SchemaDescriptor) -> Result<Vec<GeneratedRoute>> {
        let spec: Value = serde_json::from_str(&schema.content)
            .map_err(|e| Error::Farp(format!("Invalid AsyncAPI schema: {}", e)))?;

        let mut routes = Vec::new();

        if let Some(channels) = spec.get("channels").and_then(|c| c.as_object()) {
            for (channel, definition) in channels {
                // For AsyncAPI, create WebSocket/SSE routes
                let metadata = RouteMetadata {
                    summary: definition.get("description")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    ..Default::default()
                };

                routes.push(GeneratedRoute {
                    method: "GET".to_string(), // WebSocket upgrade
                    path: format!("/ws{}", channel),
                    upstream: schema.service.clone(),
                    metadata,
                });
            }
        }

        Ok(routes)
    }

    /// Generate routes from GraphQL schema
    fn generate_from_graphql(&self, schema: &SchemaDescriptor) -> Result<Vec<GeneratedRoute>> {
        // For GraphQL, create a single endpoint
        Ok(vec![
            GeneratedRoute {
                method: "POST".to_string(),
                path: "/graphql".to_string(),
                upstream: schema.service.clone(),
                metadata: RouteMetadata {
                    summary: Some("GraphQL endpoint".to_string()),
                    tags: vec!["graphql".to_string()],
                    ..Default::default()
                },
            },
            GeneratedRoute {
                method: "GET".to_string(),
                path: "/graphql".to_string(),
                upstream: schema.service.clone(),
                metadata: RouteMetadata {
                    summary: Some("GraphQL playground".to_string()),
                    tags: vec!["graphql".to_string()],
                    ..Default::default()
                },
            },
        ])
    }

    /// Apply prefix to all routes
    pub fn apply_prefix(routes: Vec<GeneratedRoute>, prefix: &str) -> Vec<GeneratedRoute> {
        routes
            .into_iter()
            .map(|mut route| {
                route.path = format!("{}{}", prefix, route.path);
                route
            })
            .collect()
    }

    /// Filter routes by tags
    pub fn filter_by_tags(routes: Vec<GeneratedRoute>, tags: &[String]) -> Vec<GeneratedRoute> {
        routes
            .into_iter()
            .filter(|route| {
                route.metadata.tags.iter().any(|t| tags.contains(t))
            })
            .collect()
    }
}

impl Default for RouteGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openapi_route_generation() {
        let generator = RouteGenerator::new();

        let schema = SchemaDescriptor {
            id: "users-schema".to_string(),
            service: "users".to_string(),
            format: SchemaFormat::OpenApi,
            version: "1.0.0".to_string(),
            content: r#"{
                "openapi": "3.0.0",
                "paths": {
                    "/users": {
                        "get": {
                            "operationId": "listUsers",
                            "summary": "List all users"
                        },
                        "post": {
                            "operationId": "createUser",
                            "summary": "Create a user"
                        }
                    }
                }
            }"#.to_string(),
            checksum: None,
        };

        let routes = generator.generate_routes(&schema).unwrap();
        assert_eq!(routes.len(), 2);
        assert_eq!(routes[0].path, "/users");
        assert!(routes[0].method == "GET" || routes[0].method == "POST");
    }

    #[test]
    fn test_apply_prefix() {
        let routes = vec![GeneratedRoute {
            method: "GET".to_string(),
            path: "/users".to_string(),
            upstream: "test".to_string(),
            metadata: RouteMetadata::default(),
        }];

        let prefixed = RouteGenerator::apply_prefix(routes, "/api/v1");
        assert_eq!(prefixed[0].path, "/api/v1/users");
    }

    #[test]
    fn test_graphql_route_generation() {
        let generator = RouteGenerator::new();

        let schema = SchemaDescriptor {
            id: "graphql-schema".to_string(),
            service: "graphql".to_string(),
            format: SchemaFormat::GraphQL,
            version: "1.0.0".to_string(),
            content: "type Query { users: [User] }".to_string(),
            checksum: None,
        };

        let routes = generator.generate_routes(&schema).unwrap();
        assert_eq!(routes.len(), 2); // POST and GET
        assert_eq!(routes[0].path, "/graphql");
    }
}

