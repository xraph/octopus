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
