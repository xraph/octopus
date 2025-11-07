//! # Octopus Configuration
//!
//! Configuration management with support for:
//! - Multiple formats (YAML, TOML, JSON)
//! - Environment variable overrides
//! - Hot reload (file watching)
//! - Validation
//! - Default values

#![forbid(unsafe_code)]
#![warn(
    missing_docs,
    missing_debug_implementations,
    rust_2018_idioms,
    unreachable_pub
)]

pub mod builder;
pub mod loader;
pub mod merger;
pub mod types;
pub mod validator;

pub use builder::ConfigBuilder;
pub use loader::{load_and_merge, load_config, load_from_file, load_from_str};
pub use merger::merge_configs;
pub use types::{Config, GatewayConfig, PluginConfig, UpstreamConfig};
pub use validator::validate_config;

use octopus_core::{Error, Result};
use std::path::Path;

/// Load configuration from a file
pub fn load<P: AsRef<Path>>(path: P) -> Result<Config> {
    load_from_file(path)
}

/// Load configuration from a string
pub fn load_str(content: &str, format: ConfigFormat) -> Result<Config> {
    load_from_str(content, format)
}

/// Configuration format
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFormat {
    /// YAML format
    Yaml,
    /// TOML format
    Toml,
    /// JSON format
    Json,
}

impl ConfigFormat {
    /// Detect format from file extension
    pub fn from_path(path: &Path) -> Result<Self> {
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .ok_or_else(|| Error::Config("Unable to detect config format".to_string()))?;

        match ext {
            "yaml" | "yml" => Ok(ConfigFormat::Yaml),
            "toml" => Ok(ConfigFormat::Toml),
            "json" => Ok(ConfigFormat::Json),
            _ => Err(Error::Config(format!("Unsupported config format: {}", ext))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_detect_format_from_path() {
        assert_eq!(
            ConfigFormat::from_path(&PathBuf::from("config.yaml")).unwrap(),
            ConfigFormat::Yaml
        );
        assert_eq!(
            ConfigFormat::from_path(&PathBuf::from("config.toml")).unwrap(),
            ConfigFormat::Toml
        );
        assert_eq!(
            ConfigFormat::from_path(&PathBuf::from("config.json")).unwrap(),
            ConfigFormat::Json
        );
    }

    #[test]
    fn test_unsupported_format() {
        let result = ConfigFormat::from_path(&PathBuf::from("config.txt"));
        assert!(result.is_err());
    }
}
