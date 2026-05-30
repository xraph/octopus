//! Kubernetes service discovery
//!
//! Discovers upstream instances from the Kubernetes API. Primary path uses the
//! `discovery.k8s.io/v1` **EndpointSlice** API, which reflects pod scale up/down
//! (the legacy `Endpoints` + `Service`-only watch misses those events). The
//! legacy `Endpoints` path is retained as a fallback for clusters where the
//! EndpointSlice API is unavailable.

use crate::provider::{
    DiscoveryEvent, DiscoveryProvider, ServiceHealth, ServiceInstance, ServiceMetadata,
};
use async_trait::async_trait;
use futures::{StreamExt, TryStreamExt};
use k8s_openapi::api::core::v1::{Endpoints, Service};
use k8s_openapi::api::discovery::v1::EndpointSlice;
use kube::{
    api::{Api, ListParams},
    runtime::{watcher, watcher::Config as WatcherConfig},
    Client,
};
use octopus_core::{Error, Result};
use std::collections::HashMap;
use tracing::{debug, info, warn};

/// Label set by Kubernetes on every EndpointSlice naming its owning Service.
const SERVICE_NAME_LABEL: &str = "kubernetes.io/service-name";

/// Kubernetes service discovery
#[derive(Clone)]
pub struct K8sDiscovery {
    client: Client,
    namespace: Option<String>,
    label_selector: Option<String>,
    use_endpoint_slices: bool,
    include_not_ready: bool,
}

impl std::fmt::Debug for K8sDiscovery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("K8sDiscovery")
            .field("namespace", &self.namespace)
            .field("label_selector", &self.label_selector)
            .field("use_endpoint_slices", &self.use_endpoint_slices)
            .field("include_not_ready", &self.include_not_ready)
            .finish()
    }
}

/// Kubernetes discovery configuration
#[derive(Debug, Clone)]
pub struct K8sConfig {
    /// Namespace filter (None = all namespaces)
    pub namespace: Option<String>,

    /// Label selector for filtering services
    pub label_selector: Option<String>,

    /// Prefer the EndpointSlice API (default: true), falling back to Endpoints
    /// if it is unavailable.
    pub use_endpoint_slices: bool,

    /// Include endpoints whose `ready` condition is false/unknown.
    pub include_not_ready: bool,
}

impl Default for K8sConfig {
    fn default() -> Self {
        Self {
            namespace: None,
            label_selector: None,
            use_endpoint_slices: true,
            include_not_ready: false,
        }
    }
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
            use_endpoint_slices: config.use_endpoint_slices,
            include_not_ready: config.include_not_ready,
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

    /// Get list parameters for the Service list (honoring the label selector).
    fn service_list_params(&self) -> ListParams {
        let mut params = ListParams::default();
        if let Some(selector) = &self.label_selector {
            params = params.labels(selector);
        }
        params
    }

    fn services_api(&self) -> Api<Service> {
        match &self.namespace {
            Some(ns) => Api::namespaced(self.client.clone(), ns),
            None => Api::all(self.client.clone()),
        }
    }

    fn endpoint_slices_api(&self) -> Api<EndpointSlice> {
        match &self.namespace {
            Some(ns) => Api::namespaced(self.client.clone(), ns),
            None => Api::all(self.client.clone()),
        }
    }

    fn endpoints_api(&self) -> Api<Endpoints> {
        match &self.namespace {
            Some(ns) => Api::namespaced(self.client.clone(), ns),
            None => Api::all(self.client.clone()),
        }
    }

    // ── EndpointSlice path (primary) ─────────────────────────────────────

