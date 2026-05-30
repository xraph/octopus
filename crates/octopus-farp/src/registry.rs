//! Schema registry using external farp implementation with Octopus-specific features

use crate::adapter::ExternalSchemaRegistry;
use crate::manifest::SchemaManifest;
use crate::schema::SchemaDescriptor;
use dashmap::DashMap;
use governor::{Quota, RateLimiter};
use octopus_core::{Error, Result};
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::SystemTime;
use tracing::{debug, info, warn};

/// Service registration using external farp types
#[derive(Debug, Clone)]
pub struct ServiceRegistration {
    /// Service name
    pub service_name: String,
    /// Registration time
    pub registered_at: SystemTime,
    /// Last updated time
    pub updated_at: SystemTime,
    /// Schema manifest
    pub manifest: SchemaManifest,
    /// Fetched schemas
    pub schemas: Vec<SchemaDescriptor>,
    /// Manifest fetch URL (for heartbeat retry — §17.4.1)
    pub manifest_url: Option<String>,
}

impl ServiceRegistration {
    /// Create a new service registration
    #[must_use]
    pub fn new(manifest: SchemaManifest) -> Self {
        let now = SystemTime::now();
        Self {
            service_name: manifest.service_name.clone(),
            registered_at: now,
            updated_at: now,
            manifest,
            schemas: Vec::new(),
            manifest_url: None,
        }
    }

    /// Update the registration with new manifest
    pub fn update(&mut self, manifest: SchemaManifest) {
        self.manifest = manifest;
        self.updated_at = SystemTime::now();
    }

    /// Add a schema to the registration
    pub fn add_schema(&mut self, schema: SchemaDescriptor) {
        self.schemas.push(schema);
        self.updated_at = SystemTime::now();
    }
}

type ServiceRateLimiter = RateLimiter<
    governor::state::direct::NotKeyed,
    governor::state::InMemoryState,
    governor::clock::DefaultClock,
>;

/// Schema registry wrapping external farp registry with Octopus features
#[derive(Clone)]
pub struct SchemaRegistry {
    /// External farp registry (async)
    external_registry: Arc<dyn ExternalSchemaRegistry>,
    /// Local cache for sync API compatibility
    services: Arc<DashMap<String, ServiceRegistration>>,
    /// Rate limiter per service (updates per minute)
    rate_limiters: Arc<DashMap<String, Arc<ServiceRateLimiter>>>,
    /// Maximum updates per minute per service
    max_updates_per_minute: u32,
    /// Schema cache TTL - if set, schemas older than this are considered stale
    schema_cache_ttl: Option<std::time::Duration>,
}

impl std::fmt::Debug for SchemaRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SchemaRegistry")
            .field("max_updates_per_minute", &self.max_updates_per_minute)
            .field("service_count", &self.services.len())
            .finish()
    }
}

impl Default for SchemaRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl SchemaRegistry {
    /// Create a new schema registry with default rate limit (60 updates/minute)
    #[must_use]
    pub fn new() -> Self {
        Self::with_rate_limit(60)
    }

    /// Create a new schema registry with custom rate limit
    #[must_use]
    pub fn with_rate_limit(max_updates_per_minute: u32) -> Self {
        // Create external farp memory registry
        let external_registry: Arc<dyn ExternalSchemaRegistry> =
            Arc::new(farp::registry::memory::MemoryRegistry::new());

        Self {
            external_registry,
            services: Arc::new(DashMap::new()),
            rate_limiters: Arc::new(DashMap::new()),
            max_updates_per_minute,
            schema_cache_ttl: None,
        }
    }

    /// Create a new schema registry with a schema cache TTL
    ///
    /// Schemas older than the TTL will be cleared when retrieved via `get_service()`.
    #[must_use]
    pub fn with_cache_ttl(ttl: std::time::Duration) -> Self {
        let mut registry = Self::new();
        registry.schema_cache_ttl = Some(ttl);
        registry
    }

    /// Create a registry with custom external registry implementation
    #[must_use]
    pub fn with_external_registry(
        external_registry: Arc<dyn ExternalSchemaRegistry>,
        max_updates_per_minute: u32,
    ) -> Self {
        Self {
            external_registry,
            services: Arc::new(DashMap::new()),
            rate_limiters: Arc::new(DashMap::new()),
            max_updates_per_minute,
            schema_cache_ttl: None,
        }
    }

    /// Check rate limit for a service
    fn check_rate_limit(&self, service_name: &str) -> Result<()> {
        // Get or create rate limiter for this service
        let limiter = self
            .rate_limiters
            .entry(service_name.to_string())
            .or_insert_with(|| {
                let quota =
                    Quota::per_minute(NonZeroU32::new(self.max_updates_per_minute).unwrap());
                Arc::new(RateLimiter::direct(quota))
            });

        // Check if we can proceed
        if let Ok(()) = limiter.check() {
            Ok(())
        } else {
            warn!(
                service = %service_name,
                max_per_minute = self.max_updates_per_minute,
                "Rate limit exceeded for service schema updates"
            );
            Err(Error::Farp(format!(
                "Rate limit exceeded for service '{}': max {} updates per minute",
                service_name, self.max_updates_per_minute
            )))
        }
    }

