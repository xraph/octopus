//! Configuration loading

use crate::{Config, ConfigFormat};
use octopus_core::{Error, Result};
use regex::Regex;
use std::env;
use std::fs;
use std::path::Path;

/// Load configuration from a file
pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Config> {
    let path = path.as_ref();

    let content = fs::read_to_string(path)
        .map_err(|e| Error::Config(format!("Failed to read config file: {e}")))?;

    let format = ConfigFormat::from_path(path)?;

    load_from_str(&content, format)
}

/// Expand environment variables in configuration string
/// Supports syntax: ${VAR} and ${VAR:-default}
fn expand_env_vars(content: &str) -> Result<String> {
    // Regex to match ${VAR} or ${VAR:-default}
    let re = Regex::new(r"\$\{([A-Za-z_][A-Za-z0-9_]*)(:-([^}]*))?\}")
        .map_err(|e| Error::Config(format!("Invalid regex: {e}")))?;

    let mut result = String::new();
    let mut last_match = 0;

    for cap in re.captures_iter(content) {
        let full_match = cap.get(0).unwrap();
        let var_name = cap.get(1).unwrap().as_str();
        let default_value = cap.get(3).map(|m| m.as_str());

        // Get environment variable value
        let value = match env::var(var_name) {
            Ok(val) => val,
            Err(_) => {
                // Variable not set, use default if provided
                match default_value {
                    Some(default) => default.to_string(),
                    None => {
                        return Err(Error::Config(format!(
                            "Environment variable '{var_name}' not set and no default provided"
                        )));
                    }
                }
            }
        };

        // Append text before this match
        result.push_str(&content[last_match..full_match.start()]);
        // Append the replacement value
        result.push_str(&value);
        // Update position
        last_match = full_match.end();
    }

    // Append remaining text after last match
    result.push_str(&content[last_match..]);

    Ok(result)
}

/// Load configuration from a string
pub fn load_from_str(content: &str, format: ConfigFormat) -> Result<Config> {
    // Expand environment variables first
    let expanded_content = expand_env_vars(content)?;

    let config = match format {
        ConfigFormat::Yaml => serde_yaml::from_str(&expanded_content)
            .map_err(|e| Error::Config(format!("Failed to parse YAML: {e}")))?,
        ConfigFormat::Toml => toml::from_str(&expanded_content)
            .map_err(|e| Error::Config(format!("Failed to parse TOML: {e}")))?,
        ConfigFormat::Json => serde_json::from_str(&expanded_content)
            .map_err(|e| Error::Config(format!("Failed to parse JSON: {e}")))?,
    };

    Ok(config)
}

/// Load configuration with optional overrides
pub fn load_config<P: AsRef<Path>>(path: P, _env_overrides: bool) -> Result<Config> {
    let config = load_from_file(path)?;

    // TODO: Apply environment variable overrides
    // This would read env vars like OCTOPUS_GATEWAY_LISTEN and override config values

    crate::validator::validate_config(&config)?;

    Ok(config)
}

/// Load and merge multiple configuration files
///
/// Files are merged in order, with later files overriding earlier ones.
/// This enables layered configuration:
/// - base.yaml (common defaults)
/// - production.yaml (env-specific)
/// - secrets.yaml (sensitive values)
///
/// # Example
///
/// ```
/// use octopus_config::load_and_merge;
///
/// let config = load_and_merge(vec![
///     "config/base.yaml",
///     "config/production.yaml",
///     "config/secrets.yaml",
/// ])?;
/// ```
pub fn load_and_merge<P: AsRef<Path>>(paths: Vec<P>) -> Result<Config> {
    if paths.is_empty() {
        return Err(Error::Config("No configuration files provided".to_string()));
    }

    let mut configs = Vec::new();

    for path in paths {
        let config = load_from_file(path)?;
        configs.push(config);
    }

    let merged = crate::merger::merge_configs(configs)?;
    crate::validator::validate_config(&merged)?;

    Ok(merged)
}

#[cfg(test)]
mod tests {
    use super::*;

