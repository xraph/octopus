//! In-memory and hot-swappable TLS, for Secret/cert-manager-driven certificates.
//!
//! [`build_server_config_from_pem`] builds a rustls config from cert/key bytes
//! (e.g. a Kubernetes `kubernetes.io/tls` Secret), and [`SwappableTlsAcceptor`]
//! lets the running server swap to a new config without a restart when a Secret
//! rotates.

use arc_swap::ArcSwap;
use octopus_core::{Error, Result};
use rustls::server::ServerConfig;
use std::io::Cursor;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_rustls::TlsAcceptor as RustlsAcceptor;

/// Build a rustls [`ServerConfig`] from in-memory PEM bytes (certificate chain +
/// private key), with HTTP/2 + HTTP/1.1 ALPN. No client auth.
pub fn build_server_config_from_pem(cert_pem: &[u8], key_pem: &[u8]) -> Result<ServerConfig> {
    crate::ensure_crypto_provider();

    let certs = rustls_pemfile::certs(&mut Cursor::new(cert_pem))
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| Error::Config(format!("invalid certificate PEM: {e}")))?;
    if certs.is_empty() {
        return Err(Error::Config("no certificates found in PEM".into()));
    }

    let key = rustls_pemfile::private_key(&mut Cursor::new(key_pem))
        .map_err(|e| Error::Config(format!("invalid private key PEM: {e}")))?
        .ok_or_else(|| Error::Config("no private key found in PEM".into()))?;

    let mut config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| Error::Config(format!("failed to build TLS config: {e}")))?;
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Ok(config)
}

/// A TLS acceptor whose certificate/config can be swapped at runtime.
///
/// Reads are lock-free; a connection builds its acceptor from the current
/// config (a cheap `Arc` clone). Call [`SwappableTlsAcceptor::swap`] when a
/// Secret rotates.
#[derive(Clone)]
pub struct SwappableTlsAcceptor {
    config: Arc<ArcSwap<ServerConfig>>,
}

impl SwappableTlsAcceptor {
    /// Create from an initial config.
    pub fn new(config: Arc<ServerConfig>) -> Self {
        Self {
            config: Arc::new(ArcSwap::from(config)),
        }
    }

    /// Replace the active config; connections accepted afterward use it.
    pub fn swap(&self, config: Arc<ServerConfig>) {
        self.config.store(config);
    }

    /// The currently active config.
    pub fn current(&self) -> Arc<ServerConfig> {
        self.config.load_full()
    }

    /// Accept a TLS connection using the current config.
    pub async fn accept<IO>(&self, stream: IO) -> Result<tokio_rustls::server::TlsStream<IO>>
    where
        IO: AsyncRead + AsyncWrite + Unpin,
    {
        let acceptor = RustlsAcceptor::from(self.config.load_full());
        acceptor
            .accept(stream)
            .await
            .map_err(|e| Error::Internal(format!("TLS handshake failed: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_CERT: &str = "-----BEGIN CERTIFICATE-----
MIIBqTCCAVCgAwIBAgIUdXZJNtio8+gPkOsw2TsTczF8LiAwCgYIKoZIzj0EAwIw
GDEWMBQGA1UEAwwNb2N0b3B1cy5sb2NhbDAeFw0yNjA1MzAxOTAxMDBaFw0zNjA1
MjcxOTAxMDBaMBgxFjAUBgNVBAMMDW9jdG9wdXMubG9jYWwwWTATBgcqhkjOPQIB
BggqhkjOPQMBBwNCAARg9r23sThOLJ0CVVqTeLLbkQSbl/fAMZJwLhzCrGHJXk0e
xP7K73agVp3RiDz7w/rmMBCmhSCppD+vpl7vMnZ9o3gwdjAdBgNVHQ4EFgQU4Lgf
Lbz635DVurCsZ3dWSqQ2eJAwHwYDVR0jBBgwFoAU4LgfLbz635DVurCsZ3dWSqQ2
eJAwDwYDVR0TAQH/BAUwAwEB/zAjBgNVHREEHDAagg1vY3RvcHVzLmxvY2Fsggls
b2NhbGhvc3QwCgYIKoZIzj0EAwIDRwAwRAIgZo1rDiv07r7Sc8bMkOb/WVCmL6m8
AbWTroKXTQjea7oCIFC3gsegwlyDazwLWcXPoq/9orb8RokhQlRjTtmCzW6P
-----END CERTIFICATE-----
";

    const TEST_KEY: &str = "-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgdWBkKWLdsDaJ1ERt
VsIFX7+uAgAU2d0mbk+Hls1GCeKhRANCAARg9r23sThOLJ0CVVqTeLLbkQSbl/fA
MZJwLhzCrGHJXk0exP7K73agVp3RiDz7w/rmMBCmhSCppD+vpl7vMnZ9
-----END PRIVATE KEY-----
";

    #[test]
    fn builds_config_from_valid_pem() {
        let config =
            build_server_config_from_pem(TEST_CERT.as_bytes(), TEST_KEY.as_bytes()).unwrap();
        assert!(
            config.alpn_protocols.contains(&b"h2".to_vec()),
            "negotiates HTTP/2"
        );
    }

    #[test]
    fn rejects_garbage_pem() {
        assert!(build_server_config_from_pem(b"not a cert", b"not a key").is_err());
    }

    #[test]
    fn rejects_cert_without_key() {
        assert!(build_server_config_from_pem(TEST_CERT.as_bytes(), b"").is_err());
    }

    #[test]
    fn swap_replaces_active_config() {
        let a = Arc::new(
            build_server_config_from_pem(TEST_CERT.as_bytes(), TEST_KEY.as_bytes()).unwrap(),
        );
        let b = Arc::new(
            build_server_config_from_pem(TEST_CERT.as_bytes(), TEST_KEY.as_bytes()).unwrap(),
        );

        let acceptor = SwappableTlsAcceptor::new(Arc::clone(&a));
        assert!(Arc::ptr_eq(&acceptor.current(), &a), "starts on config a");

        acceptor.swap(Arc::clone(&b));
        assert!(
            Arc::ptr_eq(&acceptor.current(), &b),
            "swap activates config b"
        );
    }
}
