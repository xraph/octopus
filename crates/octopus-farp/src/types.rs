//! Core FARP types matching the v1.0.0 specification

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Schema type enumeration - supported schema/protocol types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SchemaType {
    /// OpenAPI/Swagger specifications (REST APIs)
    #[serde(rename = "openapi")]
    OpenAPI,

    /// AsyncAPI specifications (WebSocket, SSE, message queues)
    #[serde(rename = "asyncapi")]
    AsyncAPI,

    /// gRPC protocol buffer definitions
    #[serde(rename = "grpc")]
    GRPC,

    /// GraphQL Schema Definition Language
    #[serde(rename = "graphql")]
    GraphQL,

    /// oRPC (OpenAPI-based RPC) specifications
    #[serde(rename = "orpc")]
    ORPC,

    /// Apache Thrift IDL
    #[serde(rename = "thrift")]
    Thrift,

    /// Apache Avro schemas
    #[serde(rename = "avro")]
    Avro,

    /// Custom/proprietary schema types
    #[serde(rename = "custom")]
    Custom,
}

impl SchemaType {
    /// Returns the string representation of the schema type
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::OpenAPI => "openapi",
            Self::AsyncAPI => "asyncapi",
            Self::GRPC => "grpc",
            Self::GraphQL => "graphql",
            Self::ORPC => "orpc",
            Self::Thrift => "thrift",
            Self::Avro => "avro",
            Self::Custom => "custom",
        }
    }
}

/// Location type enumeration - how schemas can be retrieved
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LocationType {
    /// Fetch schema via HTTP GET request
    #[serde(rename = "http")]
    HTTP,

    /// Fetch schema from backend KV store (Consul, etcd, etc.)
    #[serde(rename = "registry")]
    Registry,

    /// Schema is embedded directly in the manifest
    #[serde(rename = "inline")]
    Inline,
}

impl LocationType {
    /// Returns the string representation of the location type
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::HTTP => "http",
            Self::Registry => "registry",
            Self::Inline => "inline",
        }
    }
}

/// Schema location describes where and how to fetch a schema
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SchemaLocation {
    /// Location type (http, registry, inline)
    #[serde(rename = "type")]
    pub location_type: LocationType,

    /// HTTP URL (if type == HTTP)
    /// Example: "http://user-service:8080/openapi.json"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    /// Registry path in backend KV store (if type == Registry)
    /// Example: "/schemas/user-service/v1/openapi"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registry_path: Option<String>,

    /// HTTP headers for authentication (if type == HTTP)
    /// Example: {"Authorization": "Bearer token"}
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<HashMap<String, String>>,
}

impl SchemaLocation {
    /// Create an HTTP location
    pub fn http(url: impl Into<String>) -> Self {
        Self {
            location_type: LocationType::HTTP,
            url: Some(url.into()),
            registry_path: None,
            headers: None,
        }
    }

    /// Create an HTTP location with headers
    pub fn http_with_headers(url: impl Into<String>, headers: HashMap<String, String>) -> Self {
        Self {
            location_type: LocationType::HTTP,
            url: Some(url.into()),
            registry_path: None,
            headers: Some(headers),
        }
    }

    /// Create a registry location
    pub fn registry(path: impl Into<String>) -> Self {
        Self {
            location_type: LocationType::Registry,
            url: None,
            registry_path: Some(path.into()),
            headers: None,
        }
    }

    /// Create an inline location
    pub fn inline() -> Self {
        Self {
            location_type: LocationType::Inline,
            url: None,
            registry_path: None,
            headers: None,
        }
    }

    /// Validate the schema location
    pub fn validate(&self) -> octopus_core::Result<()> {
        match self.location_type {
            LocationType::HTTP => {
                if self.url.is_none() {
                    return Err(octopus_core::Error::Farp(
                        "URL required for HTTP location".to_string(),
                    ));
                }
            }
            LocationType::Registry => {
                if self.registry_path.is_none() {
                    return Err(octopus_core::Error::Farp(
                        "Registry path required for registry location".to_string(),
                    ));
                }
            }
            LocationType::Inline => {
                // No additional validation needed
            }
        }
        Ok(())
    }
}

/// Schema descriptor describes a single API schema/contract
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SchemaDescriptor {
    /// Type of schema (openapi, asyncapi, grpc, graphql, etc.)
    #[serde(rename = "type")]
    pub schema_type: SchemaType,

    /// Specification version (e.g., "3.1.0" for OpenAPI, "3.0.0" for AsyncAPI)
    pub spec_version: String,

    /// How to retrieve the schema
    pub location: SchemaLocation,

    /// Content type (e.g., "application/json", "application/x-protobuf")
    pub content_type: String,

    /// Optional: Inline schema for small schemas (< 100KB recommended)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inline_schema: Option<serde_json::Value>,

    /// SHA256 hash of schema content for integrity validation
    pub hash: String,

    /// Size in bytes
    pub size: i64,
}

