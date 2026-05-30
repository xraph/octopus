//! etcd service discovery implementation

use crate::provider::{
    DiscoveryEvent, DiscoveryProvider, ServiceHealth, ServiceInstance,
    ServiceMetadata,
};
use async_trait::async_trait;
use etcd_client::{Client, GetOptions, WatchOptions};
use octopus_core::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use tracing::{debug, error, info, warn};

/// etcd discovery configuration
#[derive(Debug, Clone)]
pub struct EtcdConfig {
    /// etcd endpoints (default: ["http://localhost:2379"])
    pub endpoints: Vec<String>,

    /// Key prefix for service entries (default: "/octopus/services/")
    pub key_prefix: String,

    /// Username for authentication
    pub username: Option<String>,

    /// Password for authentication
    pub password: Option<String>,

    /// Connection timeout (default: 5s)
    pub connect_timeout: Duration,

    /// Watch poll interval for fallback polling (default: 30s)
    pub watch_interval: Duration,
}

impl Default for EtcdConfig {
    fn default() -> Self {
        Self {
            endpoints: vec!["http://localhost:2379".to_string()],
            key_prefix: "/octopus/services/".to_string(),
            username: None,
            password: None,
            connect_timeout: Duration::from_secs(5),
            watch_interval: Duration::from_secs(30),
        }
    }
}

/// Service data stored in etcd as JSON
#[derive(Debug, Clone, Serialize, Deserialize)]
struct EtcdServiceEntry {
    id: String,
    name: String,
    address: String,
    port: u16,
    #[serde(default)]
    metadata: EtcdServiceMetadata,
}

/// Metadata portion of service entry
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct EtcdServiceMetadata {
    version: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    custom: HashMap<String, String>,
}

/// etcd service discovery provider
pub struct EtcdProvider {
    /// Configuration
    config: EtcdConfig,

    /// etcd client
    client: Client,
}

impl std::fmt::Debug for EtcdProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EtcdProvider")
            .field("config", &self.config)
            .finish()
    }
}

impl EtcdProvider {
    /// Create a new etcd provider with the given configuration.
    pub async fn new(config: EtcdConfig) -> Result<Self> {
        let connect_options = etcd_client::ConnectOptions::new()
            .with_timeout(config.connect_timeout);

        let connect_options = if let (Some(ref user), Some(ref pass)) =
            (&config.username, &config.password)
        {
            connect_options.with_user(user, pass)
        } else {
            connect_options
        };

        let client = Client::connect(&config.endpoints, Some(connect_options))
            .await
            .map_err(|e| Error::Discovery(format!("Failed to connect to etcd: {e}")))?;

        Ok(Self { config, client })
    }

    /// Create a provider with default configuration.
    pub async fn with_defaults() -> Result<Self> {
        Self::new(EtcdConfig::default()).await
    }

    /// Build the full key path for a service name.
    fn service_key(&self, name: &str) -> String {
        format!("{}{}", self.config.key_prefix, name)
    }

    /// Parse a JSON value from etcd into a ServiceInstance.
    fn parse_entry(&self, value: &[u8]) -> Result<ServiceInstance> {
        let entry: EtcdServiceEntry = serde_json::from_slice(value)
            .map_err(|e| Error::Discovery(format!("Failed to parse etcd service entry: {e}")))?;

        Ok(ServiceInstance {
            id: entry.id,
            name: entry.name,
            address: entry.address,
            port: entry.port,
            health: ServiceHealth::Healthy,
            metadata: ServiceMetadata {
                version: entry.metadata.version,
                tags: entry.metadata.tags,
                datacenter: None,
                custom: entry.metadata.custom,
            },
            endpoints: vec![],
        })
    }
}

#[async_trait]
impl DiscoveryProvider for EtcdProvider {
    fn name(&self) -> &str {
        "etcd"
    }

    async fn discover_services(&self) -> Result<Vec<ServiceInstance>> {
        debug!(prefix = %self.config.key_prefix, "Discovering all services from etcd");

        let mut client = self.client.clone();
        let options = GetOptions::new().with_prefix();

        let resp = client
            .get(self.config.key_prefix.as_bytes(), Some(options))
            .await
            .map_err(|e| Error::Discovery(format!("etcd get with prefix failed: {e}")))?;

        let mut instances = Vec::new();
        for kv in resp.kvs() {
            match self.parse_entry(kv.value()) {
                Ok(instance) => instances.push(instance),
                Err(e) => {
                    let key = String::from_utf8_lossy(kv.key());
                    warn!(key = %key, error = %e, "Failed to parse etcd service entry");
                }
            }
        }

        info!(count = instances.len(), "Discovered services from etcd");
        Ok(instances)
    }

