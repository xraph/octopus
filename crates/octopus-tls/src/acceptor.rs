//! TLS acceptor implementation

use crate::config::TlsConfig;
use crate::loader::{load_certificates, load_private_key, CertificateReloader};
use octopus_core::{Error, Result};
use rustls::server::ServerConfig;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_rustls::TlsAcceptor as RustlsAcceptor;
use tracing::info;
use x509_parser::prelude::*;

/// TLS connection acceptor
#[derive(Clone)]
pub struct TlsAcceptor {
    inner: RustlsAcceptor,
    _reloader: Option<Arc<CertificateReloader>>,
}

/// Build a rustls [`ServerConfig`] from [`TlsConfig`]: loads the cert/key files,
/// configures optional mTLS client verification, and sets HTTP/2 + HTTP/1.1 ALPN.
///
/// Shared by [`TlsAcceptor::new`] and the file-based hot-reload path, so a
/// reloaded config preserves mTLS and ALPN.
pub fn build_server_config(config: &TlsConfig) -> Result<ServerConfig> {
    crate::ensure_crypto_provider();
    config.validate()?;

    let certs = load_certificates(&config.cert_file)?;
    let private_key = load_private_key(&config.key_file)?;

    let mut server_config = if let Some(ref ca_file) = config.client_ca_file {
        let mtls_cfg = crate::mtls::MtlsConfig {
            ca_cert_file: ca_file.clone(),
            require_client_cert: config.require_client_cert,
        };
        let verifier = mtls_cfg.build_client_verifier()?;
        info!(
            ca_file = %ca_file.display(),
            require = config.require_client_cert,
            "mTLS client authentication enabled"
        );
        ServerConfig::builder()
            .with_client_cert_verifier(verifier)
            .with_single_cert(certs, private_key)
            .map_err(|e| Error::Config(format!("Failed to build TLS config with mTLS: {e}")))?
    } else {
        ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, private_key)
            .map_err(|e| Error::Config(format!("Failed to build TLS config: {e}")))?
    };

    server_config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Ok(server_config)
}

impl TlsAcceptor {
    /// Create a new TLS acceptor from configuration
    pub fn new(config: &TlsConfig) -> Result<Self> {
        let server_config = build_server_config(config)?;
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

    /// Create an acceptor from a prebuilt rustls [`ServerConfig`] (e.g. one
    /// built in memory from a Kubernetes TLS Secret). No file-based reloader.
    pub fn from_server_config(config: Arc<ServerConfig>) -> Self {
        Self {
            inner: RustlsAcceptor::from(config),
            _reloader: None,
        }
    }

    /// Accept a TLS connection
    pub async fn accept<IO>(&self, stream: IO) -> Result<tokio_rustls::server::TlsStream<IO>>
    where
        IO: AsyncRead + AsyncWrite + Unpin,
    {
        self.inner
            .accept(stream)
            .await
            .map_err(|e| Error::Internal(format!("TLS handshake failed: {e}")))
    }
}

/// TLS client Common Name (CN) extracted from peer certificate
/// Stored in request extensions to make it available to auth middleware
#[derive(Debug, Clone)]
pub struct TlsClientCn(pub Option<String>);

/// Extract the Common Name (CN) from the peer's client certificate
pub fn extract_client_cn<IO>(tls_stream: &tokio_rustls::server::TlsStream<IO>) -> Option<String>
where
    IO: AsyncRead + AsyncWrite + Unpin,
{
    let (_, server_conn) = tls_stream.get_ref();
    let certs = server_conn.peer_certificates()?;
    let cert_der = certs.first()?;
    extract_cn_from_der(cert_der.as_ref())
}

/// The TLS SNI server name negotiated for this connection.
///
/// Stored in request extensions so the gateway can route on it and reject
/// `Host`/`:authority` values that disagree with the negotiated SNI (anti
/// host-spoofing for multi-tenant routing). Lowercased by rustls.
#[derive(Debug, Clone)]
pub struct TlsSniName(pub Option<String>);

/// Extract the negotiated SNI server name from a completed TLS handshake.
pub fn extract_server_name<IO>(tls_stream: &tokio_rustls::server::TlsStream<IO>) -> Option<String>
where
    IO: AsyncRead + AsyncWrite + Unpin,
{
    let (_, server_conn) = tls_stream.get_ref();
    server_conn.server_name().map(String::from)
}

/// Parse a DER-encoded X.509 certificate and extract the subject CN
fn extract_cn_from_der(der: &[u8]) -> Option<String> {
    let (_, cert) = X509Certificate::from_der(der).ok()?;
    let result = cert
        .subject()
        .iter_common_name()
        .next()
        .and_then(|cn| cn.as_str().ok())
        .map(String::from);
    result
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
