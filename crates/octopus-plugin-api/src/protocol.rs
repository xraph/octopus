//! Custom protocol handler traits

use crate::error::{PluginError, Result};
use crate::interceptor::Body;
use crate::plugin::Plugin;
use async_trait::async_trait;
use http::{Request, Response};

/// Custom protocol handler plugin
///
/// Handles custom protocols beyond HTTP (e.g., gRPC, GraphQL, WebSocket).
#[async_trait]
pub trait ProtocolHandler: Plugin {
    /// Protocol name (e.g., "grpc", "graphql", "websocket")
    fn protocol(&self) -> &str;

    /// Check if this handler supports the given request
    ///
    /// Called for every request to determine if this handler should process it.
    fn supports(&self, req: &Request<Body>) -> bool;

    /// Handle the request
    ///
    /// Process the request according to the protocol rules.
    async fn handle(&self, req: Request<Body>) -> Result<Response<Body>>;

    /// Check if protocol supports connection upgrades
    ///
    /// Returns true for protocols like WebSocket that require HTTP upgrade.
    fn supports_upgrade(&self) -> bool {
        false
    }

    /// Handle connection upgrade
    ///
    /// Called when the protocol requires an HTTP connection upgrade.
    async fn upgrade(&self, _req: Request<Body>) -> Result<()> {
        Err(PluginError::protocol("Upgrade not supported"))
    }

    /// Get protocol-specific metadata
    ///
    /// Returns additional information about the protocol capabilities.
    fn metadata(&self) -> ProtocolMetadata {
        ProtocolMetadata::default()
    }
}

/// Protocol metadata
#[derive(Debug, Clone)]
pub struct ProtocolMetadata {
    /// Protocol version
    pub version: String,

    /// Whether the protocol is bidirectional
    pub bidirectional: bool,

    /// Whether the protocol supports streaming
    pub streaming: bool,

    /// Whether the protocol requires HTTP/2
    pub requires_http2: bool,

    /// Content types handled by this protocol
    pub content_types: Vec<String>,

    /// Custom properties
    pub properties: std::collections::HashMap<String, serde_json::Value>,
}

impl Default for ProtocolMetadata {
    fn default() -> Self {
        Self {
            version: "1.0".to_string(),
            bidirectional: false,
            streaming: false,
            requires_http2: false,
            content_types: Vec::new(),
            properties: std::collections::HashMap::new(),
        }
    }
}

impl ProtocolMetadata {
    /// Create a new protocol metadata
    pub fn new() -> Self {
        Self::default()
    }

    /// Set protocol version
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = version.into();
        self
    }

    /// Mark as bidirectional
    pub fn bidirectional(mut self) -> Self {
        self.bidirectional = true;
        self
    }

    /// Mark as streaming
    pub fn streaming(mut self) -> Self {
        self.streaming = true;
        self
    }

    /// Mark as requiring HTTP/2
    pub fn requires_http2(mut self) -> Self {
        self.requires_http2 = true;
        self
    }

    /// Add supported content type
    pub fn with_content_type(mut self, content_type: impl Into<String>) -> Self {
        self.content_types.push(content_type.into());
        self
    }

    /// Add custom property
    pub fn with_property(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.properties.insert(key.into(), value);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_metadata() {
        let metadata = ProtocolMetadata::new()
            .with_version("2.0")
            .bidirectional()
            .streaming()
            .requires_http2()
            .with_content_type("application/grpc")
            .with_property("custom", serde_json::json!("value"));

        assert_eq!(metadata.version, "2.0");
        assert!(metadata.bidirectional);
        assert!(metadata.streaming);
        assert!(metadata.requires_http2);
        assert_eq!(metadata.content_types, vec!["application/grpc"]);
        assert_eq!(
            metadata.properties.get("custom"),
            Some(&serde_json::json!("value"))
        );
    }
}

