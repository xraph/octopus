//! Schema descriptor and provider traits

use async_trait::async_trait;
use octopus_core::Result;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Schema format
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SchemaFormat {
    /// OpenAPI schema
    OpenApi,
    /// AsyncAPI schema
    AsyncApi,
    /// gRPC protobuf schema
    Grpc,
    /// GraphQL schema
    GraphQL,
    /// Custom schema format
    Custom,
}

impl fmt::Display for SchemaFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SchemaFormat::OpenApi => write!(f, "openapi"),
            SchemaFormat::AsyncApi => write!(f, "asyncapi"),
            SchemaFormat::Grpc => write!(f, "grpc"),
            SchemaFormat::GraphQL => write!(f, "graphql"),
            SchemaFormat::Custom => write!(f, "custom"),
        }
    }
}

/// Schema descriptor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaDescriptor {
    /// Schema ID
    pub id: String,
    /// Service name
    pub service: String,
    /// Schema format
    pub format: SchemaFormat,
    /// Schema version
    pub version: String,
    /// Schema content (JSON/YAML)
    pub content: String,
    /// Checksum for integrity
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
}

impl SchemaDescriptor {
    /// Create a new schema descriptor
    pub fn new(
        id: impl Into<String>,
        service: impl Into<String>,
        format: SchemaFormat,
        version: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            service: service.into(),
            format,
            version: version.into(),
            content: content.into(),
            checksum: None,
        }
    }

    /// Calculate checksum
    pub fn calculate_checksum(&mut self) -> String {
        use sha2::{Digest, Sha256};

        let mut hasher = Sha256::new();
        hasher.update(self.content.as_bytes());
        let result = hasher.finalize();
        let checksum = format!("{:x}", result);
        self.checksum = Some(checksum.clone());
        checksum
    }
}

/// Schema provider trait for fetching schemas
#[async_trait]
pub trait SchemaProvider: Send + Sync + fmt::Debug {
    /// Fetch a schema by ID
    async fn fetch_schema(&self, schema_id: &str) -> Result<SchemaDescriptor>;

    /// List available schemas
    async fn list_schemas(&self) -> Result<Vec<String>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schema_format_display() {
        assert_eq!(SchemaFormat::OpenApi.to_string(), "openapi");
        assert_eq!(SchemaFormat::Grpc.to_string(), "grpc");
    }

    #[test]
    fn test_schema_descriptor() {
        let mut schema = SchemaDescriptor::new(
            "schema-1",
            "my-service",
            SchemaFormat::OpenApi,
            "3.0.0",
            r#"{"openapi": "3.0.0"}"#,
        );

        assert_eq!(schema.id, "schema-1");
        assert_eq!(schema.format, SchemaFormat::OpenApi);

        let checksum = schema.calculate_checksum();
        assert!(!checksum.is_empty());
        assert_eq!(schema.checksum.as_ref(), Some(&checksum));
    }
}
