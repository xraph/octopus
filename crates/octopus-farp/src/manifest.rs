//! Schema manifest types for FARP v1.0.0

use crate::types::{SchemaDescriptor, SchemaEndpoints, SchemaLocation, SchemaType, PROTOCOL_VERSION};
use octopus_core::{Error, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

/// Schema manifest describing all API contracts for a service instance
///
/// This is the core data structure in FARP v1.0.0, representing all schemas
/// exposed by a service instance along with metadata for change detection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaManifest {
    /// Protocol version (semver, currently "1.0.0")
    pub version: String,
    
    /// Service name (logical service identifier)
    pub service_name: String,
    
    /// Service version (semver recommended: "v1.2.3")
    pub service_version: String,
    
    /// Unique instance identifier
    pub instance_id: String,
    
    /// Schemas exposed by this instance
    pub schemas: Vec<SchemaDescriptor>,
    
    /// Capabilities/protocols supported (e.g., ["rest", "grpc", "websocket"])
    pub capabilities: Vec<String>,
    
    /// Endpoints for introspection and health
    pub endpoints: SchemaEndpoints,
    
    /// Unix timestamp of last manifest update
    pub updated_at: i64,
    
    /// SHA256 checksum of all schemas combined (for change detection)
    pub checksum: String,
}

impl SchemaManifest {
    /// Create a new schema manifest
    pub fn new(
        service_name: impl Into<String>,
        service_version: impl Into<String>,
        instance_id: impl Into<String>,
    ) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
            
        Self {
            version: PROTOCOL_VERSION.to_string(),
            service_name: service_name.into(),
            service_version: service_version.into(),
            instance_id: instance_id.into(),
            schemas: Vec::new(),
            capabilities: Vec::new(),
            endpoints: SchemaEndpoints::default(),
            updated_at: now,
            checksum: String::new(),
        }
    }
    
    /// Add a schema descriptor to the manifest
    pub fn add_schema(&mut self, schema: SchemaDescriptor) {
        self.schemas.push(schema);
    }
    
    /// Add a schema descriptor (builder pattern)
    pub fn with_schema(mut self, schema: SchemaDescriptor) -> Self {
        self.schemas.push(schema);
        self
    }
    
    /// Add a capability
    pub fn add_capability(&mut self, capability: impl Into<String>) {
        let cap = capability.into();
        if !self.capabilities.contains(&cap) {
            self.capabilities.push(cap);
        }
    }
    
    /// Add a capability (builder pattern)
    pub fn with_capability(mut self, capability: impl Into<String>) -> Self {
        self.add_capability(capability);
        self
    }
    
    /// Set endpoints
    pub fn with_endpoints(mut self, endpoints: SchemaEndpoints) -> Self {
        self.endpoints = endpoints;
        self
    }
    
    /// Add an OpenAPI schema via HTTP endpoint (convenience method)
    pub fn add_openapi_http(&mut self, url: &str) {
        let schema = SchemaDescriptor::new(
            SchemaType::OpenAPI,
            "3.1.0",
            SchemaLocation::http(url),
            "application/json",
            String::new(), // Hash will be calculated when schema is fetched
            0,             // Size will be set when schema is fetched
        );
        self.add_schema(schema);
    }
    
    /// Add an AsyncAPI schema via HTTP endpoint (convenience method)
    pub fn add_asyncapi_http(&mut self, url: &str) {
        let schema = SchemaDescriptor::new(
            SchemaType::AsyncAPI,
            "3.0.0",
            SchemaLocation::http(url),
            "application/json",
            String::new(), // Hash will be calculated when schema is fetched
            0,             // Size will be set when schema is fetched
        );
        self.add_schema(schema);
    }
    
    /// Calculate and update the manifest checksum
    ///
    /// This combines all schema hashes in a deterministic order
    /// and calculates a SHA256 hash of the result.
    pub fn update_checksum(&mut self) -> Result<()> {
        self.checksum = self.calculate_checksum()?;
        self.updated_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        Ok(())
    }
    
    /// Calculate the manifest checksum without modifying the manifest
    pub fn calculate_checksum(&self) -> Result<String> {
        if self.schemas.is_empty() {
            return Ok(String::new());
        }
        
        // Sort schemas by type for deterministic hashing
        let mut sorted_schemas = self.schemas.clone();
        sorted_schemas.sort_by(|a, b| a.schema_type.as_str().cmp(b.schema_type.as_str()));
        
        // Concatenate all schema hashes
        let combined: String = sorted_schemas
            .iter()
            .map(|s| s.hash.as_str())
            .collect();
        
        // Calculate SHA256 of combined hashes
        let mut hasher = Sha256::new();
        hasher.update(combined.as_bytes());
        let result = hasher.finalize();
        Ok(format!("{:x}", result))
    }
    
    /// Validate the manifest for correctness
    pub fn validate(&self) -> Result<()> {
        // Check protocol version compatibility
        if !is_compatible(&self.version) {
            return Err(Error::Farp(format!(
                "Incompatible manifest version: {} (expected {})",
                self.version, PROTOCOL_VERSION
            )));
        }
        
        // Check required fields
        if self.service_name.is_empty() {
            return Err(Error::Farp("service_name is required".to_string()));
        }
        if self.instance_id.is_empty() {
            return Err(Error::Farp("instance_id is required".to_string()));
        }
        if self.endpoints.health.is_empty() {
            return Err(Error::Farp("health endpoint is required".to_string()));
        }
        
        // Validate each schema descriptor
        for (i, schema) in self.schemas.iter().enumerate() {
            validate_schema_descriptor(schema).map_err(|e| {
                Error::Farp(format!("Invalid schema at index {}: {}", i, e))
            })?;
        }
        
        // Verify checksum if present
        if !self.checksum.is_empty() {
            let expected_checksum = self.calculate_checksum()?;
            if self.checksum != expected_checksum {
                return Err(Error::Farp(format!(
                    "Checksum mismatch: expected {}, got {}",
                    expected_checksum, self.checksum
                )));
            }
        }
        
        Ok(())
    }
    
    /// Get a schema descriptor by type
    pub fn get_schema(&self, schema_type: SchemaType) -> Option<&SchemaDescriptor> {
        self.schemas.iter().find(|s| s.schema_type == schema_type)
    }
    
    /// Check if the manifest has a specific capability
    pub fn has_capability(&self, capability: &str) -> bool {
        self.capabilities.iter().any(|c| c == capability)
    }
    
    /// Clone the manifest
    pub fn deep_clone(&self) -> Self {
        self.clone()
    }
    
    /// Serialize to JSON
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string(self).map_err(|e| Error::Farp(format!("Failed to serialize: {}", e)))
    }
    
    /// Serialize to pretty JSON
    pub fn to_pretty_json(&self) -> Result<String> {
        serde_json::to_string_pretty(self)
            .map_err(|e| Error::Farp(format!("Failed to serialize: {}", e)))
    }
    
    /// Deserialize from JSON
    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json).map_err(|e| Error::Farp(format!("Failed to deserialize: {}", e)))
    }
    
    /// Verify checksum matches calculated checksum
    pub fn verify_checksum(&self) -> Result<bool> {
        if self.checksum.is_empty() {
            return Ok(true); // No checksum to verify
        }
        
        let calculated = self.calculate_checksum()?;
        Ok(self.checksum == calculated)
    }
}