impl SchemaDescriptor {
    /// Create a new schema descriptor
    pub fn new(
        schema_type: SchemaType,
        spec_version: impl Into<String>,
        location: SchemaLocation,
        content_type: impl Into<String>,
        hash: impl Into<String>,
        size: i64,
    ) -> Self {
        Self {
            schema_type,
            spec_version: spec_version.into(),
            location,
            content_type: content_type.into(),
            inline_schema: None,
            hash: hash.into(),
            size,
        }
    }

    /// Set inline schema
    pub fn with_inline_schema(mut self, schema: serde_json::Value) -> Self {
        self.inline_schema = Some(schema);
        self
    }
}

/// Schema endpoints provides URLs for service introspection
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SchemaEndpoints {
    /// Health check endpoint (required)
    /// Example: "/health" or "/healthz"
    pub health: String,

    /// Prometheus metrics endpoint (optional)
    /// Example: "/metrics"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics: Option<String>,

    /// OpenAPI spec endpoint (optional)
    /// Example: "/openapi.json"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub openapi: Option<String>,

    /// AsyncAPI spec endpoint (optional)
    /// Example: "/asyncapi.json"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asyncapi: Option<String>,

    /// Whether gRPC server reflection is enabled
    #[serde(default, skip_serializing_if = "is_false")]
    pub grpc_reflection: bool,

    /// GraphQL introspection endpoint (optional)
    /// Example: "/graphql"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graphql: Option<String>,
}

fn is_false(b: &bool) -> bool {
    !(*b)
}

/// Protocol capability enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Capability {
    /// REST API support
    #[serde(rename = "rest")]
    REST,

    /// gRPC support
    #[serde(rename = "grpc")]
    GRPC,

    /// WebSocket support
    #[serde(rename = "websocket")]
    WebSocket,

    /// Server-Sent Events support
    #[serde(rename = "sse")]
    SSE,

    /// GraphQL support
    #[serde(rename = "graphql")]
    GraphQL,

    /// MQTT support
    #[serde(rename = "mqtt")]
    MQTT,

    /// AMQP support
    #[serde(rename = "amqp")]
    AMQP,

    /// RPC support (oRPC)
    #[serde(rename = "rpc")]
    RPC,
}

impl Capability {
    /// Returns the string representation of the capability
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::REST => "rest",
            Self::GRPC => "grpc",
            Self::WebSocket => "websocket",
            Self::SSE => "sse",
            Self::GraphQL => "graphql",
            Self::MQTT => "mqtt",
            Self::AMQP => "amqp",
            Self::RPC => "rpc",
        }
    }
}

/// FARP protocol version
pub const PROTOCOL_VERSION: &str = "1.0.0";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schema_type_serialization() {
        let schema_type = SchemaType::OpenAPI;
        let json = serde_json::to_string(&schema_type).unwrap();
        assert_eq!(json, r#""openapi""#);
    }

    #[test]
    fn test_location_type_serialization() {
        let location_type = LocationType::HTTP;
        let json = serde_json::to_string(&location_type).unwrap();
        assert_eq!(json, r#""http""#);
    }

    #[test]
    fn test_schema_location_http() {
        let location = SchemaLocation::http("http://example.com/openapi.json");
        assert_eq!(location.location_type, LocationType::HTTP);
        assert_eq!(
            location.url,
            Some("http://example.com/openapi.json".to_string())
        );
        assert!(location.validate().is_ok());
    }

    #[test]
    fn test_schema_location_registry() {
        let location = SchemaLocation::registry("/schemas/service/v1/openapi");
        assert_eq!(location.location_type, LocationType::Registry);
        assert_eq!(
            location.registry_path,
            Some("/schemas/service/v1/openapi".to_string())
        );
        assert!(location.validate().is_ok());
    }

    #[test]
    fn test_schema_location_inline() {
        let location = SchemaLocation::inline();
        assert_eq!(location.location_type, LocationType::Inline);
        assert!(location.validate().is_ok());
    }

    #[test]
    fn test_schema_descriptor_creation() {
        let descriptor = SchemaDescriptor::new(
            SchemaType::OpenAPI,
            "3.1.0",
            SchemaLocation::http("http://example.com/openapi.json"),
            "application/json",
            "abc123",
            1024,
        );

        assert_eq!(descriptor.schema_type, SchemaType::OpenAPI);
        assert_eq!(descriptor.spec_version, "3.1.0");
        assert_eq!(descriptor.content_type, "application/json");
        assert_eq!(descriptor.hash, "abc123");
        assert_eq!(descriptor.size, 1024);
    }
}