    /// Convert local manifest to external format
    fn to_external_manifest(&self, manifest: &SchemaManifest) -> farp::types::SchemaManifest {
        // Convert via serde to handle type differences
        // This works because both types have the same structure and derive Serialize/Deserialize
        let json = serde_json::to_value(manifest).expect("Failed to serialize manifest");
        serde_json::from_value(json).expect("Failed to deserialize manifest")
    }

    /// Register a service
    pub async fn register_service(&self, manifest: SchemaManifest) -> Result<()> {
        let service_name = manifest.service_name.clone();

        // Check rate limit for updates
        self.check_rate_limit(&service_name)?;

        // Convert to external manifest format
        let external_manifest = self.to_external_manifest(&manifest);

        // Use external registry
        self.external_registry
            .register_manifest(&external_manifest)
            .await
            .map_err(|e| {
                Error::Farp(format!(
                    "Failed to register service in external registry: {}",
                    e
                ))
            })?;

        // Update local cache
        let registration = ServiceRegistration::new(manifest);
        self.services.insert(service_name.clone(), registration);

        info!(service = %service_name, "Service registered");
        Ok(())
    }

    /// Update a service registration
    pub async fn update_service(&self, manifest: SchemaManifest) -> Result<()> {
        let service_name = manifest.service_name.clone();

        // Check rate limit for updates
        self.check_rate_limit(&service_name)?;

        // Convert to external manifest format
        let external_manifest = self.to_external_manifest(&manifest);

        // Update in external registry
        self.external_registry
            .update_manifest(&external_manifest)
            .await
            .map_err(|e| {
                Error::Farp(format!(
                    "Failed to update service in external registry: {}",
                    e
                ))
            })?;

        // Update local cache
        if let Some(mut reg) = self.services.get_mut(&service_name) {
            reg.update(manifest);
            info!(service = %service_name, "Service updated");
            Ok(())
        } else {
            Err(Error::Farp(format!(
                "Service '{service_name}' not registered"
            )))
        }
    }

    /// Deregister a service
    pub async fn deregister_service(&self, service_name: &str) -> Result<()> {
        // Get instance ID for external registry
        let instance_id = if let Some(reg) = self.services.get(service_name) {
            format!("{}-{}", reg.service_name, reg.manifest.service_version)
        } else {
            return Err(Error::Farp(format!(
                "Service '{service_name}' not registered"
            )));
        };

        // Remove from external registry
        if let Err(e) = self.external_registry.delete_manifest(&instance_id).await {
            warn!(
                service = %service_name,
                error = %e,
                "Failed to deregister service from external registry"
            );
        }

        // Remove from local cache
        self.services
            .remove(service_name)
            .ok_or_else(|| Error::Farp(format!("Service '{service_name}' not registered")))?;

        info!(service = %service_name, "Service deregistered");
        Ok(())
    }

    /// Get a service registration
    ///
    /// Returns the registration as-is. Schemas may be stale if `schema_cache_ttl` is set
    /// and the registration hasn't been refreshed. Use `needs_schema_refresh()` to check
    /// staleness without clearing schemas.
    pub fn get_service(&self, service_name: &str) -> Result<ServiceRegistration> {
        let registration = self
            .services
            .get(service_name)
            .map(|reg| reg.clone())
            .ok_or_else(|| Error::Farp(format!("Service '{service_name}' not registered")))?;

        Ok(registration)
    }

    /// Check if a service's schemas need refreshing (expired past TTL)
    ///
    /// Unlike `get_service()`, this does not clear or modify schemas — it only checks
    /// whether the TTL has been exceeded so the caller can trigger a re-fetch.
    pub fn needs_schema_refresh(&self, service_name: &str) -> bool {
        if let Some(ttl) = self.schema_cache_ttl {
            if let Some(reg) = self.services.get(service_name) {
                if let Ok(elapsed) = reg.updated_at.elapsed() {
                    if elapsed > ttl {
                        debug!(
                            service = %service_name,
                            elapsed_secs = elapsed.as_secs(),
                            ttl_secs = ttl.as_secs(),
                            "Schema cache expired, refresh needed"
                        );
                        return true;
                    }
                }
            }
        }
        false
    }

    /// List all registered services
    #[must_use]
    pub fn list_services(&self) -> Vec<String> {
        self.services
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }

    /// Add a schema to a service
    pub fn add_schema(&self, service_name: &str, schema: SchemaDescriptor) -> Result<()> {
        if let Some(mut reg) = self.services.get_mut(service_name) {
            reg.add_schema(schema);
            Ok(())
        } else {
            Err(Error::Farp(format!(
                "Service '{service_name}' not registered"
            )))
        }
    }

