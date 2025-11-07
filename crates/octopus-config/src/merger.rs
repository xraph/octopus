//! Configuration file merging
//!
//! Supports loading and merging multiple configuration files.
//! Later files override earlier files, allowing for layered configuration:
//! - base.yaml (defaults)
//! - environment.yaml (env-specific)
//! - secrets.yaml (sensitive values)
//! - local.yaml (developer overrides)

use crate::types::{Config, GatewayConfig, PluginConfig, RouteConfig, UpstreamConfig};
use octopus_core::Result;
use std::collections::HashMap;

/// Merge multiple configurations together
///
/// Later configs override earlier configs. Arrays are concatenated.
///
/// # Example
///
/// ```rust
/// use octopus_config::merger::merge_configs;
///
/// let base = Config { /* ... */ };
/// let override_config = Config { /* ... */ };
///
/// let merged = merge_configs(vec![base, override_config])?;
/// ```
pub fn merge_configs(configs: Vec<Config>) -> Result<Config> {
    if configs.is_empty() {
        return Err(octopus_core::Error::Config(
            "No configurations to merge".to_string(),
        ));
    }

    let mut result = configs[0].clone();

    for config in configs.iter().skip(1) {
        result = merge_two_configs(result, config.clone())?;
    }

    Ok(result)
}

/// Merge two configurations
fn merge_two_configs(mut base: Config, overlay: Config) -> Result<Config> {
    // Merge gateway config
    base.gateway = merge_gateway_config(base.gateway, overlay.gateway);

    // Merge upstreams (by name, or append if new)
    base.upstreams = merge_upstreams(base.upstreams, overlay.upstreams);

    // Merge routes (by path, or append if new)
    base.routes = merge_routes(base.routes, overlay.routes);

    // Merge plugins (by name, or append if new)
    base.plugins = merge_plugins(base.plugins, overlay.plugins);

    // Merge observability
    base.observability = overlay.observability;

    Ok(base)
}

/// Merge gateway configurations
fn merge_gateway_config(base: GatewayConfig, overlay: GatewayConfig) -> GatewayConfig {
    // Only override non-default values from overlay
    // This is a simple strategy - overlay wins
    GatewayConfig {
        listen: overlay.listen,
        workers: if overlay.workers > 0 {
            overlay.workers
        } else {
            base.workers
        },
        request_timeout: overlay.request_timeout,
        shutdown_timeout: overlay.shutdown_timeout,
        max_body_size: overlay.max_body_size,
        tls: overlay.tls.or(base.tls),
        compression: overlay.compression,
        internal_route_prefix: overlay.internal_route_prefix.or(base.internal_route_prefix),
    }
}

/// Merge upstreams by name
fn merge_upstreams(
    mut base: Vec<UpstreamConfig>,
    overlay: Vec<UpstreamConfig>,
) -> Vec<UpstreamConfig> {
    let mut upstream_map: HashMap<String, UpstreamConfig> =
        base.drain(..).map(|u| (u.name.clone(), u)).collect();

    // Override or add upstreams from overlay
    for upstream in overlay {
        upstream_map.insert(upstream.name.clone(), upstream);
    }

    upstream_map.into_values().collect()
}

/// Merge routes by path
fn merge_routes(mut base: Vec<RouteConfig>, overlay: Vec<RouteConfig>) -> Vec<RouteConfig> {
    let mut route_map: HashMap<String, RouteConfig> =
        base.drain(..).map(|r| (r.path.clone(), r)).collect();

    // Override or add routes from overlay
    for route in overlay {
        route_map.insert(route.path.clone(), route);
    }

    route_map.into_values().collect()
}

/// Merge plugins by name
fn merge_plugins(mut base: Vec<PluginConfig>, overlay: Vec<PluginConfig>) -> Vec<PluginConfig> {
    let mut plugin_map: HashMap<String, PluginConfig> =
        base.drain(..).map(|p| (p.name.clone(), p)).collect();

    // Override or add plugins from overlay
    for plugin in overlay {
        plugin_map.insert(plugin.name.clone(), plugin);
    }

    plugin_map.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CompressionConfig, ObservabilityConfig};
    use std::net::SocketAddr;
    use std::time::Duration;

    fn create_test_config(listen_port: u16, workers: usize) -> Config {
        Config {
            gateway: GatewayConfig {
                listen: format!("127.0.0.1:{}", listen_port)
                    .parse::<SocketAddr>()
                    .unwrap(),
                workers,
                request_timeout: Duration::from_secs(30),
                shutdown_timeout: Duration::from_secs(10),
                max_body_size: 10 * 1024 * 1024,
                tls: None,
                compression: CompressionConfig::default(),
                internal_route_prefix: None,
            },
            upstreams: vec![],
            routes: vec![],
            plugins: vec![],
            observability: ObservabilityConfig::default(),
        }
    }

    #[test]
    fn test_merge_two_configs() {
        let base = create_test_config(8080, 4);
        let overlay = create_test_config(9090, 8);

        let merged = merge_two_configs(base, overlay).unwrap();

        // Overlay should win
        assert_eq!(merged.gateway.listen.port(), 9090);
        assert_eq!(merged.gateway.workers, 8);
    }

    #[test]
    fn test_merge_upstreams() {
        let upstream1 = UpstreamConfig {
            name: "service1".to_string(),
            instances: vec![],
            lb_policy: "round_robin".to_string(),
            health_check: None,
            circuit_breaker: None,
        };

        let upstream2 = UpstreamConfig {
            name: "service2".to_string(),
            instances: vec![],
            lb_policy: "round_robin".to_string(),
            health_check: None,
            circuit_breaker: None,
        };

        let upstream1_override = UpstreamConfig {
            name: "service1".to_string(),
            instances: vec![], // Different config
            lb_policy: "least_conn".to_string(),
            health_check: None,
            circuit_breaker: None,
        };

        let base = vec![upstream1];
        let overlay = vec![upstream1_override, upstream2];

        let merged = merge_upstreams(base, overlay);

        assert_eq!(merged.len(), 2);

        // Find service1 and verify it was overridden
        let service1 = merged.iter().find(|u| u.name == "service1").unwrap();
        assert_eq!(service1.lb_policy, "least_conn");
    }

    #[test]
    fn test_merge_multiple_configs() {
        let config1 = create_test_config(8080, 4);
        let config2 = create_test_config(9090, 8);
        let config3 = create_test_config(9091, 16);

        let merged = merge_configs(vec![config1, config2, config3]).unwrap();

        // Last config should win
        assert_eq!(merged.gateway.listen.port(), 9091);
        assert_eq!(merged.gateway.workers, 16);
    }

    #[test]
    fn test_merge_empty_configs() {
        let result = merge_configs(vec![]);
        assert!(result.is_err());
    }
}