    async fn discover_service(&self, service_name: &str) -> Result<Vec<ServiceInstance>> {
        debug!(service = %service_name, "Discovering service from etcd");

        let mut client = self.client.clone();
        let key = self.service_key(service_name);

        let resp = client
            .get(key.as_bytes(), None)
            .await
            .map_err(|e| Error::Discovery(format!("etcd get failed: {e}")))?;

        let instances: Vec<ServiceInstance> = resp
            .kvs()
            .iter()
            .filter_map(|kv| match self.parse_entry(kv.value()) {
                Ok(instance) => Some(instance),
                Err(e) => {
                    warn!(service = %service_name, error = %e, "Failed to parse service entry");
                    None
                }
            })
            .collect();

        debug!(
            service = %service_name,
            count = instances.len(),
            "Discovered service instances from etcd"
        );

        Ok(instances)
    }

    async fn watch_services(
        &self,
        callback: Box<dyn Fn(DiscoveryEvent) + Send + Sync>,
    ) -> Result<()> {
        info!(
            prefix = %self.config.key_prefix,
            "Starting etcd service watch"
        );

        let mut client = self.client.clone();
        let options = WatchOptions::new().with_prefix();

        let (mut watcher, mut stream) = client
            .watch(self.config.key_prefix.as_bytes(), Some(options))
            .await
            .map_err(|e| Error::Discovery(format!("etcd watch failed: {e}")))?;

        loop {
            let msg = tokio::time::timeout(
                self.config.watch_interval * 10,
                stream.message(),
            )
            .await;

            // Timeout just means no events in that window; keep going
            let resp = match msg {
                Err(_elapsed) => continue,
                Ok(inner) => inner,
            };

            match resp {
                Ok(Some(watch_resp)) => {
                    if watch_resp.canceled() {
                        warn!("etcd watch was canceled, stopping");
                        break;
                    }

                    for event in watch_resp.events() {
                        match event.event_type() {
                            etcd_client::EventType::Put => {
                                if let Some(kv) = event.kv() {
                                    match self.parse_entry(kv.value()) {
                                        Ok(instance) => {
                                            // If create_revision == mod_revision it's a new key
                                            if kv.create_revision() == kv.mod_revision() {
                                                debug!(
                                                    service = %instance.name,
                                                    "Service registered via etcd watch"
                                                );
                                                callback(DiscoveryEvent::ServiceRegistered(
                                                    instance,
                                                ));
                                            } else {
                                                debug!(
                                                    service = %instance.name,
                                                    "Service updated via etcd watch"
                                                );
                                                callback(DiscoveryEvent::ServiceUpdated(instance));
                                            }
                                        }
                                        Err(e) => {
                                            let key = String::from_utf8_lossy(kv.key());
                                            warn!(
                                                key = %key,
                                                error = %e,
                                                "Failed to parse etcd watch event"
                                            );
                                        }
                                    }
                                }
                            }
                            etcd_client::EventType::Delete => {
                                if let Some(kv) = event.kv() {
                                    let key = String::from_utf8_lossy(kv.key());
                                    // Extract service name from key
                                    let service_name = key
                                        .strip_prefix(&self.config.key_prefix)
                                        .unwrap_or(&key)
                                        .to_string();

                                    debug!(
                                        service = %service_name,
                                        "Service deregistered via etcd watch"
                                    );
                                    callback(DiscoveryEvent::ServiceDeregistered {
                                        service_id: service_name.clone(),
                                        service_name,
                                    });
                                }
                            }
                        }
                    }
                }
                Ok(None) => {
                    // Stream ended
                    debug!("etcd watch stream ended");
                    break;
                }
                Err(e) => {
                    error!(error = %e, "etcd watch stream error");
                    break;
                }
            }
        }

        let _ = watcher.cancel().await;
        Ok(())
    }

    async fn register_service(&self, instance: ServiceInstance) -> Result<()> {
        debug!(service = %instance.name, "Registering service in etcd");

        let mut client = self.client.clone();
        let key = self.service_key(&instance.name);

        let entry = EtcdServiceEntry {
            id: instance.id,
            name: instance.name.clone(),
            address: instance.address,
            port: instance.port,
            metadata: EtcdServiceMetadata {
                version: instance.metadata.version,
                tags: instance.metadata.tags,
                custom: instance.metadata.custom,
            },
        };

        let value = serde_json::to_string(&entry)
            .map_err(|e| Error::Discovery(format!("Failed to serialize service entry: {e}")))?;

        client
            .put(key.as_bytes(), value.as_bytes(), None)
            .await
            .map_err(|e| Error::Discovery(format!("etcd put failed: {e}")))?;

        info!(service = %instance.name, "Registered service in etcd");
        Ok(())
    }

