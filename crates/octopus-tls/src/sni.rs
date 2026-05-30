//! SNI-based multi-certificate resolver.
//!
//! Lets a single TLS listener serve many hostnames — one per Gateway listener /
//! TLS Secret — by selecting the certificate that matches the SNI server name.

use octopus_core::{Error, Result};
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Arc;

/// Resolves a server certificate by SNI hostname, with an optional default.
#[derive(Debug, Default)]
pub struct SniCertResolver {
    by_host: HashMap<String, Arc<CertifiedKey>>,
    default: Option<Arc<CertifiedKey>>,
}

impl SniCertResolver {
    /// Create an empty resolver.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add (or replace) the certificate served for `hostname`.
    pub fn add(
        &mut self,
        hostname: impl Into<String>,
        cert_pem: &[u8],
        key_pem: &[u8],
    ) -> Result<()> {
        let key = certified_key_from_pem(cert_pem, key_pem)?;
        self.by_host.insert(hostname.into(), Arc::new(key));
        Ok(())
    }

    /// Set the fallback certificate used when no hostname matches.
    pub fn set_default(&mut self, cert_pem: &[u8], key_pem: &[u8]) -> Result<()> {
        let key = certified_key_from_pem(cert_pem, key_pem)?;
        self.default = Some(Arc::new(key));
        Ok(())
    }

    /// Number of host-specific certificates tracked.
    pub fn len(&self) -> usize {
        self.by_host.len()
    }

    /// Whether no host-specific certificates are tracked.
    pub fn is_empty(&self) -> bool {
        self.by_host.is_empty()
    }

    /// Resolve the certificate for an SNI server name (testable core of
    /// [`ResolvesServerCert::resolve`]).
    fn lookup(&self, server_name: Option<&str>) -> Option<Arc<CertifiedKey>> {
        if let Some(name) = server_name {
            if let Some(ck) = self.by_host.get(name) {
                return Some(Arc::clone(ck));
            }
        }
        self.default.clone()
    }
}

impl ResolvesServerCert for SniCertResolver {
    fn resolve(&self, client_hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        self.lookup(client_hello.server_name())
    }
}

/// Build a [`CertifiedKey`] from PEM cert chain + private key bytes.
fn certified_key_from_pem(cert_pem: &[u8], key_pem: &[u8]) -> Result<CertifiedKey> {
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

    let signing_key = rustls::crypto::aws_lc_rs::sign::any_supported_type(&key)
        .map_err(|e| Error::Config(format!("unsupported private key: {e}")))?;

    Ok(CertifiedKey::new(certs, signing_key))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CERT: &str = "-----BEGIN CERTIFICATE-----
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

    const KEY: &str = "-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgdWBkKWLdsDaJ1ERt
VsIFX7+uAgAU2d0mbk+Hls1GCeKhRANCAARg9r23sThOLJ0CVVqTeLLbkQSbl/fA
MZJwLhzCrGHJXk0exP7K73agVp3RiDz7w/rmMBCmhSCppD+vpl7vMnZ9
-----END PRIVATE KEY-----
";

    #[test]
    fn resolves_by_hostname_with_default_fallback() {
        let mut r = SniCertResolver::new();
        r.add("octopus.local", CERT.as_bytes(), KEY.as_bytes())
            .unwrap();
        r.set_default(CERT.as_bytes(), KEY.as_bytes()).unwrap();
        assert_eq!(r.len(), 1);

        let host_cert = r.lookup(Some("octopus.local")).expect("host match");
        let default_cert = r
            .lookup(Some("unknown.example"))
            .expect("falls back to default");
        let none_sni = r.lookup(None).expect("no SNI falls back to default");

        // The host-specific cert is the one we added; the others are the default.
        assert!(
            !Arc::ptr_eq(&host_cert, &default_cert),
            "host cert differs from default"
        );
        assert!(
            Arc::ptr_eq(&default_cert, &none_sni),
            "unknown host and no-SNI both use default"
        );
    }

    #[test]
    fn no_default_and_no_match_resolves_none() {
        let mut r = SniCertResolver::new();
        r.add("a.example", CERT.as_bytes(), KEY.as_bytes()).unwrap();
        assert!(
            r.lookup(Some("b.example")).is_none(),
            "no match, no default"
        );
    }

    #[test]
    fn add_rejects_invalid_pem() {
        let mut r = SniCertResolver::new();
        assert!(r.add("x", b"garbage", b"garbage").is_err());
    }
}