    /// Get schemas for a service
    pub fn get_schemas(&self, service_name: &str) -> Result<Vec<SchemaDescriptor>> {
        Ok(self.get_service(service_name)?.schemas)
    }

    /// Get total number of registered services
    #[must_use]
    pub fn service_count(&self) -> usize {
        self.services.len()
    }

    /// Update heartbeat for an instance (update last-seen time)
    pub fn heartbeat(&self, instance_id: &str) -> Result<()> {
        // Find the service by instance_id
        for mut entry in self.services.iter_mut() {
            if entry.manifest.instance_id == instance_id {
                entry.updated_at = SystemTime::now();
                return Ok(());
            }
        }
        Err(Error::Farp(format!("Instance '{instance_id}' not found")))
    }

    /// Deregister a service by instance ID
    pub fn deregister_by_instance_id(&self, instance_id: &str) -> Result<()> {
        // Find and remove the service by instance_id
        let service_name = {
            let mut found = None;
            for entry in self.services.iter() {
                if entry.manifest.instance_id == instance_id {
                    found = Some(entry.key().clone());
                    break;
                }
            }
            found
        };

        if let Some(name) = service_name {
            // Remove from external registry (best-effort)
            let ext_id = format!("{}-{}", instance_id, "deregister");
            let external = self.external_registry.clone();
            tokio::spawn(async move {
                let _ = external.delete_manifest(&ext_id).await;
            });

            self.services.remove(&name);
            info!(instance_id = %instance_id, service = %name, "Service instance deregistered");
            Ok(())
        } else {
            Err(Error::Farp(format!("Instance '{instance_id}' not found")))
        }
    }

    /// Get mutable reference to services map (for storing manifest_url, etc.)
    pub fn services_mut(&self) -> &DashMap<String, ServiceRegistration> {
        &self.services
    }

    /// Get reference to underlying external registry
    pub fn external_registry(&self) -> &Arc<dyn ExternalSchemaRegistry> {
        &self.external_registry
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_manifest(name: &str, version: &str) -> SchemaManifest {
        let mut manifest = SchemaManifest::new(name, version, &format!("{}-{}", name, version));
        // Add required health endpoint for external registry validation
        manifest.endpoints.health = format!("http://localhost:8080/health");
        manifest
    }

    #[test]
    fn test_service_registration() {
        let manifest = create_test_manifest("test-service", "1.0.0");
        let registration = ServiceRegistration::new(manifest.clone());

        assert_eq!(registration.service_name, "test-service");
        assert_eq!(registration.manifest.service_name, manifest.service_name);
    }

    #[tokio::test]
    async fn test_registry_register() {
        let registry = SchemaRegistry::new();
        let manifest = create_test_manifest("test-service", "1.0.0");

        registry.register_service(manifest).await.unwrap();
        assert_eq!(registry.service_count(), 1);

        let service = registry.get_service("test-service").unwrap();
        assert_eq!(service.service_name, "test-service");
    }

    #[tokio::test]
    async fn test_registry_update() {
        let registry = SchemaRegistry::new();
        let manifest = create_test_manifest("test-service", "1.0.0");

        registry.register_service(manifest.clone()).await.unwrap();

        // Update with same instance_id but different version
        let mut updated_manifest = manifest.clone();
        updated_manifest.service_version = "2.0.0".to_string();
        registry.update_service(updated_manifest).await.unwrap();

        let service = registry.get_service("test-service").unwrap();
        assert_eq!(service.manifest.service_version, "2.0.0");
    }

    #[tokio::test]
    async fn test_registry_deregister() {
        let registry = SchemaRegistry::new();
        let manifest = create_test_manifest("test-service", "1.0.0");

        registry.register_service(manifest).await.unwrap();
        assert_eq!(registry.service_count(), 1);

        registry.deregister_service("test-service").await.unwrap();
        assert_eq!(registry.service_count(), 0);

        assert!(registry.get_service("test-service").is_err());
    }

    #[tokio::test]
    async fn test_registry_list_services() {
        let registry = SchemaRegistry::new();

        for i in 1..=3 {
            let manifest = create_test_manifest(&format!("service-{}", i), "1.0.0");
            registry.register_service(manifest).await.unwrap();
        }

        let services = registry.list_services();
        assert_eq!(services.len(), 3);
        assert!(services.contains(&"service-1".to_string()));
        assert!(services.contains(&"service-2".to_string()));
        assert!(services.contains(&"service-3".to_string()));
    }

    #[tokio::test]
    async fn test_external_registry_integration() {
        let registry = SchemaRegistry::new();
        let manifest = create_test_manifest("test-service", "1.0.0");

        // Register via wrapped registry
        registry.register_service(manifest.clone()).await.unwrap();

        // Verify it's in the external registry
        let external_registry = registry.external_registry();
        let instance_id = format!("{}-{}", manifest.service_name, manifest.service_version);

        let external_manifest = external_registry.get_manifest(&instance_id).await.unwrap();

        assert_eq!(external_manifest.service_name, "test-service");
    }
}
