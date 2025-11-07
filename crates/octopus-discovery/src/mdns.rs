//! mDNS/Bonjour service discovery for local development
//!
//! This module provides multicast DNS service discovery, allowing services
//! on the local network to advertise and discover each other without
//! centralized configuration. Useful for Mac/Windows local development.

use crate::provider::{
    DiscoveryEvent, DiscoveryProvider, ServiceEndpoint, ServiceHealth, ServiceInstance,
    ServiceMetadata,
};
use async_trait::async_trait;
use octopus_core::{Error, Result};
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{debug, error, info};

/// mDNS service discovery for local networks
///
/// Uses multicast DNS (Bonjour/Avahi) to discover services on the local network.
/// Services advertise themselves with a specific service type (e.g., `_octopus._tcp.local.`)
/// and the gateway watches for these advertisements.
#[derive(Debug)]
pub struct MdnsDiscovery {
    config: MdnsConfig,
    discovered_services: Arc<RwLock<HashMap<String, ServiceInstance>>>,
}

/// mDNS discovery configuration
#[derive(Debug, Clone)]
pub struct MdnsConfig {
    /// Service type to discover (e.g., "_octopus._tcp.local.")
    pub service_type: String,

    /// Domain (usually "local.")
    pub domain: String,

    /// Watch interval for re-discovery
    pub watch_interval: Duration,

    /// Query timeout
    pub query_timeout: Duration,

    /// Enable IPv6 address registration (filters discovered addresses only)
    ///
    /// When false, only IPv4 addresses will be registered with FARP.
    /// **Note**: This does NOT prevent mDNS from querying IPv6 interfaces.
    /// VPN tunnel errors will still occur as they are logged by the mdns-sd library.
    pub enable_ipv6: bool,
}

impl Default for MdnsConfig {
    fn default() -> Self {
        // Prefer IPv4 addresses on macOS (still queries IPv6 interfaces)
        #[cfg(target_os = "macos")]
        let enable_ipv6 = false;

        #[cfg(not(target_os = "macos"))]
        let enable_ipv6 = true;

        Self {
            service_type: "_octopus._tcp".to_string(),
            domain: "local.".to_string(),
            watch_interval: Duration::from_secs(30),
            query_timeout: Duration::from_secs(5),
            enable_ipv6,
        }
    }
}

impl MdnsConfig {
    /// Create a new mDNS config with custom service type
    pub fn new(service_type: impl Into<String>) -> Self {
        Self {
            service_type: service_type.into(),
            ..Default::default()
        }
    }

    /// Set the domain
    pub fn with_domain(mut self, domain: impl Into<String>) -> Self {
        self.domain = domain.into();
        self
    }

    /// Set watch interval
    pub fn with_watch_interval(mut self, interval: Duration) -> Self {
        self.watch_interval = interval;
        self
    }

    /// Enable or disable IPv6 address registration
    ///
    /// When `false`, only IPv4 addresses from discovered services will be registered
    /// with FARP. IPv6 addresses will be filtered out.
    ///
    /// **Note**: This does NOT prevent mDNS from querying IPv6 interfaces or stop
    /// VPN interface errors. It only controls which service addresses get registered.
    ///
    /// Useful for:
    /// - Forcing IPv4-only service communication
    /// - Avoiding IPv6 routing/firewall issues
    /// - Networks with unreliable IPv6 connectivity
    ///
    /// Default: `false` on macOS (to prefer IPv4), `true` on other platforms
    pub fn with_ipv6(mut self, enable: bool) -> Self {
        self.enable_ipv6 = enable;
        self
    }

    /// Get full service name (type.domain)
    pub fn full_service_name(&self) -> String {
        format!("{}.{}", self.service_type, self.domain)
    }
}

