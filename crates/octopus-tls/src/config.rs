//! TLS configuration

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// TLS configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TlsConfig {
    /// Path to certificate file (PEM format)
    pub cert_file: PathBuf,

    /// Path to private key file (PEM format)
    pub key_file: PathBuf,

    /// Optional client CA certificate for mTLS
    #[serde(default)]
    pub client_ca_file: Option<PathBuf>,

    /// Require client certificates (mutual TLS)
    #[serde(default)]
    pub require_client_cert: bool,

    /// Minimum TLS version (1.2 or 1.3)
    #[serde(default = "default_min_tls_version")]
    pub min_tls_version: String,

    /// Enable certificate reloading
    #[serde(default = "default_cert_reload")]
    pub enable_cert_reload: bool,

    /// Certificate reload check interval in seconds
    #[serde(default = "default_reload_interval")]
    pub reload_interval_secs: u64,
}

fn default_min_tls_version() -> String {
    "1.2".to_string()
}

fn default_cert_reload() -> bool {
    true
}

fn default_reload_interval() -> u64 {
    300 // 5 minutes
}

impl TlsConfig {
    /// Validate the configuration
    pub fn validate(&self) -> octopus_core::Result<()> {
        // Check certificate file exists
        if !self.cert_file.exists() {
            return Err(octopus_core::Error::Config(format!(
                "Certificate file not found: {}",
                self.cert_file.display()
            )));
        }

        // Check key file exists
        if !self.key_file.exists() {
            return Err(octopus_core::Error::Config(format!(
                "Private key file not found: {}",
                self.key_file.display()
            )));
        }

        // Check client CA if specified
        if let Some(ref ca_file) = self.client_ca_file {
            if !ca_file.exists() {
                return Err(octopus_core::Error::Config(format!(
                    "Client CA file not found: {}",
                    ca_file.display()
                )));
            }
        }

        // Validate TLS version
        match self.min_tls_version.as_str() {
            "1.2" | "1.3" => Ok(()),
            _ => Err(octopus_core::Error::Config(format!(
                "Invalid TLS version: {} (must be 1.2 or 1.3)",
                self.min_tls_version
            ))),
        }
    }

    /// Get the minimum TLS protocol version
    pub fn get_min_protocol_version(&self) -> rustls::ProtocolVersion {
        match self.min_tls_version.as_str() {
            "1.3" => rustls::ProtocolVersion::TLSv1_3,
            _ => rustls::ProtocolVersion::TLSv1_2,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_values() {
        assert_eq!(default_min_tls_version(), "1.2");
        assert!(default_cert_reload());
        assert_eq!(default_reload_interval(), 300);
    }

    #[test]
    fn test_tls_version_parsing() {
        let config = TlsConfig {
            cert_file: PathBuf::from("/tmp/cert.pem"),
            key_file: PathBuf::from("/tmp/key.pem"),
            client_ca_file: None,
            require_client_cert: false,
            min_tls_version: "1.2".to_string(),
            enable_cert_reload: true,
            reload_interval_secs: 300,
        };

        assert_eq!(
            config.get_min_protocol_version(),
            rustls::ProtocolVersion::TLSv1_2
        );
    }
}
