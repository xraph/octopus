//! Schema manifest validation using JSON Schema
//!
//! Provides validation for FARP v1.0.0 schema manifests

use crate::manifest::SchemaManifest;
use octopus_core::{Error, Result};
use serde_json::json;

#[cfg(feature = "validation")]
use jsonschema::JSONSchema;

/// FARP v1.0.0 Schema Manifest JSON Schema
const MANIFEST_JSON_SCHEMA: &str = r##"{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "title": "FARP Schema Manifest",
  "description": "FARP v1.0.0 Schema Manifest specification",
  "type": "object",
  "required": ["version", "service_name", "instance_id", "schemas", "capabilities", "endpoints", "updated_at", "checksum"],
  "properties": {
    "version": {
      "type": "string",
      "pattern": "^\\d+\\.\\d+\\.\\d+$",
      "description": "Protocol version (semver)"
    },
    "service_name": {
      "type": "string",
      "minLength": 1,
      "description": "Logical service name"
    },
    "service_version": {
      "type": "string",
      "description": "Service version"
    },
    "instance_id": {
      "type": "string",
      "minLength": 1,
      "description": "Unique instance identifier"
    },
    "schemas": {
      "type": "array",
      "items": {
        "$ref": "#/definitions/schemaDescriptor"
      },
      "description": "Schema descriptors"
    },
    "capabilities": {
      "type": "array",
      "items": {
        "type": "string",
        "enum": ["rest", "grpc", "websocket", "sse", "graphql", "mqtt", "amqp", "rpc"]
      },
      "description": "Supported protocols/capabilities"
    },
    "endpoints": {
      "$ref": "#/definitions/schemaEndpoints"
    },
    "updated_at": {
      "type": "integer",
      "minimum": 0,
      "description": "Unix timestamp"
    },
    "checksum": {
      "type": "string",
      "pattern": "^[a-f0-9]{64}$",
      "description": "SHA256 checksum"
    }
  },
  "definitions": {
    "schemaDescriptor": {
      "type": "object",
      "required": ["type", "spec_version", "location", "content_type", "hash", "size"],
      "properties": {
        "type": {
          "type": "string",
          "enum": ["openapi", "asyncapi", "grpc", "graphql", "orpc", "thrift", "avro", "custom"]
        },
        "spec_version": {
          "type": "string"
        },
        "location": {
          "$ref": "#/definitions/schemaLocation"
        },
        "content_type": {
          "type": "string"
        },
        "inline_schema": {
          "description": "Optional inline schema for small schemas"
        },
        "hash": {
          "type": "string",
          "pattern": "^[a-f0-9]{64}$"
        },
        "size": {
          "type": "integer",
          "minimum": 0
        }
      }
    },
    "schemaLocation": {
      "type": "object",
      "required": ["type"],
      "properties": {
        "type": {
          "type": "string",
          "enum": ["http", "registry", "inline"]
        },
        "url": {
          "type": "string",
          "format": "uri"
        },
        "registry_path": {
          "type": "string"
        },
        "headers": {
          "type": "object",
          "additionalProperties": {
            "type": "string"
          }
        }
      },
      "oneOf": [
        {
          "properties": {
            "type": { "const": "http" }
          },
          "required": ["url"]
        },
        {
          "properties": {
            "type": { "const": "registry" }
          },
          "required": ["registry_path"]
        },
        {
          "properties": {
            "type": { "const": "inline" }
          }
        }
      ]
    },
    "schemaEndpoints": {
      "type": "object",
      "required": ["health"],
      "properties": {
        "health": {
          "type": "string"
        },
        "metrics": {
          "type": "string"
        },
        "openapi": {
          "type": "string"
        },
        "asyncapi": {
          "type": "string"
        },
        "grpc_reflection": {
          "type": "boolean"
        },
        "graphql": {
          "type": "string"
        }
      }
    }
  }
}"##;

/// Schema manifest validator
pub struct ManifestValidator {
    #[cfg(feature = "validation")]
    schema: JSONSchema,
}