    async fn deregister_service(&self, service_id: &str) -> Result<()> {
        debug!(service_id = %service_id, "Deregistering service from etcd");

        let mut client = self.client.clone();
        let key = self.service_key(service_id);

        client
            .delete(key.as_bytes(), None)
            .await
            .map_err(|e| Error::Discovery(format!("etcd delete failed: {e}")))?;

        info!(service_id = %service_id, "Deregistered service from etcd");
        Ok(())
    }

    async fn health_check(&self, _service_id: &str) -> Result<ServiceHealth> {
        // For etcd, we check connectivity as a health indicator
        let mut client = self.client.clone();
        match client.status().await {
            Ok(_) => Ok(ServiceHealth::Healthy),
            Err(_) => Ok(ServiceHealth::Unhealthy),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_etcd_config_default() {
        let config = EtcdConfig::default();
        assert_eq!(config.endpoints, vec!["http://localhost:2379"]);
        assert_eq!(config.key_prefix, "/octopus/services/");
        assert_eq!(config.connect_timeout, Duration::from_secs(5));
        assert!(config.username.is_none());
        assert!(config.password.is_none());
    }

    #[test]
    fn test_service_key_generation() {
        // We test the key generation logic directly
        let config = EtcdConfig::default();
        let key = format!("{}{}", config.key_prefix, "user-service");
        assert_eq!(key, "/octopus/services/user-service");
    }

    #[test]
    fn test_parse_service_entry_json() {
        let json = r#"{
            "id": "user-svc-1",
            "name": "user-service",
            "address": "10.0.1.5",
            "port": 8080,
            "metadata": {
                "version": "1.0.0",
                "tags": ["rest", "farp"],
                "custom": { "farp.enabled": "true" }
            }
        }"#;

        let entry: EtcdServiceEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.id, "user-svc-1");
        assert_eq!(entry.name, "user-service");
        assert_eq!(entry.address, "10.0.1.5");
        assert_eq!(entry.port, 8080);
        assert_eq!(entry.metadata.version, Some("1.0.0".to_string()));
        assert_eq!(entry.metadata.tags, vec!["rest", "farp"]);
        assert_eq!(
            entry.metadata.custom.get("farp.enabled"),
            Some(&"true".to_string())
        );
    }

    #[test]
    fn test_parse_service_entry_minimal_json() {
        let json = r#"{
            "id": "svc-1",
            "name": "minimal-service",
            "address": "127.0.0.1",
            "port": 3000
        }"#;

        let entry: EtcdServiceEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.id, "svc-1");
        assert_eq!(entry.name, "minimal-service");
        assert!(entry.metadata.version.is_none());
        assert!(entry.metadata.tags.is_empty());
        assert!(entry.metadata.custom.is_empty());
    }

    #[test]
    fn test_etcd_config_custom() {
        let config = EtcdConfig {
            endpoints: vec![
                "http://etcd1:2379".to_string(),
                "http://etcd2:2379".to_string(),
            ],
            key_prefix: "/myapp/services/".to_string(),
            username: Some("admin".to_string()),
            password: Some("secret".to_string()),
            connect_timeout: Duration::from_secs(10),
            watch_interval: Duration::from_secs(60),
        };

        assert_eq!(config.endpoints.len(), 2);
        assert_eq!(config.key_prefix, "/myapp/services/");
        assert_eq!(config.username, Some("admin".to_string()));
        assert_eq!(config.connect_timeout, Duration::from_secs(10));
    }

    #[test]
    fn test_serialize_service_entry() {
        let entry = EtcdServiceEntry {
            id: "test-1".to_string(),
            name: "test-service".to_string(),
            address: "10.0.0.1".to_string(),
            port: 8080,
            metadata: EtcdServiceMetadata {
                version: Some("2.0.0".to_string()),
                tags: vec!["grpc".to_string()],
                custom: HashMap::new(),
            },
        };

        let json = serde_json::to_string(&entry).unwrap();
        let parsed: EtcdServiceEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "test-1");
        assert_eq!(parsed.metadata.version, Some("2.0.0".to_string()));
    }
}
