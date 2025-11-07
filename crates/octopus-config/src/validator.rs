//! Configuration validation

use crate::Config;
use octopus_core::{Error, Result};

/// Validate configuration
pub fn validate_config(config: &Config) -> Result<()> {
    // Validate gateway
    validate_gateway(config)?;

    // Validate upstreams
    validate_upstreams(config)?;

    // Validate routes
    validate_routes(config)?;

    // Validate plugins
    validate_plugins(config)?;

    Ok(())
}

fn validate_gateway(config: &Config) -> Result<()> {
    // Check request timeout is reasonable
    if config.gateway.request_timeout.as_secs() == 0 {
        return Err(Error::Config("request_timeout must be > 0".to_string()));
    }

    if config.gateway.request_timeout.as_secs() > 300 {
        tracing::warn!("request_timeout is very high (>5 minutes)");
    }

    // Check max body size
    if config.gateway.max_body_size == 0 {
        return Err(Error::Config("max_body_size must be > 0".to_string()));
    }

    // Validate TLS configuration if present
    if let Some(ref tls) = config.gateway.tls {
        if tls.cert_file.is_empty() {
            return Err(Error::Config("TLS cert_file cannot be empty".to_string()));
        }
        if tls.key_file.is_empty() {
            return Err(Error::Config("TLS key_file cannot be empty".to_string()));
        }

        // Validate TLS version
        match tls.min_tls_version.as_str() {
            "1.2" | "1.3" => {}
            _ => {
                return Err(Error::Config(format!(
                    "Invalid TLS version: {} (must be 1.2 or 1.3)",
                    tls.min_tls_version
                )));
            }
        }

        // Validate reload interval
        if tls.enable_cert_reload && tls.reload_interval_secs == 0 {
            return Err(Error::Config(
                "TLS reload_interval_secs must be > 0 when cert reload is enabled".to_string(),
            ));
        }
    }

    Ok(())
}

fn validate_upstreams(config: &Config) -> Result<()> {
    for upstream in &config.upstreams {
        if upstream.name.is_empty() {
            return Err(Error::Config("upstream name cannot be empty".to_string()));
        }

        if upstream.instances.is_empty() {
            tracing::warn!(
                upstream = %upstream.name,
                "Upstream has no instances"
            );
        }

        // Validate instances
        for instance in &upstream.instances {
            if instance.id.is_empty() {
                return Err(Error::Config("instance id cannot be empty".to_string()));
            }

            if instance.host.is_empty() {
                return Err(Error::Config("instance host cannot be empty".to_string()));
            }

            if instance.port == 0 {
                return Err(Error::Config("instance port must be > 0".to_string()));
            }
        }
    }

    Ok(())
}

fn validate_routes(config: &Config) -> Result<()> {
    for route in &config.routes {
        if route.path.is_empty() {
            return Err(Error::Config("route path cannot be empty".to_string()));
        }

        if !route.path.starts_with('/') {
            return Err(Error::Config("route path must start with '/'".to_string()));
        }

        if route.upstream.is_empty() {
            return Err(Error::Config("route upstream cannot be empty".to_string()));
        }

        // Check that upstream exists
        if !config.upstreams.iter().any(|u| u.name == route.upstream) {
            return Err(Error::Config(format!(
                "Route references non-existent upstream: {}",
                route.upstream
            )));
        }
    }

    Ok(())
}

fn validate_plugins(_config: &Config) -> Result<()> {
    // TODO: Validate plugin configurations
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;
    use std::time::Duration;

    fn minimal_config() -> Config {
        Config {
            gateway: GatewayConfig {
                listen: "127.0.0.1:8080".parse().unwrap(),
                workers: 0,
                request_timeout: Duration::from_secs(30),
                shutdown_timeout: Duration::from_secs(30),
                max_body_size: 1024 * 1024,
                tls: None,
                compression: CompressionConfig::default(),
                internal_route_prefix: Some("__".to_string()),
            },
            upstreams: vec![],
            routes: vec![],
            plugins: vec![],
            observability: ObservabilityConfig::default(),
        }
    }

    #[test]
    fn test_valid_minimal_config() {
        let config = minimal_config();
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn test_zero_timeout() {
        let mut config = minimal_config();
        config.gateway.request_timeout = Duration::from_secs(0);

        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn test_zero_body_size() {
        let mut config = minimal_config();
        config.gateway.max_body_size = 0;

        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn test_route_invalid_upstream() {
        let mut config = minimal_config();
        config.routes.push(RouteConfig {
            path: "/test".to_string(),
            methods: vec!["GET".to_string()],
            upstream: "nonexistent".to_string(),
            priority: 0,
            strip_prefix: None,
            add_prefix: None,
            metadata: std::collections::HashMap::new(),
        });

        assert!(validate_config(&config).is_err());
    }
}
