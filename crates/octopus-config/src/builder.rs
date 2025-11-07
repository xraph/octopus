//! Configuration builder

use crate::types::{Config, GatewayConfig, ObservabilityConfig};
use std::net::SocketAddr;

/// Builder for constructing configuration programmatically
#[derive(Debug, Default)]
pub struct ConfigBuilder {
    gateway: Option<GatewayConfig>,
}

impl ConfigBuilder {
    /// Create a new configuration builder
    pub fn new() -> Self {
        Self::default()
    }

    /// Set gateway configuration
    pub fn gateway(mut self, gateway: GatewayConfig) -> Self {
        self.gateway = Some(gateway);
        self
    }

    /// Set listen address
    pub fn listen(mut self, addr: SocketAddr) -> Self {
        let gateway = self.gateway.get_or_insert_with(|| GatewayConfig {
            listen: addr,
            workers: 0,
            request_timeout: std::time::Duration::from_secs(30),
            shutdown_timeout: std::time::Duration::from_secs(30),
            max_body_size: 10 * 1024 * 1024,
            tls: None,
            compression: crate::types::CompressionConfig::default(),
            internal_route_prefix: Some("__".to_string()),
        });
        gateway.listen = addr;
        self
    }

    /// Build the configuration
    pub fn build(self) -> octopus_core::Result<Config> {
        let gateway = self
            .gateway
            .ok_or_else(|| octopus_core::Error::Config("gateway is required".to_string()))?;

        Ok(Config {
            gateway,
            upstreams: Vec::new(),
            routes: Vec::new(),
            plugins: Vec::new(),
            farp: crate::types::FarpConfig::default(),
            observability: ObservabilityConfig::default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_builder() {
        let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();

        let config = ConfigBuilder::new().listen(addr).build().unwrap();

        assert_eq!(config.gateway.listen, addr);
    }

    #[test]
    fn test_builder_missing_gateway() {
        let result = ConfigBuilder::new().build();
        assert!(result.is_err());
    }
}
