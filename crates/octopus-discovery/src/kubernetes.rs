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
use k8s_openapi::api::core::v1::{Endpoints, Service, ServicePort};
use k8s_openapi::api::discovery::v1::EndpointSlice;
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
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
        service_list_params(self.label_selector.as_deref())
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
        let mut svc_meta: HashMap<(String, String), (ServiceMetadata, PortFilter)> = HashMap::new();
        for svc in &svc_list {
            if let (Some(name), Some(ns)) = (&svc.metadata.name, &svc.metadata.namespace) {
                let metadata = metadata_from_service(svc);
                let port_filter = PortFilter::new(&metadata, Some(svc));
                svc_meta.insert((ns.clone(), name.clone()), (metadata, port_filter));
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
            let Some((metadata, port_filter)) = svc_meta.get(&(ns.clone(), svc_name.clone()))
            else {
                continue;
            };
            all_instances.extend(instances_from_slices(
                &svc_name,
                &ns,
                metadata,
                port_filter,
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
        let (metadata, port_filter) = match services.get(service_name).await {
            Ok(svc) => {
                let metadata = metadata_from_service(&svc);
                let port_filter = PortFilter::new(&metadata, Some(&svc));
                (metadata, port_filter)
            }
            Err(e) => {
                debug!(service = %service_name, error = %e, "Service metadata unavailable, using defaults");
                (
                    ServiceMetadata {
                        datacenter: Some(namespace.clone()),
                        ..Default::default()
                    },
                    PortFilter::default(),
                )
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
            &port_filter,
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

/// List parameters for the Service list. The selector goes to the API server
/// unchanged, which ANDs comma-joined equality terms such as
/// `farp.enabled=true,team=payments`.
fn service_list_params(label_selector: Option<&str>) -> ListParams {
    let mut params = ListParams::default();
    if let Some(selector) = label_selector {
        params = params.labels(selector);
    }
    params
}

/// Prefix of the Service annotations that carry FARP opt-in metadata.
const FARP_PREFIX: &str = "farp.";

/// Key (label or annotation) that narrows discovery to one Service port.
const FARP_PORT_KEY: &str = "farp.port";

/// Build [`ServiceMetadata`] from a Kubernetes Service object.
///
/// Labels all go into the custom metadata. Annotations whose key starts with
/// `farp.` are merged on top and win over a label with the same key: label
/// values cannot hold `/`, so path keys such as `farp.openapi.path` have to be
/// annotations. Other annotations (for example
/// `kubectl.kubernetes.io/last-applied-configuration`) never leak in.
fn metadata_from_service(svc: &Service) -> ServiceMetadata {
    let labels = svc.metadata.labels.as_ref();
    let mut custom: HashMap<String, String> = labels
        .map(|l| l.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();
    if let Some(annotations) = svc.metadata.annotations.as_ref() {
        custom.extend(
            annotations
                .iter()
                .filter(|(k, _)| k.starts_with(FARP_PREFIX))
                .map(|(k, v)| (k.clone(), v.clone())),
        );
    }
    ServiceMetadata {
        version: labels.and_then(|l| l.get("version").cloned()),
        tags: labels
            .map(|l| l.keys().cloned().collect())
            .unwrap_or_default(),
        datacenter: svc.metadata.namespace.clone(),
        custom,
    }
}

/// Whether an endpoint port is selected by `farp.port`.
///
/// A number matches a port with that number, any other value matches the port
/// with that name, and no selector keeps every port. The selector is trimmed.
/// [`PortFilter`] adds the Service-port reading of a number on top of this.
fn port_selected(selector: Option<&str>, number: u16, name: Option<&str>) -> bool {
    let Some(selector) = selector.map(str::trim) else {
        return true;
    };
    match selector.parse::<u16>() {
        Ok(wanted) => wanted == number,
        Err(_) => name == Some(selector),
    }
}

/// The `farp.port` selector of a Service, with the Service's own ports so a
/// number can be read as a Service port.
///
/// EndpointSlice and Endpoints list the target (container) port numbers, while
/// only the port names come from the Service. A numeric selector therefore
/// matches either a Service port number, resolved to that port's name (or, for
/// an unnamed Service port, its numeric `targetPort`), or an endpoint port with
/// that number directly. A non-numeric selector matches the port name.
#[derive(Debug, Clone, Default)]
struct PortFilter {
    selector: Option<String>,
    service_ports: Vec<ServicePort>,
}

impl PortFilter {
    fn new(metadata: &ServiceMetadata, svc: Option<&Service>) -> Self {
        Self {
            selector: metadata
                .custom
                .get(FARP_PORT_KEY)
                .map(|s| s.trim().to_string()),
            service_ports: svc
                .and_then(|s| s.spec.as_ref())
                .and_then(|spec| spec.ports.clone())
                .unwrap_or_default(),
        }
    }

    fn selector(&self) -> Option<&str> {
        self.selector.as_deref()
    }

    /// Whether the endpoint port (`number`, `name`) is kept.
    fn accepts(&self, number: u16, name: Option<&str>) -> bool {
        let Some(selector) = self.selector() else {
            return true;
        };
        if port_selected(Some(selector), number, name) {
            return true;
        }
        let Ok(wanted) = selector.parse::<u16>() else {
            return false;
        };
        self.service_ports
            .iter()
            .filter(|sp| sp.port == i32::from(wanted))
            .any(|sp| match sp.name.as_deref() {
                Some(service_port_name) => name == Some(service_port_name),
                None => matches!(
                    &sp.target_port,
                    Some(IntOrString::Int(target)) if *target == i32::from(number)
                ),
            })
    }

    /// Log when a selector is set and no port of the Service matched it.
    fn warn_if_nothing_matched(&self, svc_name: &str, namespace: &str, seen: bool, matched: bool) {
        if let (Some(selector), true, false) = (self.selector(), seen, matched) {
            warn!(
                service = %svc_name,
                namespace = %namespace,
                farp_port = %selector,
                "farp.port matches no port of the Service, so it has no instances"
            );
        }
    }
}

/// Merge a set of EndpointSlices for one Service into [`ServiceInstance`]s.
///
/// Honors each endpoint's `ready` condition (unless `include_not_ready`), skips
/// FQDN slices (no routable IPs), and produces one instance per address × port.
/// `farp.port` on the Service narrows the ports to one.
fn instances_from_slices(
    svc_name: &str,
    namespace: &str,
    metadata: &ServiceMetadata,
    port_filter: &PortFilter,
    slices: &[&EndpointSlice],
    include_not_ready: bool,
) -> Vec<ServiceInstance> {
    let mut instances = Vec::new();
    let mut ports_seen = false;
    let mut ports_matched = false;

    for slice in slices {
        // FQDN slices carry hostnames rather than routable IPs.
        if slice.address_type == "FQDN" {
            continue;
        }

        let ports: Vec<u16> = slice
            .ports
            .as_ref()
            .map(|ps| {
                ps.iter()
                    .filter_map(|p| p.port.map(|num| (num as u16, p.name.as_deref())))
                    .inspect(|_| ports_seen = true)
                    .filter(|(num, name)| port_filter.accepts(*num, *name))
                    .map(|(num, _)| num)
                    .collect()
            })
            .unwrap_or_default();
        ports_matched |= !ports.is_empty();

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

    port_filter.warn_if_nothing_matched(svc_name, namespace, ports_seen, ports_matched);
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
    let port_filter = PortFilter::new(&metadata, Some(svc));
    let mut ports_seen = false;
    let mut ports_matched = false;

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
                    ports_seen = true;
                    if !port_filter.accepts(port_num, port.name.as_deref()) {
                        continue;
                    }
                    ports_matched = true;
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

    port_filter.warn_if_nothing_matched(svc_name, &namespace, ports_seen, ports_matched);
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

        let instances = instances_from_slices(
            "api",
            "default",
            &meta,
            &PortFilter::default(),
            &refs,
            false,
        );
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

        let ready_only = instances_from_slices(
            "api",
            "default",
            &meta,
            &PortFilter::default(),
            &refs,
            false,
        );
        assert_eq!(ready_only.len(), 1, "not-ready endpoint excluded");

        let all =
            instances_from_slices("api", "default", &meta, &PortFilter::default(), &refs, true);
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
            instances_from_slices(
                "api",
                "default",
                &meta,
                &PortFilter::default(),
                &before_refs,
                false
            )
            .len(),
            1
        );

        let after = vec![
            slice("api-1", "api", &[("10.0.0.1", Some(true))], 8080),
            slice("api-2", "api", &[("10.0.0.2", Some(true))], 8080),
        ];
        let after_refs: Vec<&EndpointSlice> = after.iter().collect();
        assert_eq!(
            instances_from_slices(
                "api",
                "default",
                &meta,
                &PortFilter::default(),
                &after_refs,
                false
            )
            .len(),
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
        assert!(instances_from_slices(
            "ext",
            "default",
            &meta,
            &PortFilter::default(),
            &refs,
            false
        )
        .is_empty());
    }

    fn named_port(name: &str, port: i32) -> EndpointPort {
        EndpointPort {
            name: Some(name.to_string()),
            port: Some(port),
            ..Default::default()
        }
    }

    fn two_port_slice() -> EndpointSlice {
        let mut s = slice("web-1", "web", &[("10.0.0.1", Some(true))], 8080);
        s.ports = Some(vec![named_port("http", 8080), named_port("metrics", 9100)]);
        s
    }

    fn service(
        labels: &[(&str, &str)],
        annotations: &[(&str, &str)],
    ) -> k8s_openapi::api::core::v1::Service {
        let to_map = |pairs: &[(&str, &str)]| -> BTreeMap<String, String> {
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        };
        k8s_openapi::api::core::v1::Service {
            metadata: ObjectMeta {
                name: Some("web".to_string()),
                namespace: Some("default".to_string()),
                labels: Some(to_map(labels)),
                annotations: Some(to_map(annotations)),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn ports_of(instances: &[ServiceInstance]) -> Vec<u16> {
        let mut ports: Vec<u16> = instances.iter().map(|i| i.port).collect();
        ports.sort_unstable();
        ports
    }

    #[test]
    fn annotation_metadata_overrides_labels() {
        let svc = service(
            &[("farp.enabled", "true"), ("farp.openapi.path", "a")],
            &[("farp.openapi.path", "/v1/openapi.json")],
        );
        let meta = metadata_from_service(&svc);
        assert_eq!(meta.custom["farp.openapi.path"], "/v1/openapi.json");
        assert_eq!(meta.custom["farp.enabled"], "true");
    }

    #[test]
    fn farp_annotations_fill_in_keys_labels_cannot_hold() {
        let svc = service(
            &[("farp.enabled", "true")],
            &[
                ("farp.openapi.path", "/docs/openapi.json"),
                ("farp.health.path", "/healthz"),
            ],
        );
        let meta = metadata_from_service(&svc);
        assert_eq!(meta.custom["farp.openapi.path"], "/docs/openapi.json");
        assert_eq!(meta.custom["farp.health.path"], "/healthz");
        // Tags still come from labels only.
        assert_eq!(meta.tags, vec!["farp.enabled".to_string()]);
    }

    #[test]
    fn non_farp_annotations_are_ignored() {
        let svc = service(
            &[("farp.enabled", "true")],
            &[
                (
                    "kubectl.kubernetes.io/last-applied-configuration",
                    "{\"big\":\"blob\"}",
                ),
                ("loomplane.farp.gateway", "gw"),
            ],
        );
        let meta = metadata_from_service(&svc);
        assert!(!meta
            .custom
            .contains_key("kubectl.kubernetes.io/last-applied-configuration"));
        assert!(!meta.custom.contains_key("loomplane.farp.gateway"));
    }

    #[test]
    fn farp_port_selects_named_port() {
        let svc = service(&[("farp.port", "http")], &[]);
        let meta = metadata_from_service(&svc);
        let s = two_port_slice();
        let found = instances_from_slices(
            "web",
            "default",
            &meta,
            &PortFilter::new(&meta, Some(&svc)),
            &[&s],
            false,
        );
        assert_eq!(ports_of(&found), vec![8080]);
    }

    #[test]
    fn farp_port_selects_numeric_port() {
        let svc = service(&[], &[("farp.port", "9100")]);
        let meta = metadata_from_service(&svc);
        let s = two_port_slice();
        let found = instances_from_slices(
            "web",
            "default",
            &meta,
            &PortFilter::new(&meta, Some(&svc)),
            &[&s],
            false,
        );
        assert_eq!(ports_of(&found), vec![9100]);
    }

    #[test]
    fn no_farp_port_keeps_every_port() {
        let svc = service(&[("farp.enabled", "true")], &[]);
        let meta = metadata_from_service(&svc);
        let s = two_port_slice();
        let found = instances_from_slices(
            "web",
            "default",
            &meta,
            &PortFilter::new(&meta, Some(&svc)),
            &[&s],
            false,
        );
        assert_eq!(ports_of(&found), vec![8080, 9100]);
    }

    #[test]
    fn unknown_farp_port_keeps_nothing() {
        let svc = service(&[("farp.port", "grpc")], &[]);
        let meta = metadata_from_service(&svc);
        let s = two_port_slice();
        assert!(instances_from_slices(
            "web",
            "default",
            &meta,
            &PortFilter::new(&meta, Some(&svc)),
            &[&s],
            false,
        )
        .is_empty());
    }

    #[test]
    fn port_selected_matches_number_or_name() {
        assert!(port_selected(None, 8080, Some("http")));
        assert!(port_selected(None, 8080, None));
        assert!(port_selected(Some("8080"), 8080, Some("http")));
        assert!(!port_selected(Some("8080"), 9100, Some("metrics")));
        assert!(port_selected(Some("http"), 8080, Some("http")));
        assert!(!port_selected(Some("http"), 8080, None));
        assert!(!port_selected(Some("http"), 9100, Some("metrics")));
    }

    #[test]
    fn farp_port_filters_legacy_endpoints() {
        use k8s_openapi::api::core::v1::{
            EndpointAddress, EndpointPort as CoreEndpointPort, EndpointSubset,
        };
        let endpoints = Endpoints {
            subsets: Some(vec![EndpointSubset {
                addresses: Some(vec![EndpointAddress {
                    ip: "10.0.0.1".to_string(),
                    ..Default::default()
                }]),
                ports: Some(vec![
                    CoreEndpointPort {
                        name: Some("http".to_string()),
                        port: 8080,
                        ..Default::default()
                    },
                    CoreEndpointPort {
                        name: Some("metrics".to_string()),
                        port: 9100,
                        ..Default::default()
                    },
                ]),
                ..Default::default()
            }]),
            ..Default::default()
        };

        let all = instances_from_endpoints(&service(&[], &[]), &endpoints);
        assert_eq!(ports_of(&all), vec![8080, 9100]);

        let named =
            instances_from_endpoints(&service(&[("farp.port", "metrics")], &[]), &endpoints);
        assert_eq!(ports_of(&named), vec![9100]);

        let numeric = instances_from_endpoints(&service(&[], &[("farp.port", "8080")]), &endpoints);
        assert_eq!(ports_of(&numeric), vec![8080]);
    }

    #[test]
    fn comma_joined_label_selector_is_passed_through() {
        let selector = "farp.enabled=true,loomplane.environment=env-1,loomplane.farp.gateway=gw";
        let config = K8sConfig {
            label_selector: Some(selector.into()),
            ..Default::default()
        };
        // The apiserver ANDs comma-joined equality terms, so the string must
        // reach it unchanged.
        assert_eq!(
            service_list_params(config.label_selector.as_deref())
                .label_selector
                .as_deref(),
            Some(selector)
        );
        assert_eq!(service_list_params(None).label_selector, None);
    }

    use k8s_openapi::api::core::v1::{ServicePort, ServiceSpec};
    use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;

    /// A Service carrying `farp.port` and the given ports.
    fn service_with_ports(farp_port: &str, ports: Vec<ServicePort>) -> Service {
        let mut svc = service(&[("farp.port", farp_port)], &[]);
        svc.spec = Some(ServiceSpec {
            ports: Some(ports),
            ..Default::default()
        });
        svc
    }

    fn service_port(name: Option<&str>, port: i32, target: Option<IntOrString>) -> ServicePort {
        ServicePort {
            name: name.map(str::to_string),
            port,
            target_port: target,
            ..Default::default()
        }
    }

    /// One ready endpoint on `port`, with the port name an EndpointSlice would
    /// copy from the Service port.
    fn target_port_slice(name: Option<&str>, port: i32) -> EndpointSlice {
        let mut s = slice("web-1", "web", &[("10.0.0.1", Some(true))], port);
        s.ports = Some(vec![EndpointPort {
            name: name.map(str::to_string),
            port: Some(port),
            ..Default::default()
        }]);
        s
    }

    fn discover(svc: &Service, s: &EndpointSlice) -> Vec<ServiceInstance> {
        let meta = metadata_from_service(svc);
        instances_from_slices(
            "web",
            "default",
            &meta,
            &PortFilter::new(&meta, Some(svc)),
            &[s],
            false,
        )
    }

    #[test]
    fn numeric_farp_port_matches_a_named_service_port_through_its_name() {
        // port 80 -> targetPort 3000; the slice lists 3000 under the name "http".
        let s = target_port_slice(Some("http"), 3000);
        for selector in ["80", "3000", "http", " http "] {
            let svc = service_with_ports(
                selector,
                vec![service_port(Some("http"), 80, Some(IntOrString::Int(3000)))],
            );
            assert_eq!(ports_of(&discover(&svc, &s)), vec![3000], "{selector:?}");
        }
    }

    #[test]
    fn numeric_farp_port_matches_an_unnamed_service_port_through_its_target_port() {
        let s = target_port_slice(None, 3000);
        for selector in ["80", "3000"] {
            let svc = service_with_ports(
                selector,
                vec![service_port(None, 80, Some(IntOrString::Int(3000)))],
            );
            assert_eq!(ports_of(&discover(&svc, &s)), vec![3000], "{selector:?}");
        }
    }

    #[test]
    fn numeric_service_port_with_a_named_target_port_resolves_by_name() {
        let s = target_port_slice(Some("web"), 3000);
        let svc = service_with_ports(
            "80",
            vec![service_port(
                Some("web"),
                80,
                Some(IntOrString::String("app".to_string())),
            )],
        );
        assert_eq!(ports_of(&discover(&svc, &s)), vec![3000]);
    }

    #[test]
    fn farp_port_picks_the_right_service_port_among_several() {
        let mut s = target_port_slice(Some("http"), 3000);
        s.ports = Some(vec![named_port("http", 3000), named_port("metrics", 9100)]);
        let svc = service_with_ports(
            "80",
            vec![
                service_port(Some("http"), 80, Some(IntOrString::Int(3000))),
                service_port(Some("metrics"), 9090, Some(IntOrString::Int(9100))),
            ],
        );
        assert_eq!(ports_of(&discover(&svc, &s)), vec![3000]);
    }

    #[test]
    fn farp_port_that_matches_nothing_keeps_no_instances() {
        // 81 is neither a Service port nor an endpoint port. The warning that
        // names the Service is logged; the observable result is no instances.
        let s = target_port_slice(Some("http"), 3000);
        let svc = service_with_ports(
            "81",
            vec![service_port(Some("http"), 80, Some(IntOrString::Int(3000)))],
        );
        assert!(discover(&svc, &s).is_empty());
    }

    #[test]
    fn farp_port_resolves_service_ports_for_legacy_endpoints() {
        use k8s_openapi::api::core::v1::{
            EndpointAddress, EndpointPort as CoreEndpointPort, EndpointSubset,
        };
        let endpoints = Endpoints {
            subsets: Some(vec![EndpointSubset {
                addresses: Some(vec![EndpointAddress {
                    ip: "10.0.0.1".to_string(),
                    ..Default::default()
                }]),
                ports: Some(vec![CoreEndpointPort {
                    name: Some("http".to_string()),
                    port: 3000,
                    ..Default::default()
                }]),
                ..Default::default()
            }]),
            ..Default::default()
        };
        for selector in ["80", "3000", "http"] {
            let svc = service_with_ports(
                selector,
                vec![service_port(Some("http"), 80, Some(IntOrString::Int(3000)))],
            );
            assert_eq!(
                ports_of(&instances_from_endpoints(&svc, &endpoints)),
                vec![3000],
                "{selector:?}"
            );
        }
    }

    #[test]
    fn port_selected_trims_the_selector() {
        assert!(port_selected(Some(" 8080 "), 8080, None));
        assert!(port_selected(Some(" http "), 8080, Some("http")));
    }
}
