//! Core service discovery abstractions

use async_trait::async_trait;
use octopus_core::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::net::SocketAddr;

/// Service instance discovered by a provider
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceInstance {
    /// Service ID
    pub id: String,

    /// Service name
    pub name: String,

    /// Service address (IP or hostname)
    pub address: String,

    /// Service port
    pub port: u16,

    /// Service health status
    pub health: ServiceHealth,

    /// Service metadata/tags
    pub metadata: ServiceMetadata,

    /// Service endpoints
    pub endpoints: Vec<ServiceEndpoint>,
}

impl ServiceInstance {
    /// Get the socket address for this instance
    pub fn socket_addr(&self) -> Option<SocketAddr> {
        format!("{}:{}", self.address, self.port).parse().ok()
    }
}

/// Service health status
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ServiceHealth {
    /// Service is healthy
    Healthy,

    /// Service is unhealthy
    Unhealthy,

    /// Service health is unknown
    Unknown,

    /// Service is in warning state
    Warning,
}

impl fmt::Display for ServiceHealth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ServiceHealth::Healthy => write!(f, "healthy"),
            ServiceHealth::Unhealthy => write!(f, "unhealthy"),
            ServiceHealth::Unknown => write!(f, "unknown"),
            ServiceHealth::Warning => write!(f, "warning"),
        }
    }
}

/// Service metadata
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceMetadata {
    /// Service version
    pub version: Option<String>,

    /// Service tags
    pub tags: Vec<String>,

    /// Service datacenter/region
    pub datacenter: Option<String>,

    /// Custom metadata
    pub custom: HashMap<String, String>,
}

/// Service endpoint
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceEndpoint {
    /// Endpoint path
    pub path: String,

    /// HTTP methods
    pub methods: Vec<String>,

    /// Endpoint metadata
    pub metadata: HashMap<String, String>,
}

/// Discovery events
#[derive(Debug, Clone)]
pub enum DiscoveryEvent {
    /// New service registered
    ServiceRegistered(ServiceInstance),

    /// Service deregistered
    /// Service deregistered event
    ServiceDeregistered {
        /// Service ID
        service_id: String,
        /// Service name
        service_name: String,
    },

    /// Service health changed
    HealthChanged {
        /// Service ID
        service_id: String,
        /// Previous health status
        old_health: ServiceHealth,
        /// New health status
        new_health: ServiceHealth,
    },

    /// Service updated
    ServiceUpdated(ServiceInstance),
}

/// Service discovery provider trait
#[async_trait]
pub trait DiscoveryProvider: Send + Sync + fmt::Debug {
    /// Get the provider name
    fn name(&self) -> &str;

    /// Discover all services
    async fn discover_services(&self) -> Result<Vec<ServiceInstance>>;

    /// Discover instances of a specific service
    async fn discover_service(&self, service_name: &str) -> Result<Vec<ServiceInstance>>;

    /// Watch for service changes
    async fn watch_services(
        &self,
        callback: Box<dyn Fn(DiscoveryEvent) + Send + Sync>,
    ) -> Result<()>;

    /// Register a service (if supported)
    async fn register_service(&self, instance: ServiceInstance) -> Result<()> {
        let _ = instance;
        Err(octopus_core::Error::Discovery(
            "Service registration not supported by this provider".to_string(),
        ))
    }

    /// Deregister a service (if supported)
    async fn deregister_service(&self, service_id: &str) -> Result<()> {
        let _ = service_id;
        Err(octopus_core::Error::Discovery(
            "Service deregistration not supported by this provider".to_string(),
        ))
    }

    /// Health check a service (if supported)
    async fn health_check(&self, service_id: &str) -> Result<ServiceHealth> {
        let _ = service_id;
        Ok(ServiceHealth::Unknown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_instance_socket_addr() {
        let instance = ServiceInstance {
            id: "test-1".to_string(),
            name: "test".to_string(),
            address: "127.0.0.1".to_string(),
            port: 8080,
            health: ServiceHealth::Healthy,
            metadata: ServiceMetadata::default(),
            endpoints: vec![],
        };

        let addr = instance.socket_addr().unwrap();
        assert_eq!(addr.to_string(), "127.0.0.1:8080");
    }

    #[test]
    fn test_service_health_display() {
        assert_eq!(ServiceHealth::Healthy.to_string(), "healthy");
        assert_eq!(ServiceHealth::Unhealthy.to_string(), "unhealthy");
        assert_eq!(ServiceHealth::Unknown.to_string(), "unknown");
        assert_eq!(ServiceHealth::Warning.to_string(), "warning");
    }
}
