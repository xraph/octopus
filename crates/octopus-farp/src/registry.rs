//! Schema registry for managing service schemas

use crate::manifest::SchemaManifest;
use crate::schema::SchemaDescriptor;
use dashmap::DashMap;
use governor::{Quota, RateLimiter};
use octopus_core::{Error, Result};
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::SystemTime;
use tracing::{info, warn};

/// Service registration
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
}

impl ServiceRegistration {
    /// Create a new service registration
    pub fn new(manifest: SchemaManifest) -> Self {
        let now = SystemTime::now();
        Self {
            service_name: manifest.service_name.clone(),
            registered_at: now,
            updated_at: now,
            manifest,
            schemas: Vec::new(),
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

/// Schema registry with rate limiting
#[derive(Clone, Debug)]
pub struct SchemaRegistry {
    services: Arc<DashMap<String, ServiceRegistration>>,
    /// Rate limiter per service (updates per minute)
    rate_limiters: Arc<DashMap<String, Arc<ServiceRateLimiter>>>,
    /// Maximum updates per minute per service
    max_updates_per_minute: u32,
}

impl Default for SchemaRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl SchemaRegistry {
    /// Create a new schema registry with default rate limit (60 updates/minute)
    pub fn new() -> Self {
        Self::with_rate_limit(60)
    }

    /// Create a new schema registry with custom rate limit
    pub fn with_rate_limit(max_updates_per_minute: u32) -> Self {
        Self {
            services: Arc::new(DashMap::new()),
            rate_limiters: Arc::new(DashMap::new()),
            max_updates_per_minute,
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
        match limiter.check() {
            Ok(_) => Ok(()),
            Err(_) => {
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
    }

    /// Register a service
    pub fn register_service(&self, manifest: SchemaManifest) -> Result<()> {
        let service_name = manifest.service_name.clone();

        // Check rate limit for updates
        self.check_rate_limit(&service_name)?;

        let registration = ServiceRegistration::new(manifest);
        self.services.insert(service_name.clone(), registration);

        info!(service = %service_name, "Service registered");
        Ok(())
    }

    /// Update a service registration
    pub fn update_service(&self, manifest: SchemaManifest) -> Result<()> {
        let service_name = manifest.service_name.clone();

        // Check rate limit for updates
        self.check_rate_limit(&service_name)?;

        if let Some(mut reg) = self.services.get_mut(&service_name) {
            reg.update(manifest);
            info!(service = %service_name, "Service updated");
            Ok(())
        } else {
            Err(Error::Farp(format!(
                "Service '{}' not registered",
                service_name
            )))
        }
    }

    /// Deregister a service
    pub fn deregister_service(&self, service_name: &str) -> Result<()> {
        self.services
            .remove(service_name)
            .ok_or_else(|| Error::Farp(format!("Service '{}' not registered", service_name)))?;
        Ok(())
    }

    /// Get a service registration
    pub fn get_service(&self, service_name: &str) -> Result<ServiceRegistration> {
        self.services
            .get(service_name)
            .map(|reg| reg.clone())
            .ok_or_else(|| Error::Farp(format!("Service '{}' not registered", service_name)))
    }

    /// List all registered services
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
                "Service '{}' not registered",
                service_name
            )))
        }
    }

    /// Get schemas for a service
    pub fn get_schemas(&self, service_name: &str) -> Result<Vec<SchemaDescriptor>> {
        Ok(self.get_service(service_name)?.schemas)
    }

    /// Get total number of registered services
    pub fn service_count(&self) -> usize {
        self.services.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::ServiceInfo;
    use std::collections::HashMap;

    fn create_test_service_info(name: &str) -> ServiceInfo {
        ServiceInfo {
            name: name.to_string(),
            version: "1.0.0".to_string(),
            description: "Test service".to_string(),
            base_url: format!("http://localhost:8080/{}", name),
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn test_service_registration() {
        let service_info = create_test_service_info("test-service");
        let manifest = SchemaManifest::new(service_info);
        let registration = ServiceRegistration::new(manifest.clone());

        assert_eq!(registration.service_name, "test-service");
        assert_eq!(registration.manifest.service.name, manifest.service.name);
    }

    #[test]
    fn test_registry_register() {
        let registry = SchemaRegistry::new();
        let service_info = create_test_service_info("test-service");
        let manifest = SchemaManifest::new(service_info);

        registry.register_service(manifest).unwrap();
        assert_eq!(registry.service_count(), 1);

        let service = registry.get_service("test-service").unwrap();
        assert_eq!(service.service_name, "test-service");
    }

    #[test]
    fn test_registry_update() {
        let registry = SchemaRegistry::new();
        let service_info = create_test_service_info("test-service");
        let manifest = SchemaManifest::new(service_info.clone());

        registry.register_service(manifest.clone()).unwrap();

        let mut updated_service = service_info;
        updated_service.version = "2.0.0".to_string();
        let updated_manifest = SchemaManifest::new(updated_service);

        registry.update_service(updated_manifest).unwrap();

        let service = registry.get_service("test-service").unwrap();
        assert_eq!(service.manifest.service.version, "2.0.0");
    }

    #[test]
    fn test_registry_deregister() {
        let registry = SchemaRegistry::new();
        let service_info = create_test_service_info("test-service");
        let manifest = SchemaManifest::new(service_info);

        registry.register_service(manifest).unwrap();
        assert_eq!(registry.service_count(), 1);

        registry.deregister_service("test-service").unwrap();
        assert_eq!(registry.service_count(), 0);

        assert!(registry.get_service("test-service").is_err());
    }

    #[test]
    fn test_registry_list_services() {
        let registry = SchemaRegistry::new();

        for i in 1..=3 {
            let service_info = create_test_service_info(&format!("service-{}", i));
            let manifest = SchemaManifest::new(service_info);
            registry.register_service(manifest).unwrap();
        }

        let services = registry.list_services();
        assert_eq!(services.len(), 3);
        assert!(services.contains(&"service-1".to_string()));
        assert!(services.contains(&"service-2".to_string()));
        assert!(services.contains(&"service-3".to_string()));
    }
}
