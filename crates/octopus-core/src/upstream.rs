//! Upstream service definitions

use crate::types::{CircuitBreakerConfig, HealthCheckConfig, LoadBalanceStrategy, TimeoutConfig};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU32, Ordering};

/// Upstream service cluster
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamCluster {
    /// Cluster name
    pub name: String,

    /// Load balancing strategy
    pub strategy: LoadBalanceStrategy,

    /// Upstream instances
    #[serde(skip)]
    pub instances: Vec<UpstreamInstance>,

    /// Health check configuration
    pub health_check: HealthCheckConfig,

    /// Circuit breaker configuration
    pub circuit_breaker: Option<CircuitBreakerConfig>,

    /// Timeout configuration
    pub timeout: TimeoutConfig,
}

impl UpstreamCluster {
    /// Create a new upstream cluster
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            strategy: LoadBalanceStrategy::default(),
            instances: Vec::new(),
            health_check: HealthCheckConfig::default(),
            circuit_breaker: Some(CircuitBreakerConfig::default()),
            timeout: TimeoutConfig::default(),
        }
    }

    /// Add an instance to the cluster
    pub fn add_instance(&mut self, instance: UpstreamInstance) {
        self.instances.push(instance);
    }

    /// Get all healthy instances
    pub fn healthy_instances(&self) -> Vec<&UpstreamInstance> {
        self.instances.iter().filter(|i| i.is_healthy()).collect()
    }

    /// Get total instance count
    pub fn instance_count(&self) -> usize {
        self.instances.len()
    }

    /// Get healthy instance count
    pub fn healthy_count(&self) -> usize {
        self.instances.iter().filter(|i| i.is_healthy()).count()
    }
}

/// Upstream service instance
#[derive(Debug, Serialize, Deserialize)]
pub struct UpstreamInstance {
    /// Instance ID
    pub id: String,

    /// Instance address
    pub address: String,

    /// Instance port
    pub port: u16,

    /// Instance weight for load balancing
    pub weight: u32,

    /// Is instance healthy
    #[serde(skip)]
    healthy: bool,

    /// Number of active connections (for least-connections LB)
    #[serde(skip)]
    #[serde(default)]
    active_connections: AtomicU32,

    /// Metadata
    pub metadata: std::collections::HashMap<String, String>,
}

impl Clone for UpstreamInstance {
    fn clone(&self) -> Self {
        Self {
            id: self.id.clone(),
            address: self.address.clone(),
            port: self.port,
            weight: self.weight,
            healthy: self.healthy,
            active_connections: AtomicU32::new(self.active_connections.load(Ordering::Relaxed)),
            metadata: self.metadata.clone(),
        }
    }
}

impl UpstreamInstance {
    /// Create a new upstream instance
    pub fn new(id: impl Into<String>, address: impl Into<String>, port: u16) -> Self {
        Self {
            id: id.into(),
            address: address.into(),
            port,
            weight: 1,
            healthy: true,
            active_connections: AtomicU32::new(0),
            metadata: Default::default(),
        }
    }

    /// Get socket address
    pub fn socket_addr(&self) -> Result<SocketAddr, std::net::AddrParseError> {
        format!("{}:{}", self.address, self.port).parse()
    }

    /// Get base URL
    pub fn base_url(&self) -> String {
        format!("http://{}:{}", self.address, self.port)
    }

    /// Check if instance is healthy
    pub fn is_healthy(&self) -> bool {
        self.healthy
    }

    /// Mark instance as healthy
    pub fn mark_healthy(&mut self) {
        self.healthy = true;
    }

    /// Mark instance as unhealthy
    pub fn mark_unhealthy(&mut self) {
        self.healthy = false;
    }

    /// Get active connection count
    pub fn active_connections(&self) -> u32 {
        self.active_connections.load(Ordering::Relaxed)
    }

    /// Increment active connections
    pub fn increment_connections(&self) {
        self.active_connections.fetch_add(1, Ordering::Relaxed);
    }

    /// Decrement active connections
    pub fn decrement_connections(&self) {
        self.active_connections.fetch_sub(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_upstream_cluster() {
        let mut cluster = UpstreamCluster::new("test-service");

        let instance1 = UpstreamInstance::new("inst-1", "localhost", 8080);
        let instance2 = UpstreamInstance::new("inst-2", "localhost", 8081);

        cluster.add_instance(instance1);
        cluster.add_instance(instance2);

        assert_eq!(cluster.instance_count(), 2);
        assert_eq!(cluster.healthy_count(), 2);
    }

    #[test]
    fn test_upstream_instance() {
        let mut instance = UpstreamInstance::new("inst-1", "127.0.0.1", 8080);

        assert!(instance.is_healthy());
        assert_eq!(instance.base_url(), "http://127.0.0.1:8080");

        instance.mark_unhealthy();
        assert!(!instance.is_healthy());

        instance.increment_connections();
        instance.increment_connections();
        assert_eq!(instance.active_connections(), 2);

        instance.decrement_connections();
        assert_eq!(instance.active_connections(), 1);
    }
}
