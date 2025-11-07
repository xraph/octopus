//! TLS acceptor implementation

use crate::config::TlsConfig;
use crate::loader::{load_certificates, load_private_key, CertificateReloader};
use octopus_core::{Error, Result};
use rustls::server::ServerConfig;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_rustls::TlsAcceptor as RustlsAcceptor;
use tracing::info;

/// TLS connection acceptor
#[derive(Clone)]
pub struct TlsAcceptor {
    inner: RustlsAcceptor,
    _reloader: Option<Arc<CertificateReloader>>,
}

impl TlsAcceptor {
    /// Create a new TLS acceptor from configuration
    pub fn new(config: &TlsConfig) -> Result<Self> {
        // Validate configuration
        config.validate()?;

        // Load certificates and private key
        let certs = load_certificates(&config.cert_file)?;
        let private_key = load_private_key(&config.key_file)?;

        // Build TLS server configuration
        let mut server_config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, private_key)
            .map_err(|e| Error::Config(format!("Failed to build TLS config: {}", e)))?;

        // Configure ALPN protocols (HTTP/1.1 and HTTP/2)
        server_config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

        let inner = RustlsAcceptor::from(Arc::new(server_config));

        // Set up certificate reloading if enabled
        let reloader = if config.enable_cert_reload {
            let reloader = Arc::new(CertificateReloader::new(
                &config.cert_file,
                &config.key_file,
                std::time::Duration::from_secs(config.reload_interval_secs),
            )?);

            Arc::clone(&reloader).start_auto_reload();
            info!(
                interval_secs = config.reload_interval_secs,
                "Certificate auto-reload enabled"
            );
            Some(reloader)
        } else {
            None
        };

        info!(
            cert_file = %config.cert_file.display(),
            min_tls = %config.min_tls_version,
            "TLS acceptor initialized"
        );

        Ok(Self {
            inner,
            _reloader: reloader,
        })
    }

    /// Accept a TLS connection
    pub async fn accept<IO>(&self, stream: IO) -> Result<tokio_rustls::server::TlsStream<IO>>
    where
        IO: AsyncRead + AsyncWrite + Unpin,
    {
        self.inner
            .accept(stream)
            .await
            .map_err(|e| Error::Internal(format!("TLS handshake failed: {}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_acceptor_with_invalid_config() {
        let config = TlsConfig {
            cert_file: PathBuf::from("/nonexistent/cert.pem"),
            key_file: PathBuf::from("/nonexistent/key.pem"),
            client_ca_file: None,
            require_client_cert: false,
            min_tls_version: "1.2".to_string(),
            enable_cert_reload: false,
            reload_interval_secs: 300,
        };

        let result = TlsAcceptor::new(&config);
        assert!(result.is_err());
    }
}
