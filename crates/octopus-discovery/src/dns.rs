//! DNS-based service discovery

use crate::provider::{
    DiscoveryEvent, DiscoveryProvider, ServiceHealth, ServiceInstance, ServiceMetadata,
};
use async_trait::async_trait;
use octopus_core::{Error, Result};
use std::net::IpAddr;
use std::time::Duration;
use tokio::time;
use tracing::{debug, error, info};
use trust_dns_resolver::config::{ResolverConfig, ResolverOpts};
use trust_dns_resolver::TokioAsyncResolver;

/// DNS service discovery
#[derive(Debug, Clone)]
pub struct DnsDiscovery {
    resolver: TokioAsyncResolver,
    default_port: u16,
    watch_interval: Duration,
}

/// DNS discovery configuration
#[derive(Debug, Clone)]
pub struct DnsConfig {
    /// Default port for discovered services
    pub default_port: u16,

    /// Watch interval for DNS changes
    pub watch_interval: Duration,

    /// Custom DNS resolver config
    pub resolver_config: Option<ResolverConfig>,
}

impl Default for DnsConfig {
    fn default() -> Self {
        Self {
            default_port: 80,
            watch_interval: Duration::from_secs(60),
            resolver_config: None,
        }
    }
}

impl DnsDiscovery {
    /// Create a new DNS discovery client
    pub async fn new(config: DnsConfig) -> Result<Self> {
        let resolver = if let Some(resolver_config) = config.resolver_config {
            TokioAsyncResolver::tokio(resolver_config, ResolverOpts::default())
        } else {
            TokioAsyncResolver::tokio(ResolverConfig::default(), ResolverOpts::default())
        };

        Ok(Self {
            resolver,
            default_port: config.default_port,
            watch_interval: config.watch_interval,
        })
    }

    /// Create with default configuration
    pub async fn with_defaults() -> Result<Self> {
        Self::new(DnsConfig::default()).await
    }

    /// Resolve service name to IP addresses
    async fn resolve_name(&self, service_name: &str) -> Result<Vec<IpAddr>> {
        debug!(service = %service_name, "Resolving DNS name");

        let response = self
            .resolver
            .lookup_ip(service_name)
            .await
            .map_err(|e| Error::Discovery(format!("DNS lookup failed: {e}")))?;

        let ips: Vec<IpAddr> = response.iter().collect();
        debug!(service = %service_name, count = ips.len(), "Resolved IPs");

        Ok(ips)
    }

    /// Resolve SRV record
    async fn resolve_srv(&self, service_name: &str) -> Result<Vec<ServiceInstance>> {
        debug!(service = %service_name, "Resolving SRV record");

        let response = self
            .resolver
            .srv_lookup(service_name)
            .await
            .map_err(|e| Error::Discovery(format!("SRV lookup failed: {e}")))?;

        let mut instances = Vec::new();

        for srv in response.iter() {
            let target = srv.target().to_string();
            let port = srv.port();

            // Resolve the target hostname
            match self.resolve_name(&target).await {
                Ok(ips) => {
                    for ip in ips {
                        instances.push(ServiceInstance {
                            id: format!("{service_name}:{ip}:{port}"),
                            name: service_name.to_string(),
                            address: ip.to_string(),
                            port,
                            health: ServiceHealth::Unknown,
                            metadata: ServiceMetadata::default(),
                            endpoints: vec![],
                        });
                    }
                }
                Err(e) => {
                    error!(target = %target, error = %e, "Failed to resolve SRV target");
                }
            }
        }

        debug!(service = %service_name, count = instances.len(), "Resolved SRV instances");
        Ok(instances)
    }
}

#[async_trait]
impl DiscoveryProvider for DnsDiscovery {
    fn name(&self) -> &str {
        "dns"
    }

    async fn discover_services(&self) -> Result<Vec<ServiceInstance>> {
        // DNS doesn't support listing all services
        Err(Error::Discovery(
            "DNS discovery cannot list all services. Use discover_service with specific names"
                .to_string(),
        ))
    }

    async fn discover_service(&self, service_name: &str) -> Result<Vec<ServiceInstance>> {
        info!(service = %service_name, "Discovering service via DNS");

        // Try SRV record first
        if let Ok(instances) = self.resolve_srv(service_name).await {
            if !instances.is_empty() {
                return Ok(instances);
            }
        }

        // Fallback to A/AAAA record lookup
        let ips = self.resolve_name(service_name).await?;

        let instances: Vec<ServiceInstance> = ips
            .into_iter()
            .map(|ip| ServiceInstance {
                id: format!("{}:{}:{}", service_name, ip, self.default_port),
                name: service_name.to_string(),
                address: ip.to_string(),
                port: self.default_port,
                health: ServiceHealth::Unknown,
                metadata: ServiceMetadata::default(),
                endpoints: vec![],
            })
            .collect();

        info!(
            service = %service_name,
            count = instances.len(),
            "Discovered service via DNS"
        );

        Ok(instances)
    }

    async fn watch_services(
        &self,
        _callback: Box<dyn Fn(DiscoveryEvent) + Send + Sync>,
    ) -> Result<()> {
        // DNS watching is not implemented as DNS doesn't provide change notifications
        // This would require periodic polling of specific service names
        info!("DNS watching not implemented - use periodic discover_service calls instead");

        // Keep alive
        let mut interval = time::interval(self.watch_interval);
        loop {
            interval.tick().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dns_config_default() {
        let config = DnsConfig::default();
        assert_eq!(config.default_port, 80);
        assert_eq!(config.watch_interval, Duration::from_secs(60));
    }

    #[tokio::test]
    async fn test_dns_discovery_creation() {
        let discovery = DnsDiscovery::with_defaults().await.unwrap();
        assert_eq!(discovery.name(), "dns");
    }

    #[tokio::test]
    async fn test_resolve_localhost() {
        let discovery = DnsDiscovery::with_defaults().await.unwrap();
        let instances = discovery.discover_service("localhost").await.unwrap();
        assert!(!instances.is_empty());
        assert_eq!(instances[0].name, "localhost");
    }
}
