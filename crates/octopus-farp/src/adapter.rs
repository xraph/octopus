//! Adapter layer between external farp crate and Octopus gateway integration
//!
//! This module provides compatibility between the external farp protocol implementation
//! and Octopus-specific gateway features.

use std::sync::Arc;

// Import external farp types (not re-exported to avoid conflicts)
use farp::{
    errors::{Error as FarpError, Result as FarpResult},
    types::{
        SchemaDescriptor as ExternalSchemaDescriptor, SchemaEndpoints,
        SchemaManifest as ExternalSchemaManifest,
    },
};

// Re-export registry types that don't conflict
pub use farp::registry::{
    EventType, ManifestEvent, SchemaEvent, SchemaRegistry as ExternalSchemaRegistry,
};

// Re-export gateway client
pub use farp::gateway::{Client as GatewayClient, ServiceRoute};

/// Adapter for SchemaManifest to bridge between external and internal representations
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SchemaManifestAdapter {
    inner: ExternalSchemaManifest,
}

impl SchemaManifestAdapter {
    /// Create a new adapter from external manifest
    pub fn new(manifest: ExternalSchemaManifest) -> Self {
        Self { inner: manifest }
    }

    /// Get reference to inner manifest
    pub fn inner(&self) -> &ExternalSchemaManifest {
        &self.inner
    }

    /// Convert into inner manifest
    pub fn into_inner(self) -> ExternalSchemaManifest {
        self.inner
    }

    /// Get service name
    pub fn service_name(&self) -> &str {
        &self.inner.service_name
    }

    /// Get service version
    pub fn service_version(&self) -> &str {
        &self.inner.service_version
    }

    /// Get schemas
    pub fn schemas(&self) -> &[ExternalSchemaDescriptor] {
        &self.inner.schemas
    }

    /// Get endpoints
    pub fn endpoints(&self) -> &SchemaEndpoints {
        &self.inner.endpoints
    }
}

impl From<ExternalSchemaManifest> for SchemaManifestAdapter {
    fn from(manifest: ExternalSchemaManifest) -> Self {
        Self::new(manifest)
    }
}

impl From<SchemaManifestAdapter> for ExternalSchemaManifest {
    fn from(adapter: SchemaManifestAdapter) -> Self {
        adapter.into_inner()
    }
}

/// Async registry wrapper that implements octopus-specific features
pub struct RegistryAdapter {
    registry: Arc<dyn ExternalSchemaRegistry>,
}

impl RegistryAdapter {
    /// Create a new registry adapter
    pub fn new(registry: Arc<dyn ExternalSchemaRegistry>) -> Self {
        Self { registry }
    }

    /// Get the underlying registry
    pub fn registry(&self) -> &Arc<dyn ExternalSchemaRegistry> {
        &self.registry
    }

    /// Register a service manifest (async wrapper)
    pub async fn register_manifest(&self, manifest: &ExternalSchemaManifest) -> FarpResult<()> {
        self.registry.register_manifest(manifest).await
    }

    /// Get a service manifest by instance ID
    pub async fn get_manifest(&self, instance_id: &str) -> FarpResult<ExternalSchemaManifest> {
        self.registry.get_manifest(instance_id).await
    }

    /// Update a service manifest
    pub async fn update_manifest(&self, manifest: &ExternalSchemaManifest) -> FarpResult<()> {
        self.registry.update_manifest(manifest).await
    }

    /// Delete a service manifest
    pub async fn delete_manifest(&self, instance_id: &str) -> FarpResult<()> {
        self.registry.delete_manifest(instance_id).await
    }

    /// List all manifests for a service
    pub async fn list_manifests(
        &self,
        service_name: &str,
    ) -> FarpResult<Vec<ExternalSchemaManifest>> {
        self.registry.list_manifests(service_name).await
    }

    /// Publish a schema to the registry
    pub async fn publish_schema(&self, path: &str, schema: &serde_json::Value) -> FarpResult<()> {
        self.registry.publish_schema(path, schema).await
    }

    /// Fetch a schema from the registry
    pub async fn fetch_schema(&self, path: &str) -> FarpResult<serde_json::Value> {
        self.registry.fetch_schema(path).await
    }
}

impl std::fmt::Debug for RegistryAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegistryAdapter").finish()
    }
}

impl Clone for RegistryAdapter {
    fn clone(&self) -> Self {
        Self {
            registry: Arc::clone(&self.registry),
        }
    }
}

/// Convert octopus_core::Error to farp::Error
pub fn to_farp_error(err: octopus_core::Error) -> FarpError {
    // Use the validation helper to create a general error
    FarpError::validation("general", err.to_string())
}

/// Convert farp::Error to octopus_core::Error
pub fn from_farp_error(err: FarpError) -> octopus_core::Error {
    octopus_core::Error::Farp(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_adapter() {
        use farp::manifest::new_manifest;

        let manifest = new_manifest("test-service", "1.0.0", "instance-1");
        let adapter = SchemaManifestAdapter::new(manifest.clone());

        assert_eq!(adapter.service_name(), "test-service");
        assert_eq!(adapter.service_version(), "1.0.0");

        let inner: ExternalSchemaManifest = adapter.into();
        assert_eq!(inner.service_name, "test-service");
    }
}