    /// Discover all services via EndpointSlices, grouped by owning Service.
    async fn discover_via_slices(&self) -> Result<Vec<ServiceInstance>> {
        let services = self.services_api();
        let slices = self.endpoint_slices_api();

        let svc_list = services
            .list(&self.service_list_params())
            .await
            .map_err(|e| Error::Discovery(format!("Failed to list services: {e}")))?;

        let slice_list = slices
            .list(&ListParams::default())
            .await
            .map_err(|e| Error::Discovery(format!("Failed to list endpoint slices: {e}")))?;

        // Map (namespace, service-name) -> metadata, for services that passed
        // the label selector.
        let mut svc_meta: HashMap<(String, String), ServiceMetadata> = HashMap::new();
        for svc in &svc_list {
            if let (Some(name), Some(ns)) = (&svc.metadata.name, &svc.metadata.namespace) {
                svc_meta.insert((ns.clone(), name.clone()), metadata_from_service(svc));
            }
        }

        // Group slices by their owning (namespace, service).
        let mut by_service: HashMap<(String, String), Vec<&EndpointSlice>> = HashMap::new();
        for slice in &slice_list {
            if let (Some(svc_name), Some(ns)) =
                (owning_service(slice), slice.metadata.namespace.clone())
            {
                by_service.entry((ns, svc_name)).or_default().push(slice);
            }
        }

        let mut all_instances = Vec::new();
        for ((ns, svc_name), slices) in by_service {
            // Only emit instances for services that passed the selector.
            let Some(metadata) = svc_meta.get(&(ns.clone(), svc_name.clone())) else {
                continue;
            };
            all_instances.extend(instances_from_slices(
                &svc_name,
                &ns,
                metadata,
                &slices,
                self.include_not_ready,
            ));
        }

        info!(
            count = all_instances.len(),
            "Discovered services from Kubernetes (EndpointSlice)"
        );
        Ok(all_instances)
    }

    /// Discover a single service's instances via EndpointSlices.
    async fn discover_service_via_slices(
        &self,
        service_name: &str,
    ) -> Result<Vec<ServiceInstance>> {
        let namespace = self.get_namespace().to_string();
        let services: Api<Service> = Api::namespaced(self.client.clone(), &namespace);
        let slices: Api<EndpointSlice> = Api::namespaced(self.client.clone(), &namespace);

        // Service metadata (best-effort; default if the Service is gone).
        let metadata = match services.get(service_name).await {
            Ok(svc) => metadata_from_service(&svc),
            Err(e) => {
                debug!(service = %service_name, error = %e, "Service metadata unavailable, using defaults");
                ServiceMetadata {
                    datacenter: Some(namespace.clone()),
                    ..Default::default()
                }
            }
        };

        let params = ListParams::default().labels(&format!("{SERVICE_NAME_LABEL}={service_name}"));
        let slice_list = slices
            .list(&params)
            .await
            .map_err(|e| Error::Discovery(format!("Failed to list endpoint slices: {e}")))?;

        let slice_refs: Vec<&EndpointSlice> = slice_list.iter().collect();
        Ok(instances_from_slices(
            service_name,
            &namespace,
            &metadata,
            &slice_refs,
            self.include_not_ready,
        ))
    }

    // ── Endpoints path (fallback) ────────────────────────────────────────

    /// Discover all services via the legacy Endpoints API.
    async fn discover_via_endpoints(&self) -> Result<Vec<ServiceInstance>> {
        let services = self.services_api();
        let endpoints = self.endpoints_api();

        let svc_list = services
            .list(&self.service_list_params())
            .await
            .map_err(|e| Error::Discovery(format!("Failed to list services: {e}")))?;

        let ep_list = endpoints
            .list(&ListParams::default())
            .await
            .map_err(|e| Error::Discovery(format!("Failed to list endpoints: {e}")))?;

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
                    all_instances.extend(instances_from_endpoints(svc, ep));
                } else {
                    warn!(service = %name, "No endpoints found for service");
                }
            }
        }

        info!(
            count = all_instances.len(),
            "Discovered services from Kubernetes (Endpoints)"
        );
        Ok(all_instances)
    }

    async fn discover_service_via_endpoints(
        &self,
        service_name: &str,
    ) -> Result<Vec<ServiceInstance>> {
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

        Ok(instances_from_endpoints(&svc, &ep))
    }
}

#[async_trait]
impl DiscoveryProvider for K8sDiscovery {
    fn name(&self) -> &str {
        "kubernetes"
    }

    async fn discover_services(&self) -> Result<Vec<ServiceInstance>> {
        info!("Discovering all services from Kubernetes");

        if self.use_endpoint_slices {
            match self.discover_via_slices().await {
                Ok(instances) => return Ok(instances),
                Err(e) => {
                    warn!(error = %e, "EndpointSlice discovery failed, falling back to Endpoints");
                }
            }
        }
        self.discover_via_endpoints().await
    }

    async fn discover_service(&self, service_name: &str) -> Result<Vec<ServiceInstance>> {
        debug!(service = %service_name, "Discovering service from Kubernetes");

        if self.use_endpoint_slices {
            match self.discover_service_via_slices(service_name).await {
                Ok(instances) => return Ok(instances),
                Err(e) => {
                    warn!(service = %service_name, error = %e, "EndpointSlice discovery failed, falling back to Endpoints");
                }
            }
        }
        self.discover_service_via_endpoints(service_name).await
    }

