//! Certificate and key loading utilities

use octopus_core::{Error, Result};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls_pemfile::{certs, private_key};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;
use tracing::{info, warn};

/// Load certificates from a PEM file
pub fn load_certificates(path: &Path) -> Result<Vec<CertificateDer<'static>>> {
    let file = File::open(path).map_err(|e| {
        Error::Config(format!(
            "Failed to open certificate file {}: {}",
            path.display(),
            e
        ))
    })?;

    let mut reader = BufReader::new(file);
    let certs = certs(&mut reader)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| Error::Config(format!("Failed to parse certificates: {e}")))?;

    if certs.is_empty() {
        return Err(Error::Config(format!(
            "No certificates found in {}",
            path.display()
        )));
    }

    info!(
        path = %path.display(),
        count = certs.len(),
        "Loaded TLS certificates"
    );

    Ok(certs)
}

/// Load private key from a PEM file
pub fn load_private_key(path: &Path) -> Result<PrivateKeyDer<'static>> {
    let file = File::open(path).map_err(|e| {
        Error::Config(format!(
            "Failed to open private key file {}: {}",
            path.display(),
            e
        ))
    })?;

    let mut reader = BufReader::new(file);
    let key = private_key(&mut reader)
        .map_err(|e| Error::Config(format!("Failed to parse private key: {e}")))?
        .ok_or_else(|| Error::Config(format!("No private key found in {}", path.display())))?;

    info!(path = %path.display(), "Loaded TLS private key");

    Ok(key)
}

/// Certificate metadata for reload tracking
#[derive(Debug)]
struct CertificateMetadata {
    cert_path: Box<Path>,
    key_path: Box<Path>,
    last_modified: SystemTime,
}

/// Certificate reloader for zero-downtime certificate updates
pub struct CertificateReloader {
    metadata: Arc<RwLock<CertificateMetadata>>,
    certificates: Arc<RwLock<Vec<CertificateDer<'static>>>>,
    private_key: Arc<RwLock<PrivateKeyDer<'static>>>,
    reload_interval: Duration,
}

impl CertificateReloader {
    /// Create a new certificate reloader
    pub fn new(cert_path: &Path, key_path: &Path, reload_interval: Duration) -> Result<Self> {
        // Load initial certificates
        let certificates = load_certificates(cert_path)?;
        let private_key = load_private_key(key_path)?;

        // Get initial modification time
        let cert_metadata = std::fs::metadata(cert_path)
            .map_err(|e| Error::Config(format!("Failed to read cert metadata: {e}")))?;
        let last_modified = cert_metadata
            .modified()
            .map_err(|e| Error::Config(format!("Failed to get cert modification time: {e}")))?;

        let metadata = CertificateMetadata {
            cert_path: cert_path.into(),
            key_path: key_path.into(),
            last_modified,
        };

        Ok(Self {
            metadata: Arc::new(RwLock::new(metadata)),
            certificates: Arc::new(RwLock::new(certificates)),
            private_key: Arc::new(RwLock::new(private_key)),
            reload_interval,
        })
    }

    /// Get current certificates
    pub async fn certificates(&self) -> Vec<CertificateDer<'static>> {
        self.certificates.read().await.clone()
    }

    /// Get current private key
    pub async fn private_key(&self) -> PrivateKeyDer<'static> {
        self.private_key.read().await.clone_key()
    }

    /// Check and reload certificates if modified
    pub async fn check_and_reload(&self) -> Result<bool> {
        let metadata = self.metadata.read().await;

        // Check if certificate file was modified
        let cert_metadata = std::fs::metadata(&metadata.cert_path)
            .map_err(|e| Error::Internal(format!("Failed to read cert metadata: {e}")))?;

        let current_modified = cert_metadata
            .modified()
            .map_err(|e| Error::Internal(format!("Failed to get cert modification time: {e}")))?;

        if current_modified <= metadata.last_modified {
            return Ok(false);
        }

        drop(metadata);

        // Certificate was modified, reload
        info!("Certificate file modified, reloading...");

        match self.reload_certificates().await {
            Ok(()) => {
                info!("Successfully reloaded certificates");
                Ok(true)
            }
            Err(e) => {
                warn!(error = %e, "Failed to reload certificates, keeping current");
                Ok(false)
            }
        }
    }

    /// Reload certificates from disk
    async fn reload_certificates(&self) -> Result<()> {
        let metadata = self.metadata.read().await;

        // Load new certificates
        let new_certs = load_certificates(&metadata.cert_path)?;
        let new_key = load_private_key(&metadata.key_path)?;

        // Update stored certificates
        *self.certificates.write().await = new_certs;
        *self.private_key.write().await = new_key;

        // Update modification time
        let cert_metadata = std::fs::metadata(&metadata.cert_path)
            .map_err(|e| Error::Internal(format!("Failed to read cert metadata: {e}")))?;
        let new_modified = cert_metadata
            .modified()
            .map_err(|e| Error::Internal(format!("Failed to get cert modification time: {e}")))?;

        drop(metadata);
        self.metadata.write().await.last_modified = new_modified;

        Ok(())
    }

    /// Start automatic certificate reloading
    pub fn start_auto_reload(self: Arc<Self>) {
        let reloader = Arc::clone(&self);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(reloader.reload_interval);
            loop {
                interval.tick().await;
                if let Err(e) = reloader.check_and_reload().await {
                    warn!(error = %e, "Error checking for certificate reload");
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_nonexistent_file() {
        let result = load_certificates(Path::new("/nonexistent/cert.pem"));
        assert!(result.is_err());
    }
}
