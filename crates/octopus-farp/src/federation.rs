//! Schema federation engine for combining multiple service schemas

use crate::schema::{SchemaDescriptor, SchemaFormat};
use dashmap::DashMap;
use octopus_core::{Error, Result};
use serde_json::Value;
use std::sync::Arc;

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
    pub fn new() -> Self {
        Self {
            federated: Arc::new(DashMap::new()),
        }
    }

    /// Federate schemas from multiple services
    pub fn federate_schemas(
        &self,
        schemas: Vec<SchemaDescriptor>,
    ) -> Result<()> {
        // Group by format
        let mut by_format: std::collections::HashMap<SchemaFormat, Vec<SchemaDescriptor>> =
            std::collections::HashMap::new();

        for schema in schemas {
            by_format
                .entry(schema.format.clone())
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

    /// Federate OpenAPI schemas
    fn federate_openapi(&self, schemas: Vec<SchemaDescriptor>) -> Result<()> {
        let mut combined = serde_json::json!({
            "openapi": "3.0.0",
            "info": {
                "title": "Federated API",
                "version": "1.0.0"
            },
            "paths": {},
            "components": {
                "schemas": {}
            }
        });

        let sources: Vec<String> = schemas.iter().map(|s| s.service.clone()).collect();

        for schema in schemas {
            if let Ok(spec) = serde_json::from_str::<Value>(&schema.content) {
                // Merge paths
                if let Some(paths) = spec.get("paths").and_then(|p| p.as_object()) {
                    let combined_paths = combined["paths"].as_object_mut().unwrap();
                    for (path, methods) in paths {
                        // Add service prefix to avoid conflicts
                        let prefixed_path = format!("/{}{}", schema.service, path);
                        combined_paths.insert(prefixed_path, methods.clone());
                    }
                }

                // Merge components/schemas
                if let Some(components) = spec.get("components") {
                    if let Some(comp_schemas) = components.get("schemas").and_then(|s| s.as_object()) {
                        let combined_schemas = combined["components"]["schemas"]
                            .as_object_mut()
                            .unwrap();
                        for (name, comp_schema) in comp_schemas {
                            // Prefix schema names to avoid conflicts
                            let prefixed_name = format!("{}_{}", schema.service, name);
                            combined_schemas.insert(prefixed_name, comp_schema.clone());
                        }
                    }
                }
            }
        }

        let federated = FederatedSchema {
            format: SchemaFormat::OpenApi,
            content: serde_json::to_string_pretty(&combined).unwrap(),
            sources,
            updated_at: std::time::SystemTime::now(),
        };

        self.federated.insert(SchemaFormat::OpenApi, federated);
        Ok(())
    }

    /// Federate AsyncAPI schemas
    fn federate_asyncapi(&self, schemas: Vec<SchemaDescriptor>) -> Result<()> {
        let mut combined = serde_json::json!({
            "asyncapi": "2.0.0",
            "info": {
                "title": "Federated Async API",
                "version": "1.0.0"
            },
            "channels": {}
        });

        let sources: Vec<String> = schemas.iter().map(|s| s.service.clone()).collect();

        for schema in schemas {
            if let Ok(spec) = serde_json::from_str::<Value>(&schema.content) {
                if let Some(channels) = spec.get("channels").and_then(|c| c.as_object()) {
                    let combined_channels = combined["channels"].as_object_mut().unwrap();
                    for (channel, def) in channels {
                        // Add service prefix
                        let prefixed_channel = format!("{}.{}", schema.service, channel);
                        combined_channels.insert(prefixed_channel, def.clone());
                    }
                }
            }
        }

        let federated = FederatedSchema {
            format: SchemaFormat::AsyncApi,
            content: serde_json::to_string_pretty(&combined).unwrap(),
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

        self.federated.insert(format.clone(), federated);
        Ok(())
    }

    /// Get federated schema for a format
    pub fn get_federated(&self, format: &SchemaFormat) -> Result<FederatedSchema> {
        self.federated
            .get(format)
            .map(|f| f.clone())
            .ok_or_else(|| Error::Farp(format!("No federated schema for format: {:?}", format)))
    }

    /// List all federated formats
    pub fn list_formats(&self) -> Vec<SchemaFormat> {
        self.federated.iter().map(|entry| entry.key().clone()).collect()
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

        let schemas = vec![
            SchemaDescriptor {
                id: "service1-schema".to_string(),
                service: "service1".to_string(),
                format: SchemaFormat::OpenApi,
                version: "1.0.0".to_string(),
                content: r#"{"openapi":"3.0.0","paths":{"/users":{}}}"#.to_string(),
                checksum: None,
            },
        ];

        federation.federate_schemas(schemas).unwrap();

        let federated = federation.get_federated(&SchemaFormat::OpenApi).unwrap();
        assert!(federated.content.contains("openapi"));
    }
}

