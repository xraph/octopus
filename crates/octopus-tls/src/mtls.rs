//! Mutual TLS (mTLS) support
//!
//! Provides server-side client certificate verification and per-target
//! TLS configuration for upstream connections.

use crate::loader::{load_certificates, load_private_key};
use octopus_core::{Error, Result};
use rustls::server::danger::ClientCertVerifier;
use rustls::server::WebPkiClientVerifier;
use rustls::RootCertStore;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

/// Server-side mTLS configuration for verifying client certificates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MtlsConfig {
    /// Path to the CA certificate file used to verify client certificates
    pub ca_cert_file: PathBuf,
    /// Whether to require client certificates (true) or make them optional (false)
    pub require_client_cert: bool,
}

impl MtlsConfig {
    /// Load CA certificates into a root cert store
    pub fn load_client_ca_roots(&self) -> Result<RootCertStore> {
        let ca_certs = load_certificates(&self.ca_cert_file)?;
        let mut root_store = RootCertStore::empty();
        for cert in ca_certs {
            root_store.add(cert).map_err(|e| {
                Error::Config(format!("Failed to add CA certificate to root store: {e}"))
            })?;
        }
        if root_store.is_empty() {
            return Err(Error::Config(format!(
                "No valid CA certificates found in {}",
                self.ca_cert_file.display()
            )));
        }
        Ok(root_store)
    }

    /// Build a client certificate verifier for rustls ServerConfig
    pub fn build_client_verifier(&self) -> Result<Arc<dyn ClientCertVerifier>> {
        let root_store = self.load_client_ca_roots()?;
        let builder = WebPkiClientVerifier::builder(Arc::new(root_store));

        let verifier = if self.require_client_cert {
            builder
                .build()
                .map_err(|e| Error::Config(format!("Failed to build client cert verifier: {e}")))?
        } else {
            builder.allow_unauthenticated().build().map_err(|e| {
                Error::Config(format!(
                    "Failed to build optional client cert verifier: {e}"
                ))
            })?
        };

        Ok(verifier)
    }
}

/// Per-upstream target TLS configuration for outbound connections
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetTlsConfig {
    /// Enable TLS for this upstream target
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// CA certificate file for verifying the upstream server's certificate
    /// If not set, the system root CA store is used
    #[serde(default)]
    pub ca_cert_file: Option<PathBuf>,

    /// Client certificate file for mTLS to the upstream
    #[serde(default)]
    pub client_cert_file: Option<PathBuf>,

    /// Client private key file for mTLS to the upstream
    #[serde(default)]
    pub client_key_file: Option<PathBuf>,

    /// Skip server certificate verification (DANGEROUS — development only)
    #[serde(default)]
    pub insecure_skip_verify: bool,

    /// Override the server name for SNI (Server Name Indication)
    #[serde(default)]
    pub server_name: Option<String>,
}

fn default_true() -> bool {
    true
}

impl Default for TargetTlsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            ca_cert_file: None,
            client_cert_file: None,
            client_key_file: None,
            insecure_skip_verify: false,
            server_name: None,
        }
    }
}

impl TargetTlsConfig {
    /// Build a rustls ClientConfig for connecting to this upstream target
    pub fn build_client_config(&self) -> Result<rustls::ClientConfig> {
        // Build root cert store
        let root_store = if let Some(ref ca_path) = self.ca_cert_file {
            let ca_certs = load_certificates(ca_path)?;
            let mut store = RootCertStore::empty();
            for cert in ca_certs {
                store
                    .add(cert)
                    .map_err(|e| Error::Config(format!("Failed to add CA cert: {e}")))?;
            }
            store
        } else {
            // Use system root certificates
            let mut store = RootCertStore::empty();
            store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            store
        };

        let builder = rustls::ClientConfig::builder().with_root_certificates(root_store);

        // Configure client authentication if cert+key provided
        let config = match (&self.client_cert_file, &self.client_key_file) {
            (Some(cert_path), Some(key_path)) => {
                let certs = load_certificates(cert_path)?;
                let key = load_private_key(key_path)?;
                builder
                    .with_client_auth_cert(certs, key)
                    .map_err(|e| Error::Config(format!("Failed to configure client auth: {e}")))?
            }
            (Some(_), None) | (None, Some(_)) => {
                return Err(Error::Config(
                    "Both client_cert_file and client_key_file must be set for mTLS".to_string(),
                ));
            }
            (None, None) => builder.with_no_client_auth(),
        };

        Ok(config)
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<()> {
        if let Some(ref path) = self.ca_cert_file {
            if !path.exists() {
                return Err(Error::Config(format!(
                    "CA cert file not found: {}",
                    path.display()
                )));
            }
        }
        if let Some(ref path) = self.client_cert_file {
            if !path.exists() {
                return Err(Error::Config(format!(
                    "Client cert file not found: {}",
                    path.display()
                )));
            }
        }
        if let Some(ref path) = self.client_key_file {
            if !path.exists() {
                return Err(Error::Config(format!(
                    "Client key file not found: {}",
                    path.display()
                )));
            }
        }
        // Ensure both or neither client cert/key
        match (&self.client_cert_file, &self.client_key_file) {
            (Some(_), None) | (None, Some(_)) => {
                return Err(Error::Config(
                    "Both client_cert_file and client_key_file must be set together".to_string(),
                ));
            }
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mtls_config_nonexistent_ca_file() {
        let config = MtlsConfig {
            ca_cert_file: PathBuf::from("/nonexistent/ca.pem"),
            require_client_cert: true,
        };
        assert!(config.load_client_ca_roots().is_err());
    }

    #[test]
    fn test_target_tls_config_default() {
        let config = TargetTlsConfig::default();
        assert!(config.enabled);
        assert!(config.ca_cert_file.is_none());
        assert!(config.client_cert_file.is_none());
        assert!(config.client_key_file.is_none());
        assert!(!config.insecure_skip_verify);
        assert!(config.server_name.is_none());
    }

    #[test]
    fn test_target_tls_config_validate_mismatched() {
        let config = TargetTlsConfig {
            client_cert_file: Some(PathBuf::from("/tmp/cert.pem")),
            client_key_file: None,
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_target_tls_config_validate_nonexistent_ca() {
        let config = TargetTlsConfig {
            ca_cert_file: Some(PathBuf::from("/nonexistent/ca.pem")),
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_target_tls_config_validate_no_auth_ok() {
        let config = TargetTlsConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_target_tls_build_client_config_no_auth() {
        // Should succeed with system roots and no client auth
        let config = TargetTlsConfig::default();
        let result = config.build_client_config();
        assert!(result.is_ok());
    }

    #[test]
    fn test_target_tls_build_client_config_mismatched_cert_key() {
        let config = TargetTlsConfig {
            client_cert_file: Some(PathBuf::from("/tmp/cert.pem")),
            client_key_file: None,
            ..Default::default()
        };
        assert!(config.build_client_config().is_err());
    }

    #[test]
    fn test_target_tls_build_client_config_nonexistent_cert() {
        let config = TargetTlsConfig {
            client_cert_file: Some(PathBuf::from("/nonexistent/cert.pem")),
            client_key_file: Some(PathBuf::from("/nonexistent/key.pem")),
            ..Default::default()
        };
        assert!(config.build_client_config().is_err());
    }
}
