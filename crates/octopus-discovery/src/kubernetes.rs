//! Kubernetes service discovery

use crate::provider::{
    DiscoveryEvent, DiscoveryProvider, ServiceHealth, ServiceInstance, ServiceMetadata,
};
use async_trait::async_trait;
use futures::{StreamExt, TryStreamExt};
use k8s_openapi::api::core::v1::{Endpoints, Service};
use kube::{
    api::{Api, ListParams},
    runtime::{watcher, watcher::Config as WatcherConfig},
    Client,
};
use octopus_core::{Error, Result};
use std::collections::HashMap;
use tracing::{debug, info, warn};

/// Kubernetes service discovery
#[derive(Clone)]
pub struct K8sDiscovery {
    client: Client,
    namespace: Option<String>,
    label_selector: Option<String>,
}

impl std::fmt::Debug for K8sDiscovery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("K8sDiscovery")
            .field("namespace", &self.namespace)
            .field("label_selector", &self.label_selector)
            .finish()
    }
}

/// Kubernetes discovery configuration
#[derive(Debug, Clone, Default)]
pub struct K8sConfig {
    /// Namespace filter (None = all namespaces)
    pub namespace: Option<String>,

    /// Label selector for filtering services
    pub label_selector: Option<String>,
}

impl K8sDiscovery {
    /// Create a new Kubernetes discovery client
    pub async fn new(config: K8sConfig) -> Result<Self> {
        let client = Client::try_default()
            .await
            .map_err(|e| Error::Discovery(format!("Failed to create K8s client: {e}")))?;

        Ok(Self {
            client,
            namespace: config.namespace,
            label_selector: config.label_selector,
        })
    }

    /// Create with default configuration
    pub async fn with_defaults() -> Result<Self> {
        Self::new(K8sConfig::default()).await
    }

    /// Get the namespace to use
    fn get_namespace(&self) -> &str {
        self.namespace.as_deref().unwrap_or("default")
    }

    /// Get list parameters
    fn list_params(&self) -> ListParams {
        let mut params = ListParams::default();
        if let Some(selector) = &self.label_selector {
            params = params.labels(selector);
        }
        params
    }

    /// Parse K8s service into ServiceInstances
    async fn parse_service(&self, svc: &Service, endpoints: &Endpoints) -> Vec<ServiceInstance> {
        let mut instances = Vec::new();

        let svc_name = svc.metadata.name.as_ref().unwrap();
        let namespace = svc.metadata.namespace.as_ref().unwrap();

        // Extract metadata
        let custom_map: HashMap<String, String> = svc
            .metadata
            .labels
            .as_ref()
            .map(|labels| labels.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default();

        let metadata = ServiceMetadata {
            version: svc
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("version").cloned()),
            tags: svc
                .metadata
                .labels
                .as_ref()
                .map(|labels| labels.keys().cloned().collect())
                .unwrap_or_default(),
            datacenter: Some(namespace.clone()),
            custom: custom_map,
        };

        // Parse endpoints
        if let Some(subsets) = &endpoints.subsets {
            for subset in subsets {
                if let Some(addresses) = &subset.addresses {
                    for address in addresses {
                        let ip = &address.ip;
                        if let Some(ports) = &subset.ports {
                            for port in ports {
                                let port_num = port.port as u16;
                                let id = format!("{namespace}:{svc_name}:{ip}:{port_num}");

                                instances.push(ServiceInstance {
                                    id,
                                    name: svc_name.clone(),
                                    address: ip.clone(),
                                    port: port_num,
                                    health: ServiceHealth::Healthy, // K8s only includes ready endpoints
                                    metadata: metadata.clone(),
                                    endpoints: vec![],
                                });
                            }
                        }
                    }
                }
            }
        }

        instances
    }
}

#[async_trait]
impl DiscoveryProvider for K8sDiscovery {
    fn name(&self) -> &str {
        "kubernetes"
    }

    async fn discover_services(&self) -> Result<Vec<ServiceInstance>> {
        info!("Discovering all services from Kubernetes");

        let services: Api<Service> = if let Some(ns) = &self.namespace {
            Api::namespaced(self.client.clone(), ns)
        } else {
            Api::all(self.client.clone())
        };

        let endpoints: Api<Endpoints> = if let Some(ns) = &self.namespace {
            Api::namespaced(self.client.clone(), ns)
        } else {
            Api::all(self.client.clone())
        };

        let svc_list = services
            .list(&self.list_params())
            .await
            .map_err(|e| Error::Discovery(format!("Failed to list services: {e}")))?;

        let ep_list = endpoints
            .list(&ListParams::default())
            .await
            .map_err(|e| Error::Discovery(format!("Failed to list endpoints: {e}")))?;

        // Create endpoint map by service name
        let mut ep_map: HashMap<String, &Endpoints> = HashMap::new();
        for ep in &ep_list {
            if let Some(name) = &ep.metadata.name {
                ep_map.insert(name.clone(), ep);
            }
        }

        let mut all_instances = Vec::new();

        for svc in &svc_list {
            if let Some(name) = &svc.metadata.name {
                if let Some(ep) = ep_map.get(name) {
                    let instances = self.parse_service(svc, ep).await;
                    all_instances.extend(instances);
                } else {
                    warn!(service = %name, "No endpoints found for service");
                }
            }
        }

        info!(
            count = all_instances.len(),
            "Discovered services from Kubernetes"
        );
        Ok(all_instances)
    }

    async fn discover_service(&self, service_name: &str) -> Result<Vec<ServiceInstance>> {
        debug!(service = %service_name, "Discovering service from Kubernetes");

        let namespace = self.get_namespace();

        let services: Api<Service> = Api::namespaced(self.client.clone(), namespace);
        let endpoints: Api<Endpoints> = Api::namespaced(self.client.clone(), namespace);

        let svc = services
            .get(service_name)
            .await
            .map_err(|e| Error::Discovery(format!("Failed to get service: {e}")))?;

        let ep = endpoints
            .get(service_name)
            .await
            .map_err(|e| Error::Discovery(format!("Failed to get endpoints: {e}")))?;

        let instances = self.parse_service(&svc, &ep).await;

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
        info!("Starting Kubernetes service watch");

        let services: Api<Service> = if let Some(ns) = &self.namespace {
            Api::namespaced(self.client.clone(), ns)
        } else {
            Api::all(self.client.clone())
        };

        let watch_config = WatcherConfig::default();
        let mut watch_stream = watcher(services, watch_config).boxed();

        while let Some(event) = watch_stream.try_next().await.ok().flatten() {
            match event {
                watcher::Event::Apply(svc) => {
                    if let Some(name) = &svc.metadata.name {
                        debug!(service = %name, "Service applied/updated");

                        // Fetch endpoints to get full instance info
                        if let Ok(instances) = self.discover_service(name).await {
                            for instance in instances {
                                callback(DiscoveryEvent::ServiceUpdated(instance));
                            }
                        }
                    }
                }
                watcher::Event::Delete(svc) => {
                    if let Some(name) = &svc.metadata.name {
                        debug!(service = %name, "Service deleted");
                        callback(DiscoveryEvent::ServiceDeregistered {
                            service_id: name.clone(),
                            service_name: name.clone(),
                        });
                    }
                }
                watcher::Event::Init | watcher::Event::InitApply(_) | watcher::Event::InitDone => {
                    // Initialization events
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_k8s_config_default() {
        let config = K8sConfig::default();
        assert!(config.namespace.is_none());
        assert!(config.label_selector.is_none());
    }
}
