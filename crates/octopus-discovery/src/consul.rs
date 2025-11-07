//! Consul service discovery implementation

use crate::provider::{
    DiscoveryEvent, DiscoveryProvider, ServiceHealth, ServiceInstance, ServiceMetadata,
};
use async_trait::async_trait;
use bytes::Bytes;
use http::{Method, Request, Uri};
use http_body_util::{BodyExt, Empty};
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use octopus_core::{Error, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::time;
use tracing::{debug, error, info, warn};

/// Consul service discovery client
#[derive(Debug, Clone)]
pub struct ConsulDiscovery {
    /// Consul HTTP API address
    address: String,

    /// Consul datacenter
    datacenter: Option<String>,

    /// HTTP client
    client: Arc<Client<hyper_util::client::legacy::connect::HttpConnector, Empty<Bytes>>>,

    /// Watch interval
    watch_interval: Duration,
}

/// Consul configuration
#[derive(Debug, Clone)]
pub struct ConsulConfig {
    /// Consul address (default: http://127.0.0.1:8500)
    pub address: String,

    /// Datacenter filter
    pub datacenter: Option<String>,

    /// Watch interval for changes
    pub watch_interval: Duration,
}

impl Default for ConsulConfig {
    fn default() -> Self {
        Self {
            address: "http://127.0.0.1:8500".to_string(),
            datacenter: None,
            watch_interval: Duration::from_secs(30),
        }
    }
}

/// Consul service entry
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ConsulService {
    #[serde(rename = "ID")]
    id: String,
    service: String,
    address: String,
    port: u16,
    tags: Vec<String>,
    meta: Option<HashMap<String, String>>,
}

/// Consul health check
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ConsulHealthCheck {
    status: String,
}

impl ConsulDiscovery {
    /// Create a new Consul discovery client
    pub fn new(config: ConsulConfig) -> Self {
        let client = Client::builder(TokioExecutor::new()).build_http();

        Self {
            address: config.address,
            datacenter: config.datacenter,
            client: Arc::new(client),
            watch_interval: config.watch_interval,
        }
    }

    /// Create with default configuration
    pub fn with_defaults() -> Self {
        Self::new(ConsulConfig::default())
    }

    /// Build Consul API URL
    fn build_url(&self, path: &str) -> Result<Uri> {
        let mut url = format!("{}{}", self.address, path);

        if let Some(dc) = &self.datacenter {
            url.push_str(&format!("?dc={}", dc));
        }

        url.parse()
            .map_err(|e| Error::Discovery(format!("Invalid Consul URL: {}", e)))
    }

    /// Make HTTP request to Consul
    async fn request(&self, uri: Uri) -> Result<Vec<u8>> {
        let req = Request::builder()
            .method(Method::GET)
            .uri(uri)
            .body(Empty::<Bytes>::new())
            .map_err(|e| Error::Discovery(format!("Failed to build request: {}", e)))?;

        let res = self
            .client
            .request(req)
            .await
            .map_err(|e| Error::Discovery(format!("Consul request failed: {}", e)))?;

        let body = res
            .into_body()
            .collect()
            .await
            .map_err(|e| Error::Discovery(format!("Failed to read response: {}", e)))?
            .to_bytes();

        Ok(body.to_vec())
    }

    /// Parse Consul service into ServiceInstance
    fn parse_service(&self, consul_svc: ConsulService, health: ServiceHealth) -> ServiceInstance {
        let metadata = ServiceMetadata {
            version: consul_svc
                .meta
                .as_ref()
                .and_then(|m| m.get("version").cloned()),
            tags: consul_svc.tags.clone(),
            datacenter: self.datacenter.clone(),
            custom: consul_svc.meta.unwrap_or_default(),
        };

        ServiceInstance {
            id: consul_svc.id,
            name: consul_svc.service,
            address: consul_svc.address,
            port: consul_svc.port,
            health,
            metadata,
            endpoints: vec![],
        }
    }

    /// Get health status from Consul checks
    fn parse_health(&self, checks: &[ConsulHealthCheck]) -> ServiceHealth {
        if checks.is_empty() {
            return ServiceHealth::Unknown;
        }

        let has_critical = checks.iter().any(|c| c.status == "critical");
        let has_warning = checks.iter().any(|c| c.status == "warning");

        if has_critical {
            ServiceHealth::Unhealthy
        } else if has_warning {
            ServiceHealth::Warning
        } else {
            ServiceHealth::Healthy
        }
    }
}

#[async_trait]
impl DiscoveryProvider for ConsulDiscovery {
    fn name(&self) -> &str {
        "consul"
    }

    async fn discover_services(&self) -> Result<Vec<ServiceInstance>> {
        debug!("Discovering all services from Consul");

        // Get all services
        let uri = self.build_url("/v1/catalog/services")?;
        let body = self.request(uri).await?;

        let services: HashMap<String, Vec<String>> = serde_json::from_slice(&body)
            .map_err(|e| Error::Discovery(format!("Failed to parse services: {}", e)))?;

        let mut instances = Vec::new();
        for service_name in services.keys() {
            match self.discover_service(service_name).await {
                Ok(mut service_instances) => instances.append(&mut service_instances),
                Err(e) => warn!(service = %service_name, error = %e, "Failed to discover service"),
            }
        }

        info!(count = instances.len(), "Discovered services from Consul");
        Ok(instances)
    }

    async fn discover_service(&self, service_name: &str) -> Result<Vec<ServiceInstance>> {
        debug!(service = %service_name, "Discovering service from Consul");

        // Get service health
        let uri = self.build_url(&format!("/v1/health/service/{}", service_name))?;
        let body = self.request(uri).await?;

        #[derive(Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct HealthEntry {
            service: ConsulService,
            checks: Vec<ConsulHealthCheck>,
        }

        let entries: Vec<HealthEntry> = serde_json::from_slice(&body)
            .map_err(|e| Error::Discovery(format!("Failed to parse health entries: {}", e)))?;

        let instances: Vec<ServiceInstance> = entries
            .into_iter()
            .map(|entry| {
                let health = self.parse_health(&entry.checks);
                self.parse_service(entry.service, health)
            })
            .collect();

        debug!(
            service = %service_name,
            count = instances.len(),
            "Discovered service instances"
        );

        Ok(instances)
    }

    async fn watch_services(
        &self,
        callback: Box<dyn Fn(DiscoveryEvent) + Send + Sync>,
    ) -> Result<()> {
        info!(
            interval = ?self.watch_interval,
            "Starting Consul service watch"
        );

        let mut previous_services: HashMap<String, Vec<ServiceInstance>> = HashMap::new();
        let mut interval = time::interval(self.watch_interval);

        loop {
            interval.tick().await;

            match self.discover_services().await {
                Ok(instances) => {
                    // Group by service name
                    let mut current_services: HashMap<String, Vec<ServiceInstance>> =
                        HashMap::new();
                    for instance in instances {
                        current_services
                            .entry(instance.name.clone())
                            .or_insert_with(Vec::new)
                            .push(instance);
                    }

                    // Detect changes
                    for (service_name, current_instances) in &current_services {
                        if let Some(previous_instances) = previous_services.get(service_name) {
                            // Check for new or updated instances
                            for instance in current_instances {
                                if !previous_instances.contains(instance) {
                                    callback(DiscoveryEvent::ServiceUpdated(instance.clone()));
                                }
                            }

                            // Check for removed instances
                            for prev_instance in previous_instances {
                                if !current_instances.contains(prev_instance) {
                                    callback(DiscoveryEvent::ServiceDeregistered {
                                        service_id: prev_instance.id.clone(),
                                        service_name: prev_instance.name.clone(),
                                    });
                                }
                            }
                        } else {
                            // New service
                            for instance in current_instances {
                                callback(DiscoveryEvent::ServiceRegistered(instance.clone()));
                            }
                        }
                    }

                    // Check for completely removed services
                    for (prev_service_name, prev_instances) in &previous_services {
                        if !current_services.contains_key(prev_service_name) {
                            for prev_instance in prev_instances {
                                callback(DiscoveryEvent::ServiceDeregistered {
                                    service_id: prev_instance.id.clone(),
                                    service_name: prev_instance.name.clone(),
                                });
                            }
                        }
                    }

                    previous_services = current_services;
                }
                Err(e) => {
                    error!(error = %e, "Failed to discover services during watch");
                }
            }
        }
    }

    async fn health_check(&self, service_id: &str) -> Result<ServiceHealth> {
        debug!(service_id = %service_id, "Checking health via Consul");

        let uri = self.build_url(&format!("/v1/health/checks/{}", service_id))?;
        let body = self.request(uri).await?;

        let checks: Vec<ConsulHealthCheck> = serde_json::from_slice(&body)
            .map_err(|e| Error::Discovery(format!("Failed to parse health checks: {}", e)))?;

        Ok(self.parse_health(&checks))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_consul_config_default() {
        let config = ConsulConfig::default();
        assert_eq!(config.address, "http://127.0.0.1:8500");
        assert_eq!(config.watch_interval, Duration::from_secs(30));
    }

    #[test]
    fn test_consul_discovery_creation() {
        let discovery = ConsulDiscovery::with_defaults();
        assert_eq!(discovery.name(), "consul");
    }

    #[test]
    fn test_build_url() {
        let discovery = ConsulDiscovery::with_defaults();
        let uri = discovery.build_url("/v1/catalog/services").unwrap();
        assert_eq!(uri.to_string(), "http://127.0.0.1:8500/v1/catalog/services");
    }

    #[test]
    fn test_parse_health() {
        let discovery = ConsulDiscovery::with_defaults();

        let healthy_checks = vec![ConsulHealthCheck {
            status: "passing".to_string(),
        }];
        assert_eq!(
            discovery.parse_health(&healthy_checks),
            ServiceHealth::Healthy
        );

        let critical_checks = vec![ConsulHealthCheck {
            status: "critical".to_string(),
        }];
        assert_eq!(
            discovery.parse_health(&critical_checks),
            ServiceHealth::Unhealthy
        );

        let warning_checks = vec![ConsulHealthCheck {
            status: "warning".to_string(),
        }];
        assert_eq!(
            discovery.parse_health(&warning_checks),
            ServiceHealth::Warning
        );
    }
}