/// Check if a manifest version is compatible with the current protocol version
fn is_compatible(manifest_version: &str) -> bool {
    // For v1.x.x, only major version must match
    let manifest_parts: Vec<&str> = manifest_version.split('.').collect();
    let protocol_parts: Vec<&str> = PROTOCOL_VERSION.split('.').collect();
    
    if manifest_parts.is_empty() || protocol_parts.is_empty() {
        return false;
    }
    
    // Major version must match
    manifest_parts[0] == protocol_parts[0]
}

/// Validate a schema descriptor
fn validate_schema_descriptor(schema: &SchemaDescriptor) -> Result<()> {
    // Check spec version
    if schema.spec_version.is_empty() {
        return Err(Error::Farp("spec_version is required".to_string()));
    }
    
    // Validate location
    schema.location.validate()?;
    
    // For inline schemas, inline_schema must be present
    if matches!(schema.location.location_type, crate::types::LocationType::Inline) 
        && schema.inline_schema.is_none() 
    {
        return Err(Error::Farp(
            "inline_schema is required for inline location type".to_string(),
        ));
    }
    
    // Check hash format (should be 64 hex characters for SHA256)
    if !schema.hash.is_empty() && schema.hash.len() != 64 {
        return Err(Error::Farp(format!(
            "invalid hash format: expected 64 hex characters, got {}",
            schema.hash.len()
        )));
    }
    
    // Check content type
    if schema.content_type.is_empty() {
        return Err(Error::Farp("content_type is required".to_string()));
    }
    
    Ok(())
}

/// Calculate the SHA256 checksum of a schema (any JSON-serializable value)
pub fn calculate_schema_checksum(schema: &serde_json::Value) -> Result<String> {
    let json = serde_json::to_string(schema)
        .map_err(|e| Error::Farp(format!("Failed to serialize schema: {}", e)))?;
    
    let mut hasher = Sha256::new();
    hasher.update(json.as_bytes());
    let result = hasher.finalize();
    Ok(format!("{:x}", result))
}

/// Manifest diff represents the difference between two manifests
#[derive(Debug, Clone)]
pub struct ManifestDiff {
    /// Schemas present in new but not in old
    pub schemas_added: Vec<SchemaDescriptor>,
    
    /// Schemas present in old but not in new
    pub schemas_removed: Vec<SchemaDescriptor>,
    
    /// Schemas present in both but with different hashes
    pub schemas_changed: Vec<SchemaChangeDiff>,
    
    /// New capabilities
    pub capabilities_added: Vec<String>,
    
    /// Removed capabilities
    pub capabilities_removed: Vec<String>,
    
    /// Endpoints changed
    pub endpoints_changed: bool,
}

/// Schema change diff represents a changed schema
#[derive(Debug, Clone)]
pub struct SchemaChangeDiff {
    pub schema_type: SchemaType,
    pub old_hash: String,
    pub new_hash: String,
}