    async fn watch_services(
        &self,
        callback: Box<dyn Fn(DiscoveryEvent) + Send + Sync>,
    ) -> Result<()> {
        // Watch EndpointSlices — these change on pod scale up/down, which a
        // Service-only watch would miss.
        info!("Starting Kubernetes EndpointSlice watch");

        let slices = self.endpoint_slices_api();
        let mut watch_stream = watcher(slices, WatcherConfig::default()).boxed();

        while let Some(event) = watch_stream.try_next().await.ok().flatten() {
            match event {
                watcher::Event::Apply(slice) => {
                    if let Some(svc_name) = owning_service(&slice) {
                        debug!(service = %svc_name, "EndpointSlice applied/updated");
                        if let Ok(instances) = self.discover_service(&svc_name).await {
                            for instance in instances {
                                callback(DiscoveryEvent::ServiceUpdated(instance));
                            }
                        }
                    }
                }
                watcher::Event::Delete(slice) => {
                    if let Some(svc_name) = owning_service(&slice) {
                        debug!(service = %svc_name, "EndpointSlice deleted");
                        callback(DiscoveryEvent::ServiceDeregistered {
                            service_id: svc_name.clone(),
                            service_name: svc_name,
                        });
                    }
                }
                watcher::Event::Init | watcher::Event::InitApply(_) | watcher::Event::InitDone => {}
            }
        }

        Ok(())
    }
}

/// Owning Service name of an EndpointSlice (from the `kubernetes.io/service-name` label).
fn owning_service(slice: &EndpointSlice) -> Option<String> {
    slice
        .metadata
        .labels
        .as_ref()
        .and_then(|labels| labels.get(SERVICE_NAME_LABEL).cloned())
}