    const YAML_CONFIG: &str = r#"
gateway:
  listen: "127.0.0.1:8080"
  workers: 4
  request_timeout: "30s"
  max_body_size: 10485760

upstreams:
  - name: "user-service"
    instances:
      - id: "user-1"
        host: "localhost"
        port: 8081
        weight: 1
    lb_policy: "round_robin"

routes:
  - path: "/users"
    methods: ["GET", "POST"]
    upstream: "user-service"
    priority: 10

plugins: []

observability:
  logging:
    level: "info"
    format: "text"
  metrics:
    enabled: true
    endpoint: "/metrics"
  tracing:
    enabled: false
"#;

    #[test]
    fn test_load_yaml() {
        let config = load_from_str(YAML_CONFIG, ConfigFormat::Yaml).unwrap();

        assert_eq!(config.gateway.workers, 4);
        assert_eq!(config.upstreams.len(), 1);
        assert_eq!(config.routes.len(), 1);
    }

    #[test]
    fn test_invalid_yaml() {
        let invalid = "invalid: [yaml";
        let result = load_from_str(invalid, ConfigFormat::Yaml);
        assert!(result.is_err());
    }

    #[test]
    fn test_env_var_substitution() {
        // Set test environment variables
        env::set_var("TEST_PORT", "9090");
        env::set_var("TEST_HOST", "0.0.0.0");

        let config_with_vars = r#"
gateway:
  listen: "${TEST_HOST}:${TEST_PORT}"
  workers: 4
  request_timeout: "30s"
  shutdown_timeout: "30s"
  max_body_size: 10485760

upstreams: []
routes: []
plugins: []

observability:
  logging:
    level: "info"
    format: "text"
  metrics:
    enabled: true
    endpoint: "/metrics"
  tracing:
    enabled: false
"#;

        let config = load_from_str(config_with_vars, ConfigFormat::Yaml).unwrap();
        assert_eq!(config.gateway.listen.to_string(), "0.0.0.0:9090");

        // Clean up
        env::remove_var("TEST_PORT");
        env::remove_var("TEST_HOST");
    }

    #[test]
    fn test_env_var_with_default() {
        // Don't set the variable, use default
        env::remove_var("UNDEFINED_VAR");

        let config_with_default = r#"
gateway:
  listen: "${UNDEFINED_VAR:-127.0.0.1:8080}"
  workers: 4
  request_timeout: "30s"
  shutdown_timeout: "30s"
  max_body_size: 10485760

upstreams: []
routes: []
plugins: []

observability:
  logging:
    level: "info"
    format: "text"
  metrics:
    enabled: true
    endpoint: "/metrics"
  tracing:
    enabled: false
"#;

        let config = load_from_str(config_with_default, ConfigFormat::Yaml).unwrap();
        assert_eq!(config.gateway.listen.to_string(), "127.0.0.1:8080");
    }

    #[test]
    fn test_env_var_override_default() {
        // Set variable to override default
        env::set_var("OVERRIDE_VAR", "192.168.1.1:3000");

        let config_with_override = r#"
gateway:
  listen: "${OVERRIDE_VAR:-127.0.0.1:8080}"
  workers: 4
  request_timeout: "30s"
  shutdown_timeout: "30s"
  max_body_size: 10485760

upstreams: []
routes: []
plugins: []

observability:
  logging:
    level: "info"
    format: "text"
  metrics:
    enabled: true
    endpoint: "/metrics"
  tracing:
    enabled: false
"#;

        let config = load_from_str(config_with_override, ConfigFormat::Yaml).unwrap();
        assert_eq!(config.gateway.listen.to_string(), "192.168.1.1:3000");

        // Clean up
        env::remove_var("OVERRIDE_VAR");
    }

    #[test]
    fn test_missing_env_var_no_default() {
        env::remove_var("MISSING_VAR");

        let config_no_default = r#"
gateway:
  listen: "${MISSING_VAR}"
  workers: 4
"#;

        let result = load_from_str(config_no_default, ConfigFormat::Yaml);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("MISSING_VAR"));
    }

    #[test]
    fn test_multiple_env_vars() {
        env::set_var("DB_HOST", "localhost");
        env::set_var("DB_PORT", "5432");
        env::set_var("DB_NAME", "octopus");

        let expanded = expand_env_vars("postgres://${DB_HOST}:${DB_PORT}/${DB_NAME}").unwrap();
        assert_eq!(expanded, "postgres://localhost:5432/octopus");

        // Clean up
        env::remove_var("DB_HOST");
        env::remove_var("DB_PORT");
        env::remove_var("DB_NAME");
    }
}
