//! TLS/SSL support for Octopus API Gateway
//!
//! Provides secure HTTPS connections with support for:
//! - TLS 1.2 and TLS 1.3
//! - Certificate loading from PEM files
//! - Private key loading (RSA, ECDSA, Ed25519)
//! - Certificate validation
//! - Automatic certificate reload
//! - SNI (Server Name Indication) support
//!
//! # Features
//!
//! - Zero-downtime certificate updates
//! - Modern cipher suites
//! - ALPN protocol negotiation (HTTP/1.1, HTTP/2)
//! - Certificate chain validation
//! - Custom CA certificate support

pub mod acceptor;
pub mod config;
pub mod loader;
pub mod mtls;

pub use acceptor::{extract_client_cn, TlsAcceptor, TlsClientCn};
pub use config::TlsConfig;
pub use loader::{load_certificates, load_private_key, CertificateReloader};
pub use mtls::{MtlsConfig, TargetTlsConfig};

/// Ensure a process-wide rustls [`CryptoProvider`] is installed.
///
/// With rustls 0.23 both the `aws-lc-rs` and `ring` backends can be compiled in
/// at once (e.g. under `--all-features`), and then `ClientConfig`/`ServerConfig`
/// builders cannot choose one automatically and panic. Installing a default once
/// removes that ambiguity; the call is idempotent and a no-op if a provider
/// already exists.
///
/// Call this before constructing any rustls client/server (TLS acceptor, the
/// Kubernetes client, etc.).
pub fn ensure_crypto_provider() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    });
}