impl ManifestValidator {
    /// Create a new validator
    pub fn new() -> Result<Self> {
        #[cfg(feature = "validation")]
        {
            let schema_value: serde_json::Value = serde_json::from_str(MANIFEST_JSON_SCHEMA)
                .map_err(|e| Error::Farp(format!("Failed to parse JSON Schema: {}", e)))?;

            let schema = JSONSchema::compile(&schema_value)
                .map_err(|e| Error::Farp(format!("Failed to compile JSON Schema: {}", e)))?;

            Ok(Self { schema })
        }

        #[cfg(not(feature = "validation"))]
        {
            Ok(Self {})
        }
    }

    /// Validate a schema manifest
    pub fn validate(&self, manifest: &SchemaManifest) -> Result<()> {
        #[cfg(feature = "validation")]
        {
            // Convert manifest to JSON value
            let manifest_json = serde_json::to_value(manifest)
                .map_err(|e| Error::Farp(format!("Failed to serialize manifest: {}", e)))?;

            // Validate against schema
            match self.schema.validate(&manifest_json) {
                Ok(_) => Ok(()),
                Err(errors) => {
                    let error_messages: Vec<String> = errors
                        .map(|e| format!("{}: {}", e.instance_path, e))
                        .collect();

                    Err(Error::Farp(format!(
                        "Manifest validation failed:\n  {}",
                        error_messages.join("\n  ")
                    )))
                }
            }
        }

        #[cfg(not(feature = "validation"))]
        {
            // Fallback to basic validation
            manifest.validate()
        }
    }

    /// Validate and provide detailed errors
    pub fn validate_detailed(&self, manifest: &SchemaManifest) -> Result<Vec<String>> {
        #[cfg(feature = "validation")]
        {
            let manifest_json = serde_json::to_value(manifest)
                .map_err(|e| Error::Farp(format!("Failed to serialize manifest: {}", e)))?;

            match self.schema.validate(&manifest_json) {
                Ok(_) => Ok(vec![]),
                Err(errors) => {
                    let error_messages: Vec<String> = errors
                        .map(|e| format!("{}: {}", e.instance_path, e))
                        .collect();
                    Ok(error_messages)
                }
            }
        }

        #[cfg(not(feature = "validation"))]
        {
            // Fallback to basic validation
            match manifest.validate() {
                Ok(_) => Ok(vec![]),
                Err(e) => Ok(vec![e.to_string()]),
            }
        }
    }
}

impl Default for ManifestValidator {
    fn default() -> Self {
        Self::new().expect("Failed to create ManifestValidator")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{SchemaDescriptor, SchemaEndpoints, SchemaLocation, SchemaType};

    #[test]
    fn test_validator_creation() {
        let validator = ManifestValidator::new();
        assert!(validator.is_ok());
    }

    #[test]
    fn test_valid_manifest() {
        let validator = ManifestValidator::new().unwrap();

        let mut manifest = SchemaManifest::new("test-service", "1.0.0", "inst-123");
        manifest.endpoints = SchemaEndpoints {
            health: "/health".to_string(),
            ..Default::default()
        };
        manifest.add_capability("rest");

        let schema = SchemaDescriptor::new(
            SchemaType::OpenAPI,
            "3.1.0",
            SchemaLocation::http("http://localhost/openapi.json"),
            "application/json",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            1024,
        );
        manifest.add_schema(schema);
        manifest.update_checksum().unwrap();

        let result = validator.validate(&manifest);
        if let Err(e) = &result {
            eprintln!("Validation error: {}", e);
        }

        #[cfg(feature = "validation")]
        assert!(result.is_ok());
    }

    #[test]
    fn test_invalid_manifest_missing_health() {
        let validator = ManifestValidator::new().unwrap();

        let mut manifest = SchemaManifest::new("test-service", "1.0.0", "inst-123");
        // Missing health endpoint
        manifest.endpoints = SchemaEndpoints::default();

        let result = validator.validate(&manifest);

        // Should fail validation (health is required)
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_checksum_format() {
        let validator = ManifestValidator::new().unwrap();

        let mut manifest = SchemaManifest::new("test-service", "1.0.0", "inst-123");
        manifest.endpoints = SchemaEndpoints {
            health: "/health".to_string(),
            ..Default::default()
        };
        manifest.checksum = "invalid-checksum".to_string(); // Invalid format

        let result = validator.validate(&manifest);

        #[cfg(feature = "validation")]
        assert!(result.is_err());
    }
}