impl MdnsDiscovery {
    /// Create a new mDNS discovery client
    pub fn new(config: MdnsConfig) -> Self {
        info!(
            service_type = %config.service_type,
            domain = %config.domain,
            "Initializing mDNS discovery"
        );

        Self {
            config,
            discovered_services: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create with default configuration
    pub fn with_defaults() -> Self {
        Self::new(MdnsConfig::default())
    }

    /// Parse TXT records into metadata
    fn parse_txt_records(&self, txt_records: &[String]) -> ServiceMetadata {
        let mut metadata = ServiceMetadata::default();
        let mut custom = HashMap::new();

        for record in txt_records {
            if let Some((key, value)) = record.split_once('=') {
                match key {
                    "version" => metadata.version = Some(value.to_string()),
                    "dc" | "datacenter" => metadata.datacenter = Some(value.to_string()),
                    "tags" => {
                        metadata.tags = value.split(',').map(|s| s.to_string()).collect();
                    }
                    "openapi" => {
                        custom.insert("openapi_url".to_string(), value.to_string());
                    }
                    "asyncapi" => {
                        custom.insert("asyncapi_url".to_string(), value.to_string());
                    }
                    "graphql" => {
                        custom.insert("graphql_url".to_string(), value.to_string());
                    }
                    "health" => {
                        custom.insert("health_url".to_string(), value.to_string());
                    }
                    _ => {
                        custom.insert(key.to_string(), value.to_string());
                    }
                }
            }
        }

        metadata.custom = custom;
        metadata
    }

    /// Parse endpoints from metadata
    fn parse_endpoints(&self, metadata: &ServiceMetadata) -> Vec<ServiceEndpoint> {
        let mut endpoints = Vec::new();

        // Extract common endpoints from custom metadata
        if let Some(openapi_path) = metadata.custom.get("openapi_url") {
            endpoints.push(ServiceEndpoint {
                path: openapi_path.clone(),
                methods: vec!["GET".to_string()],
                metadata: {
                    let mut m = HashMap::new();
                    m.insert("type".to_string(), "openapi".to_string());
                    m
                },
            });
        }

        if let Some(health_path) = metadata.custom.get("health_url") {
            endpoints.push(ServiceEndpoint {
                path: health_path.clone(),
                methods: vec!["GET".to_string()],
                metadata: {
                    let mut m = HashMap::new();
                    m.insert("type".to_string(), "health".to_string());
                    m
                },
            });
        }

        endpoints
    }

    /// Perform mDNS query for services
    async fn query_services(&self) -> Result<Vec<ServiceInstance>> {
        use mdns_sd::{ServiceDaemon, ServiceEvent as MdnsEvent};

        let full_service_name = self.config.full_service_name();
        debug!(service = %full_service_name, "Querying mDNS services");

        // Create mDNS daemon
        let mdns = ServiceDaemon::new()
            .map_err(|e| Error::Discovery(format!("Failed to create mDNS daemon: {e}")))?;

        // Browse for services
        let receiver = mdns
            .browse(&full_service_name)
            .map_err(|e| Error::Discovery(format!("Failed to browse mDNS: {e}")))?;

        let mut instances = Vec::new();
        let timeout = tokio::time::sleep(self.config.query_timeout);
        tokio::pin!(timeout);

        loop {
            tokio::select! {
                event = receiver.recv_async() => {
                    match event {
                        Ok(MdnsEvent::ServiceResolved(info)) => {
                            debug!(
                                service_name = %info.get_fullname(),
                                hostname = %info.get_hostname(),
                                "Service resolved via mDNS"
                            );

                            let addresses = info.get_addresses();
                            let port = info.get_port();
                            let txt_records: Vec<String> = info
                                .get_properties()
                                .iter()
                                .filter_map(|prop| {
                                    let key = prop.key();
                                    let val = prop.val_str();
                                    Some(format!("{key}={val}"))
                                })
                                .collect();

                            let metadata = self.parse_txt_records(&txt_records);
                            let endpoints = self.parse_endpoints(&metadata);

                            // Create instance for each address
                            for addr in addresses.iter() {
                                // Filter IPv6 if disabled
                                if !self.config.enable_ipv6 && matches!(addr, IpAddr::V6(_)) {
                                    continue;
                                }

                                let instance = ServiceInstance {
                                    id: format!("{}:{}:{}", info.get_fullname(), addr, port),
                                    name: info
                                        .get_fullname()
                                        .strip_suffix(&format!(".{full_service_name}"))
                                        .unwrap_or(info.get_fullname())
                                        .to_string(),
                                    address: addr.to_string(),
                                    port,
                                    health: ServiceHealth::Unknown,
                                    metadata: metadata.clone(),
                                    endpoints: endpoints.clone(),
                                };

                                info!(
                                    service = %instance.name,
                                    address = %instance.address,
                                    port = instance.port,
                                    "Discovered service via mDNS"
                                );

                                instances.push(instance);
                            }
                        }
                        Ok(MdnsEvent::ServiceRemoved(_, fullname)) => {
                            debug!(service = %fullname, "Service removed from mDNS");
                        }
                        Err(e) => {
                            error!(error = %e, "mDNS receiver error");
                            break;
                        }
                        _ => {}
                    }
                }
                _ = &mut timeout => {
                    debug!("mDNS query timeout reached");
                    break;
                }
            }
        }

        // Shutdown daemon (ignore errors as they're often harmless)
        if let Err(e) = mdns.shutdown() {
            // Only log debug - shutdown errors are common and usually harmless
            // (e.g., "sending on a closed channel" when the receiver is already dropped)
            debug!(error = %e, "mDNS daemon shutdown returned error (usually harmless)");
        }

        Ok(instances)
    }
}

#[async_trait]
impl DiscoveryProvider for MdnsDiscovery {
    fn name(&self) -> &str {
        "mdns"
    }

    async fn discover_services(&self) -> Result<Vec<ServiceInstance>> {
        info!(
            service_type = %self.config.service_type,
            "Discovering all services via mDNS"
        );

        let instances = self.query_services().await?;

        // Update cached services
        let mut cache = self.discovered_services.write().await;
        cache.clear();
        for instance in &instances {
            cache.insert(instance.id.clone(), instance.clone());
        }

        info!(count = instances.len(), "Discovered services via mDNS");
        Ok(instances)
    }

    async fn discover_service(&self, service_name: &str) -> Result<Vec<ServiceInstance>> {
        info!(service = %service_name, "Discovering specific service via mDNS");

        let all_instances = self.query_services().await?;

        let filtered: Vec<ServiceInstance> = all_instances
            .into_iter()
            .filter(|instance| instance.name == service_name)
            .collect();

        info!(
            service = %service_name,
            count = filtered.len(),
            "Filtered service instances"
        );

        Ok(filtered)
    }

    async fn watch_services(
        &self,
        callback: Box<dyn Fn(DiscoveryEvent) + Send + Sync>,
    ) -> Result<()> {
        info!(
            service_type = %self.config.service_type,
            interval = ?self.config.watch_interval,
            "Starting mDNS service watcher"
        );

        let mut interval = tokio::time::interval(self.config.watch_interval);

        loop {
            interval.tick().await;

            debug!("Polling mDNS for service changes");

            match self.query_services().await {
                Ok(current_instances) => {
                    let mut cache = self.discovered_services.write().await;
                    let mut current_ids = std::collections::HashSet::new();

                    // Check for new or updated services
                    for instance in current_instances {
                        current_ids.insert(instance.id.clone());

                        if let Some(old_instance) = cache.get(&instance.id) {
                            // Check if instance changed
                            if old_instance != &instance {
                                debug!(service = %instance.name, "Service updated");
                                callback(DiscoveryEvent::ServiceUpdated(instance.clone()));
                            }
                        } else {
                            // New service
                            info!(service = %instance.name, "New service registered");
                            callback(DiscoveryEvent::ServiceRegistered(instance.clone()));
                        }

                        cache.insert(instance.id.clone(), instance);
                    }

                    // Check for removed services
                    let removed_ids: Vec<String> = cache
                        .keys()
                        .filter(|id| !current_ids.contains(*id))
                        .cloned()
                        .collect();

                    for removed_id in removed_ids {
                        if let Some(removed_instance) = cache.remove(&removed_id) {
                            info!(service = %removed_instance.name, "Service deregistered");
                            callback(DiscoveryEvent::ServiceDeregistered {
                                service_id: removed_instance.id.clone(),
                                service_name: removed_instance.name.clone(),
                            });
                        }
                    }
                }
                Err(e) => {
                    error!(error = %e, "Failed to query mDNS services");
                }
            }
        }
    }

    async fn register_service(&self, _instance: ServiceInstance) -> Result<()> {
        Err(Error::Discovery(
            "mDNS service registration should be done by individual services using mDNS libraries"
                .to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mdns_config_default() {
        let config = MdnsConfig::default();
        assert_eq!(config.service_type, "_octopus._tcp");
        assert_eq!(config.domain, "local.");
        assert_eq!(config.full_service_name(), "_octopus._tcp.local.");
    }

    #[test]
    fn test_mdns_config_custom() {
        let config = MdnsConfig::new("_myapp._tcp")
            .with_domain("mynet.local.")
            .with_watch_interval(Duration::from_secs(60));

        assert_eq!(config.service_type, "_myapp._tcp");
        assert_eq!(config.domain, "mynet.local.");
        assert_eq!(config.watch_interval, Duration::from_secs(60));
        assert_eq!(config.full_service_name(), "_myapp._tcp.mynet.local.");
    }

    #[test]
    fn test_parse_txt_records() {
        let discovery = MdnsDiscovery::with_defaults();

        let txt = vec![
            "version=1.0.0".to_string(),
            "tags=api,production".to_string(),
            "openapi=/api/openapi.json".to_string(),
            "health=/health".to_string(),
            "custom_field=custom_value".to_string(),
        ];

        let metadata = discovery.parse_txt_records(&txt);

        assert_eq!(metadata.version, Some("1.0.0".to_string()));
        assert_eq!(metadata.tags, vec!["api", "production"]);
        assert_eq!(
            metadata.custom.get("openapi_url"),
            Some(&"/api/openapi.json".to_string())
        );
        assert_eq!(
            metadata.custom.get("health_url"),
            Some(&"/health".to_string())
        );
        assert_eq!(
            metadata.custom.get("custom_field"),
            Some(&"custom_value".to_string())
        );
    }

    #[test]
    fn test_parse_endpoints() {
        let discovery = MdnsDiscovery::with_defaults();

        let mut custom = HashMap::new();
        custom.insert("openapi_url".to_string(), "/api/openapi.json".to_string());
        custom.insert("health_url".to_string(), "/health".to_string());

        let metadata = ServiceMetadata {
            version: Some("1.0.0".to_string()),
            tags: vec![],
            datacenter: None,
            custom,
        };

        let endpoints = discovery.parse_endpoints(&metadata);

        assert_eq!(endpoints.len(), 2);
        assert!(endpoints.iter().any(|e| e.path == "/api/openapi.json"
            && e.metadata.get("type") == Some(&"openapi".to_string())));
        assert!(endpoints
            .iter()
            .any(|e| e.path == "/health" && e.metadata.get("type") == Some(&"health".to_string())));
    }

    #[tokio::test]
    async fn test_mdns_discovery_creation() {
        let discovery = MdnsDiscovery::with_defaults();
        assert_eq!(discovery.name(), "mdns");
    }
}