/// Build [`ServiceMetadata`] from a Kubernetes Service object.
fn metadata_from_service(svc: &Service) -> ServiceMetadata {
    let labels = svc.metadata.labels.as_ref();
    ServiceMetadata {
        version: labels.and_then(|l| l.get("version").cloned()),
        tags: labels
            .map(|l| l.keys().cloned().collect())
            .unwrap_or_default(),
        datacenter: svc.metadata.namespace.clone(),
        custom: labels
            .map(|l| l.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default(),
    }
}

/// Merge a set of EndpointSlices for one Service into [`ServiceInstance`]s.
///
/// Honors each endpoint's `ready` condition (unless `include_not_ready`), skips
/// FQDN slices (no routable IPs), and produces one instance per address × port.
fn instances_from_slices(
    svc_name: &str,
    namespace: &str,
    metadata: &ServiceMetadata,
    slices: &[&EndpointSlice],
    include_not_ready: bool,
) -> Vec<ServiceInstance> {
    let mut instances = Vec::new();

    for slice in slices {
        // FQDN slices carry hostnames rather than routable IPs.
        if slice.address_type == "FQDN" {
            continue;
        }

        let ports: Vec<u16> = slice
            .ports
            .as_ref()
            .map(|ps| ps.iter().filter_map(|p| p.port).map(|p| p as u16).collect())
            .unwrap_or_default();

        for endpoint in &slice.endpoints {
            let ready = endpoint.conditions.as_ref().and_then(|c| c.ready);
            let healthy = ready == Some(true);
            if !healthy && !include_not_ready {
                continue;
            }
            let health = match ready {
                Some(true) => ServiceHealth::Healthy,
                Some(false) => ServiceHealth::Unhealthy,
                None => ServiceHealth::Unknown,
            };

            for ip in &endpoint.addresses {
                for &port_num in &ports {
                    instances.push(ServiceInstance {
                        id: format!("{namespace}:{svc_name}:{ip}:{port_num}"),
                        name: svc_name.to_string(),
                        address: ip.clone(),
                        port: port_num,
                        health,
                        metadata: metadata.clone(),
                        endpoints: vec![],
                    });
                }
            }
        }
    }

    instances
}

/// Parse a Service + legacy Endpoints object into [`ServiceInstance`]s.
fn instances_from_endpoints(svc: &Service, endpoints: &Endpoints) -> Vec<ServiceInstance> {
    let mut instances = Vec::new();

    let Some(svc_name) = svc.metadata.name.as_ref() else {
        return instances;
    };
    let namespace = svc.metadata.namespace.clone().unwrap_or_default();
    let metadata = metadata_from_service(svc);

    if let Some(subsets) = &endpoints.subsets {
        for subset in subsets {
            let Some(addresses) = &subset.addresses else {
                continue;
            };
            let Some(ports) = &subset.ports else {
                continue;
            };
            for address in addresses {
                let ip = &address.ip;
                for port in ports {
                    let port_num = port.port as u16;
                    instances.push(ServiceInstance {
                        id: format!("{namespace}:{svc_name}:{ip}:{port_num}"),
                        name: svc_name.clone(),
                        address: ip.clone(),
                        port: port_num,
                        health: ServiceHealth::Healthy, // K8s only lists ready endpoints
                        metadata: metadata.clone(),
                        endpoints: vec![],
                    });
                }
            }
        }
    }

    instances
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::discovery::v1::{Endpoint, EndpointConditions, EndpointPort};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use std::collections::BTreeMap;

    fn slice(
        name: &str,
        svc: &str,
        addrs_ready: &[(&str, Option<bool>)],
        port: i32,
    ) -> EndpointSlice {
        let mut labels = BTreeMap::new();
        labels.insert(SERVICE_NAME_LABEL.to_string(), svc.to_string());
        EndpointSlice {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some("default".to_string()),
                labels: Some(labels),
                ..Default::default()
            },
            address_type: "IPv4".to_string(),
            endpoints: addrs_ready
                .iter()
                .map(|(ip, ready)| Endpoint {
                    addresses: vec![ip.to_string()],
                    conditions: Some(EndpointConditions {
                        ready: *ready,
                        ..Default::default()
                    }),
                    ..Default::default()
                })
                .collect(),
            ports: Some(vec![EndpointPort {
                port: Some(port),
                ..Default::default()
            }]),
        }
    }

    #[test]
    fn k8s_config_default_prefers_slices() {
        let config = K8sConfig::default();
        assert!(config.namespace.is_none());
        assert!(config.use_endpoint_slices);
        assert!(!config.include_not_ready);
    }

    #[test]
    fn merges_ready_endpoints_across_slices() {
        let meta = ServiceMetadata::default();
        let s1 = slice("api-1", "api", &[("10.0.0.1", Some(true))], 8080);
        let s2 = slice("api-2", "api", &[("10.0.0.2", Some(true))], 8080);
        let refs = vec![&s1, &s2];

        let instances = instances_from_slices("api", "default", &meta, &refs, false);
        assert_eq!(instances.len(), 2, "both slices' ready endpoints included");
        assert_eq!(instances[0].id, "default:api:10.0.0.1:8080");
        assert!(instances.iter().all(|i| i.health == ServiceHealth::Healthy));
    }

    #[test]
    fn excludes_not_ready_by_default() {
        let meta = ServiceMetadata::default();
        let s = slice(
            "api-1",
            "api",
            &[("10.0.0.1", Some(true)), ("10.0.0.2", Some(false))],
            8080,
        );
        let refs = vec![&s];

        let ready_only = instances_from_slices("api", "default", &meta, &refs, false);
        assert_eq!(ready_only.len(), 1, "not-ready endpoint excluded");

        let all = instances_from_slices("api", "default", &meta, &refs, true);
        assert_eq!(all.len(), 2, "include_not_ready surfaces the not-ready one");
        assert!(all.iter().any(|i| i.health == ServiceHealth::Unhealthy));
    }

    #[test]
    fn scale_up_adds_instances() {
        // Simulates a scale event: a second slice/endpoint appears for the same
        // service. The merge picks it up (the bug the Service-only watch missed).
        let meta = ServiceMetadata::default();
        let before = vec![&slice("api-1", "api", &[("10.0.0.1", Some(true))], 8080)]
            .iter()
            .map(|s| (*s).clone())
            .collect::<Vec<_>>();
        let before_refs: Vec<&EndpointSlice> = before.iter().collect();
        assert_eq!(
            instances_from_slices("api", "default", &meta, &before_refs, false).len(),
            1
        );

        let after = vec![
            slice("api-1", "api", &[("10.0.0.1", Some(true))], 8080),
            slice("api-2", "api", &[("10.0.0.2", Some(true))], 8080),
        ];
        let after_refs: Vec<&EndpointSlice> = after.iter().collect();
        assert_eq!(
            instances_from_slices("api", "default", &meta, &after_refs, false).len(),
            2,
            "scaled-up pod is discovered"
        );
    }

    #[test]
    fn skips_fqdn_slices() {
        let meta = ServiceMetadata::default();
        let mut s = slice("ext-1", "ext", &[("example.com", Some(true))], 443);
        s.address_type = "FQDN".to_string();
        let refs = vec![&s];
        assert!(instances_from_slices("ext", "default", &meta, &refs, false).is_empty());
    }
}
