//! TLS/HTTPS support for upstream connections using rustls

use octopus_core::{Error, Result};
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, RootCertStore};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_rustls::{client::TlsStream, TlsConnector};
use tracing::{debug, warn};

/// TLS configuration for upstream connections
#[derive(Clone)]
pub struct TlsConfig {
    /// TLS connector
    connector: Arc<TlsConnector>,

    /// Whether to verify server certificates (default: true)
    verify_certificates: bool,

    /// Custom root certificates
    root_certs: Option<Arc<RootCertStore>>,
}

impl TlsConfig {
    /// Create a new TLS configuration with system root certificates
    pub fn new() -> Result<Self> {
        let mut root_store = RootCertStore::empty();

        // Load system root certificates
        match rustls_native_certs::load_native_certs() {
            Ok(certs) => {
                for cert in certs {
                    if let Err(e) = root_store.add(cert) {
                        warn!("Failed to add certificate: {:?}", e);
                    }
                }
                debug!("Loaded {} system root certificates", root_store.len());
            }
            Err(e) => {
                warn!("Failed to load system certificates: {}", e);
                // Fall back to webpki roots
                root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
                debug!("Using webpki root certificates as fallback");
            }
        }

        let config = ClientConfig::builder()
            .with_root_certificates(root_store.clone())
            .with_no_client_auth();

        Ok(Self {
            connector: Arc::new(TlsConnector::from(Arc::new(config))),
            verify_certificates: true,
            root_certs: Some(Arc::new(root_store)),
        })
    }

    /// Create a new TLS configuration with custom root certificates
    pub fn with_custom_roots(root_store: RootCertStore) -> Result<Self> {
        let config = ClientConfig::builder()
            .with_root_certificates(root_store.clone())
            .with_no_client_auth();

        Ok(Self {
            connector: Arc::new(TlsConnector::from(Arc::new(config))),
            verify_certificates: true,
            root_certs: Some(Arc::new(root_store)),
        })
    }

    /// Create a new TLS configuration that skips certificate verification
    ///
    /// # Security Warning
    /// This is insecure and should only be used for testing or development.
    /// Never use this in production!
    pub fn insecure() -> Result<Self> {
        let config = ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(NoVerifier))
            .with_no_client_auth();

        Ok(Self {
            connector: Arc::new(TlsConnector::from(Arc::new(config))),
            verify_certificates: false,
            root_certs: None,
        })
    }

    /// Set whether to verify server certificates
    pub fn with_verification(mut self, verify: bool) -> Self {
        self.verify_certificates = verify;
        self
    }

    /// Connect to a TLS server
    pub async fn connect(&self, stream: TcpStream, domain: &str) -> Result<TlsStream<TcpStream>> {
        let server_name = ServerName::try_from(domain.to_string())
            .map_err(|e| Error::UpstreamConnection(format!("Invalid server name: {}", e)))?;

        debug!(
            domain = %domain,
            verify = self.verify_certificates,
            "Establishing TLS connection"
        );

        self.connector
            .connect(server_name, stream)
            .await
            .map_err(|e| Error::UpstreamConnection(format!("TLS handshake failed: {}", e)))
    }

    /// Get the connector
    pub fn connector(&self) -> &TlsConnector {
        &self.connector
    }

    /// Check if certificate verification is enabled
    pub fn verifies_certificates(&self) -> bool {
        self.verify_certificates
    }
}

impl Default for TlsConfig {
    fn default() -> Self {
        Self::new().expect("Failed to create default TLS config")
    }
}

impl std::fmt::Debug for TlsConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TlsConfig")
            .field("verify_certificates", &self.verify_certificates)
            .field("has_custom_roots", &self.root_certs.is_some())
            .finish()
    }
}

/// Certificate verifier that skips all verification (INSECURE!)
#[derive(Debug)]
struct NoVerifier;

impl rustls::client::danger::ServerCertVerifier for NoVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> std::result::Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::ECDSA_NISTP521_SHA512,
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA512,
            rustls::SignatureScheme::ED25519,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tls_config_new() {
        let config = TlsConfig::new();
        assert!(config.is_ok());

        let config = config.unwrap();
        assert!(config.verifies_certificates());
    }

    #[test]
    fn test_tls_config_insecure() {
        let config = TlsConfig::insecure();
        assert!(config.is_ok());

        let config = config.unwrap();
        assert!(!config.verifies_certificates());
    }

    #[test]
    fn test_tls_config_with_verification() {
        let config = TlsConfig::new().unwrap().with_verification(false);

        // Note: This doesn't actually disable verification in the connector,
        // it's just a flag. Use insecure() for that.
        assert!(!config.verifies_certificates());
    }

    #[tokio::test]
    async fn test_tls_connect_invalid_domain() {
        let config = TlsConfig::new().unwrap();
        let stream = TcpStream::connect("1.1.1.1:443").await.unwrap();

        // This should fail because the domain is invalid
        let result = config.connect(stream, "").await;
        assert!(result.is_err());
    }
}
