//! Schema federation engine for combining multiple service schemas
//!
//! This module wraps the external farp merger functionality to provide
//! Octopus-specific federation features.

use crate::schema::{SchemaDescriptor, SchemaFormat};
use dashmap::DashMap;
use octopus_core::{Error, Result};
use sha2::Digest;
use std::sync::Arc;

// Import external merger types
use farp::manifest::new_manifest;
use farp::merger::{AsyncAPIMerger, AsyncAPIServiceSchema, Merger, MergerConfig, ServiceSchema};

/// Schema federation engine
///
/// Combines schemas from multiple services into a unified schema.
#[derive(Debug, Clone)]
pub struct SchemaFederation {
    /// Federated schemas by format
    federated: Arc<DashMap<SchemaFormat, FederatedSchema>>,
    /// Whether to collapse all service tags into a single service-name tag
    collapse_service_tags: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

/// Federated schema
#[derive(Debug, Clone)]
pub struct FederatedSchema {
    /// Schema format
    pub format: SchemaFormat,

    /// Combined schema content
    pub content: String,

    /// Source schemas
    pub sources: Vec<String>, // service names

    /// Last updated
    pub updated_at: std::time::SystemTime,
}

impl SchemaFederation {
    /// Create a new schema federation engine
    #[must_use]
    pub fn new() -> Self {
        Self {
            federated: Arc::new(DashMap::new()),
            collapse_service_tags: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Set whether to collapse service tags into a single tag per service.
    pub fn set_collapse_service_tags(&self, collapse: bool) {
        self.collapse_service_tags
            .store(collapse, std::sync::atomic::Ordering::Relaxed);
    }

    /// Returns whether service tags should be collapsed.
    #[must_use]
    pub fn collapse_service_tags(&self) -> bool {
        self.collapse_service_tags
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Federate schemas from multiple services
    pub fn federate_schemas(&self, schemas: Vec<SchemaDescriptor>) -> Result<()> {
        // Group by format
        let mut by_format: std::collections::HashMap<SchemaFormat, Vec<SchemaDescriptor>> =
            std::collections::HashMap::new();

        for schema in schemas {
            by_format.entry(schema.format).or_default().push(schema);
        }

        // Federate each format
        for (format, schemas) in &by_format {
            tracing::info!(format = ?format, count = schemas.len(), "Federating schemas for format");
        }

        for (format, schemas) in by_format {
            match format {
                SchemaFormat::OpenApi => {
                    self.federate_openapi(schemas)?;
                }
                SchemaFormat::AsyncApi => {
                    self.federate_asyncapi(schemas)?;
                }
                SchemaFormat::GraphQL => {
                    self.federate_graphql(schemas)?;
                }
                _ => {
                    // For other formats, just combine
                    self.combine_schemas(format, schemas)?;
                }
            }
        }

        Ok(())
    }

    /// Federate `OpenAPI` schemas using external farp merger
    fn federate_openapi(&self, schemas: Vec<SchemaDescriptor>) -> Result<()> {
        let sources: Vec<String> = schemas.iter().map(|s| s.service.clone()).collect();

        // Convert SchemaDescriptor to ServiceSchema for farp merger
        let service_schemas: Vec<ServiceSchema> = schemas
            .into_iter()
            .filter_map(|desc| {
                // Parse the schema content
                let mut schema_value: serde_json::Value = serde_json::from_str(&desc.content).ok()?;

                // Filter out standard introspection endpoints by default.
                // Set FARP_INCLUDE_INTROSPECTION_ENDPOINTS=1 to include them.
                if crate::schema_ops::should_exclude_introspection() {
                    if let Some(obj) = schema_value.as_object_mut() {
                        if let Some(paths) = obj.get_mut("paths").and_then(|p| p.as_object_mut()) {
                            let before_count = paths.len();
                            let to_remove: Vec<String> = paths.keys()
                                .filter(|p| crate::schema_ops::is_introspection_path(p.as_str()))
                                .cloned()
                                .collect();

                            for p in &to_remove {
                                paths.remove(p);
                            }

                            if !to_remove.is_empty() {
                                tracing::debug!(
                                    service = %desc.service,
                                    removed = to_remove.len(),
                                    remaining = paths.len(),
                                    total = before_count,
                                    "Filtered introspection endpoints from schema"
                                );
                            }

                            if paths.is_empty() && before_count > 0 {
                                tracing::warn!(
                                    service = %desc.service,
                                    "All paths were introspection endpoints and were filtered out. \
                                     The service's OpenAPI spec contains no business API operations. \
                                     Routes from the gateway router will be used as a fallback."
                                );
                            }
                        }
                    }
                }

                // Log the paths that will be federated
                if let Some(paths) = schema_value.get("paths").and_then(|p| p.as_object()) {
                    let path_names: Vec<&String> = paths.keys().collect();
                    tracing::debug!(
                        service = %desc.service,
                        path_count = paths.len(),
                        paths = ?path_names,
                        "Paths to federate for service"
                    );
                }

                // Parse into OpenAPISpec (optional - merger can work without it)
                let parsed = farp::merger::parse_openapi_schema(&schema_value).ok();

                // Create a manifest for the merger with OpenAPI schema descriptor
                // The merger needs manifests with schemas to determine inclusion
                let mut manifest = new_manifest(
                    desc.service.clone(),
                    desc.version.clone(),
                    desc.id.clone(),
                );

                // Add a schema descriptor so the merger includes this service
                // Use placeholder hash from content
                let mut hasher = sha2::Sha256::new();
                hasher.update(desc.content.as_bytes());
                let hash = format!("{:x}", hasher.finalize());

                // Use external farp types for the schema descriptor
                let location = farp::types::SchemaLocation {
                    location_type: farp::types::LocationType::Inline,
                    url: None,
                    registry_path: None,
                    headers: None,
                };

                let schema_desc = farp::types::SchemaDescriptor {
                    schema_type: farp::types::SchemaType::OpenAPI,
                    spec_version: desc.version.clone(),
                    location,
                    content_type: "application/json".to_string(),
                    inline_schema: Some(schema_value.clone()),
                    hash,
                    size: desc.content.len() as i64,
                    compatibility: None,
                    metadata: None,
                };

                manifest.schemas.push(schema_desc);

                Some(ServiceSchema {
                    manifest,
                    schema: schema_value,
                    parsed,
                })
            })
            .collect();

        if service_schemas.is_empty() {
            return Err(Error::Farp("No valid OpenAPI schemas to merge".to_string()));
        }

        // Configure the merger
        // Collapse can be set via:
        // 1. FARP_COLLAPSE_SERVICE_TAGS=1 env var (gateway-wide)
        // 2. Service metadata "farp.collapse_service_tags" = "true" (per-service, via trigger_federation)
        let collapse =
            self.collapse_service_tags() || std::env::var("FARP_COLLAPSE_SERVICE_TAGS").is_ok();
        let config = MergerConfig {
            merged_title: "Federated API".to_string(),
            merged_description: "Combined API specification from multiple services".to_string(),
            merged_version: "1.0.0".to_string(),
            include_service_tags: true,
            collapse_service_tags: collapse,
            ..Default::default()
        };

        let merger = Merger::new(config);

        // Perform the merge
        let merge_result = merger
            .merge(service_schemas)
            .map_err(|e| Error::Farp(format!("Failed to merge OpenAPI schemas: {e}")))?;

        // Convert MergeResult to JSON, then post-process to set gateway server
        let mut spec_value = serde_json::to_value(&merge_result.spec)
            .map_err(|e| Error::Farp(format!("Failed to serialize merged schema: {e}")))?;

        // Set the servers array to point to the gateway root.
        // Without this, Swagger UI defaults to the fetch URL (/farp/openapi.json)
        // as the base, causing all "Try it out" requests to fail.
        if let Some(obj) = spec_value.as_object_mut() {
            obj.insert(
                "servers".to_string(),
                serde_json::json!([
                    {
                        "url": "/",
                        "description": "Gateway"
                    }
                ]),
            );
        }

        // Log the merged spec path count for debugging
        let path_count = spec_value
            .get("paths")
            .and_then(|p| p.as_object())
            .map_or(0, serde_json::Map::len);
        tracing::info!(
            path_count = path_count,
            services = ?sources,
            "Federated OpenAPI spec built"
        );

        let content = serde_json::to_string_pretty(&spec_value)
            .map_err(|e| Error::Farp(format!("Failed to serialize merged schema: {e}")))?;

        let federated = FederatedSchema {
            format: SchemaFormat::OpenApi,
            content,
            sources,
            updated_at: std::time::SystemTime::now(),
        };

        self.federated.insert(SchemaFormat::OpenApi, federated);
        Ok(())
    }

    /// Federate `AsyncAPI` schemas using external farp merger
    fn federate_asyncapi(&self, schemas: Vec<SchemaDescriptor>) -> Result<()> {
        let sources: Vec<String> = schemas.iter().map(|s| s.service.clone()).collect();

        // Convert SchemaDescriptor to AsyncAPIServiceSchema for farp merger
        let service_schemas: Vec<AsyncAPIServiceSchema> = schemas
            .into_iter()
            .filter_map(|desc| {
                // Parse the schema content
                let schema_value = serde_json::from_str(&desc.content).ok()?;

                // Parse into AsyncAPISpec
                let parsed = farp::merger::parse_asyncapi_schema(&schema_value).ok();

                // Create a manifest for the merger with AsyncAPI schema descriptor
                let mut manifest =
                    new_manifest(desc.service.clone(), desc.version.clone(), desc.id.clone());

                // Add a schema descriptor so the merger includes this service
                let mut hasher = sha2::Sha256::new();
                hasher.update(desc.content.as_bytes());
                let hash = format!("{:x}", hasher.finalize());

                // Use external farp types for the schema descriptor
                let location = farp::types::SchemaLocation {
                    location_type: farp::types::LocationType::Inline,
                    url: None,
                    registry_path: None,
                    headers: None,
                };

                let schema_desc = farp::types::SchemaDescriptor {
                    schema_type: farp::types::SchemaType::AsyncAPI,
                    spec_version: desc.version.clone(),
                    location,
                    content_type: "application/json".to_string(),
                    inline_schema: Some(schema_value.clone()),
                    hash,
                    size: desc.content.len() as i64,
                    compatibility: None,
                    metadata: None,
                };

                manifest.schemas.push(schema_desc);

                Some(AsyncAPIServiceSchema {
                    manifest,
                    schema: schema_value,
                    parsed,
                })
            })
            .collect();

        if service_schemas.is_empty() {
            return Err(Error::Farp(
                "No valid AsyncAPI schemas to merge".to_string(),
            ));
        }

        // Configure the merger
        let config = MergerConfig {
            merged_title: "Federated Async API".to_string(),
            merged_description: "Combined AsyncAPI specification from multiple services"
                .to_string(),
            merged_version: "1.0.0".to_string(),
            include_service_tags: true,
            ..Default::default()
        };

        let merger = AsyncAPIMerger::new(config);

        // Perform the merge
        let merge_result = merger
            .merge(service_schemas)
            .map_err(|e| Error::Farp(format!("Failed to merge AsyncAPI schemas: {e}")))?;

        // Convert AsyncAPIMergeResult to our FederatedSchema
        let content =
            serde_json::to_string_pretty(&serde_json::to_value(&merge_result.spec).unwrap())
                .map_err(|e| Error::Farp(format!("Failed to serialize merged schema: {e}")))?;

        let federated = FederatedSchema {
            format: SchemaFormat::AsyncApi,
            content,
            sources,
            updated_at: std::time::SystemTime::now(),
        };

        self.federated.insert(SchemaFormat::AsyncApi, federated);
        Ok(())
    }

    /// Federate GraphQL schemas
    fn federate_graphql(&self, schemas: Vec<SchemaDescriptor>) -> Result<()> {
        // For GraphQL, combine all schemas
        let mut combined_schema = String::from("# Federated GraphQL Schema\n\n");

        let sources: Vec<String> = schemas.iter().map(|s| s.service.clone()).collect();

        for schema in schemas {
            combined_schema.push_str(&format!("# From service: {}\n", schema.service));
            combined_schema.push_str(&schema.content);
            combined_schema.push_str("\n\n");
        }

        let federated = FederatedSchema {
            format: SchemaFormat::GraphQL,
            content: combined_schema,
            sources,
            updated_at: std::time::SystemTime::now(),
        };

        self.federated.insert(SchemaFormat::GraphQL, federated);
        Ok(())
    }

    /// Combine schemas (fallback for unknown formats)
    fn combine_schemas(&self, format: SchemaFormat, schemas: Vec<SchemaDescriptor>) -> Result<()> {
        let mut combined = String::new();
        let sources: Vec<String> = schemas.iter().map(|s| s.service.clone()).collect();

        for schema in schemas {
            combined.push_str(&format!("# Service: {}\n", schema.service));
            combined.push_str(&schema.content);
            combined.push_str("\n\n");
        }

        let federated = FederatedSchema {
            format,
            content: combined,
            sources,
            updated_at: std::time::SystemTime::now(),
        };

        self.federated.insert(format, federated);
        Ok(())
    }

    /// Get federated schema for a format
    pub fn get_federated(&self, format: &SchemaFormat) -> Result<FederatedSchema> {
        self.federated
            .get(format)
            .map(|f| f.clone())
            .ok_or_else(|| Error::Farp(format!("No federated schema for format: {format:?}")))
    }

    /// List all federated formats
    #[must_use]
    pub fn list_formats(&self) -> Vec<SchemaFormat> {
        self.federated.iter().map(|entry| *entry.key()).collect()
    }
}

impl Default for SchemaFederation {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_federation() {
        let federation = SchemaFederation::new();

        let schemas = vec![SchemaDescriptor {
            id: "service1-schema".to_string(),
            service: "service1".to_string(),
            format: SchemaFormat::OpenApi,
            version: "1.0.0".to_string(),
            content: r#"{"openapi":"3.0.0","info":{"title":"Test API","version":"1.0.0"},"paths":{"/users":{}}}"#.to_string(),
            checksum: Some("test-checksum".to_string()),
        }];

        federation.federate_schemas(schemas).unwrap();

        let federated = federation.get_federated(&SchemaFormat::OpenApi).unwrap();
        assert!(federated.content.contains("openapi"));
        assert!(federated.content.contains("Federated API"));
    }
}
