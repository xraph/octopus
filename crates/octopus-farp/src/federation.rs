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
use farp::merger::{
    Merger, MergerConfig, ServiceSchema, MergeResult, OpenAPISpec,
    AsyncAPIMerger, AsyncAPIServiceSchema, AsyncAPISpec,
};
use farp::manifest::new_manifest;

/// Schema federation engine
///
/// Combines schemas from multiple services into a unified schema.
#[derive(Debug, Clone)]
pub struct SchemaFederation {
    /// Federated schemas by format
    federated: Arc<DashMap<SchemaFormat, FederatedSchema>>,
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
    #[must_use] pub fn new() -> Self {
        Self {
            federated: Arc::new(DashMap::new()),
        }
    }

    /// Federate schemas from multiple services
    pub fn federate_schemas(&self, schemas: Vec<SchemaDescriptor>) -> Result<()> {
        // Group by format
        let mut by_format: std::collections::HashMap<SchemaFormat, Vec<SchemaDescriptor>> =
            std::collections::HashMap::new();

        for schema in schemas {
            by_format
                .entry(schema.format)
                .or_default()
                .push(schema);
        }

        // Federate each format
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
                let schema_value = serde_json::from_str(&desc.content).ok()?;
                
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
        let config = MergerConfig {
            merged_title: "Federated API".to_string(),
            merged_description: "Combined API specification from multiple services".to_string(),
            merged_version: "1.0.0".to_string(),
            include_service_tags: true,
            ..Default::default()
        };

        let merger = Merger::new(config);
        
        // Perform the merge
        let merge_result = merger.merge(service_schemas)
            .map_err(|e| Error::Farp(format!("Failed to merge OpenAPI schemas: {}", e)))?;

        // Convert MergeResult to our FederatedSchema
        let content = serde_json::to_string_pretty(&serde_json::to_value(&merge_result.spec).unwrap())
            .map_err(|e| Error::Farp(format!("Failed to serialize merged schema: {}", e)))?;

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
                let mut manifest = new_manifest(
                    desc.service.clone(),
                    desc.version.clone(),
                    desc.id.clone(),
                );
                
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
            return Err(Error::Farp("No valid AsyncAPI schemas to merge".to_string()));
        }

        // Configure the merger
        let config = MergerConfig {
            merged_title: "Federated Async API".to_string(),
            merged_description: "Combined AsyncAPI specification from multiple services".to_string(),
            merged_version: "1.0.0".to_string(),
            include_service_tags: true,
            ..Default::default()
        };

        let merger = AsyncAPIMerger::new(config);
        
        // Perform the merge
        let merge_result = merger.merge(service_schemas)
            .map_err(|e| Error::Farp(format!("Failed to merge AsyncAPI schemas: {}", e)))?;

        // Convert AsyncAPIMergeResult to our FederatedSchema
        let content = serde_json::to_string_pretty(&serde_json::to_value(&merge_result.spec).unwrap())
            .map_err(|e| Error::Farp(format!("Failed to serialize merged schema: {}", e)))?;

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
    #[must_use] pub fn list_formats(&self) -> Vec<SchemaFormat> {
        self.federated
            .iter()
            .map(|entry| *entry.key())
            .collect()
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