impl ManifestDiff {
    /// Check if there are any changes
    pub fn has_changes(&self) -> bool {
        !self.schemas_added.is_empty()
            || !self.schemas_removed.is_empty()
            || !self.schemas_changed.is_empty()
            || !self.capabilities_added.is_empty()
            || !self.capabilities_removed.is_empty()
            || self.endpoints_changed
    }
}

/// Compare two manifests and return the differences
pub fn diff_manifests(old: &SchemaManifest, new: &SchemaManifest) -> ManifestDiff {
    let mut diff = ManifestDiff {
        schemas_added: Vec::new(),
        schemas_removed: Vec::new(),
        schemas_changed: Vec::new(),
        capabilities_added: Vec::new(),
        capabilities_removed: Vec::new(),
        endpoints_changed: false,
    };
    
    // Build maps for easier comparison
    let old_schemas: std::collections::HashMap<_, _> = old
        .schemas
        .iter()
        .map(|s| (s.schema_type, s))
        .collect();
    
    let new_schemas: std::collections::HashMap<_, _> = new
        .schemas
        .iter()
        .map(|s| (s.schema_type, s))
        .collect();
    
    // Find added and changed schemas
    for (schema_type, new_schema) in &new_schemas {
        if let Some(old_schema) = old_schemas.get(schema_type) {
            // Schema exists in both, check if changed
            if old_schema.hash != new_schema.hash {
                diff.schemas_changed.push(SchemaChangeDiff {
                    schema_type: *schema_type,
                    old_hash: old_schema.hash.clone(),
                    new_hash: new_schema.hash.clone(),
                });
            }
        } else {
            // Schema is new
            diff.schemas_added.push((*new_schema).clone());
        }
    }
    
    // Find removed schemas
    for (schema_type, old_schema) in &old_schemas {
        if !new_schemas.contains_key(schema_type) {
            diff.schemas_removed.push((*old_schema).clone());
        }
    }
    
    // Compare capabilities
    let old_caps: std::collections::HashSet<_> = old.capabilities.iter().collect();
    let new_caps: std::collections::HashSet<_> = new.capabilities.iter().collect();
    
    diff.capabilities_added = new_caps
        .difference(&old_caps)
        .map(|&s| s.clone())
        .collect();
    
    diff.capabilities_removed = old_caps
        .difference(&new_caps)
        .map(|&s| s.clone())
        .collect();
    
    // Compare endpoints
    diff.endpoints_changed = old.endpoints != new.endpoints;
    
    diff
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::LocationType;

    #[test]
    fn test_manifest_creation() {
        let manifest = SchemaManifest::new("my-service", "v1.0.0", "inst-123");
        assert_eq!(manifest.service_name, "my-service");
        assert_eq!(manifest.service_version, "v1.0.0");
        assert_eq!(manifest.instance_id, "inst-123");
        assert_eq!(manifest.version, PROTOCOL_VERSION);
    }

    #[test]
    fn test_add_schema() {
        let mut manifest = SchemaManifest::new("test", "v1.0.0", "inst-1");
        let schema = SchemaDescriptor::new(
            SchemaType::OpenAPI,
            "3.1.0",
            SchemaLocation::http("http://example.com/openapi.json"),
            "application/json",
            "abc123".repeat(10) + "abcd", // 64 hex chars
            1024,
        );
        manifest.add_schema(schema);
        assert_eq!(manifest.schemas.len(), 1);
    }

    #[test]
    fn test_checksum_calculation() {
        let mut manifest = SchemaManifest::new("test", "v1.0.0", "inst-1");
        let schema = SchemaDescriptor::new(
            SchemaType::OpenAPI,
            "3.1.0",
            SchemaLocation::http("http://example.com/openapi.json"),
            "application/json",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            1024,
        );
        manifest.add_schema(schema);
        manifest.update_checksum().unwrap();
        assert!(!manifest.checksum.is_empty());
        assert_eq!(manifest.checksum.len(), 64); // SHA256 = 64 hex chars
    }

    #[test]
    fn test_version_compatibility() {
        assert!(is_compatible("1.0.0"));
        assert!(is_compatible("1.0.1"));
        assert!(is_compatible("1.1.0"));
        assert!(!is_compatible("2.0.0"));
        assert!(!is_compatible("0.9.0"));
    }

    #[test]
    fn test_manifest_validation() {
        let mut manifest = SchemaManifest::new("test", "v1.0.0", "inst-1");
        manifest.endpoints.health = "/health".to_string();
        
        // Should pass with minimal fields
        assert!(manifest.validate().is_ok());
        
        // Should fail without health endpoint
        manifest.endpoints.health = String::new();
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn test_diff_manifests() {
        let mut old = SchemaManifest::new("test", "v1.0.0", "inst-1");
        old.add_capability("rest");
        
        let mut new = SchemaManifest::new("test", "v1.0.0", "inst-1");
        new.add_capability("rest");
        new.add_capability("grpc");
        
        let diff = diff_manifests(&old, &new);
        assert_eq!(diff.capabilities_added.len(), 1);
        assert!(diff.capabilities_added.contains(&"grpc".to_string()));
    }
}
