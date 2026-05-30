//! Schema fetcher - fetches schemas from various location strategies
//!
//! Supports HTTP, Registry, and Inline location strategies as per FARP v1.0.0

use crate::manifest::{calculate_schema_checksum, SchemaManifest};
use crate::types::{LocationType, SchemaDescriptor};
use octopus_core::{Error, Result};
use serde_json::Value;
use std::time::Duration;
use tracing::{debug, info, warn};

#[cfg(any(feature = "consul-backend", feature = "etcd-backend"))]
use std::sync::Arc;

#[cfg(feature = "consul-backend")]
use consul::Client as ConsulClient;

#[cfg(feature = "etcd-backend")]
use etcd_client::Client as EtcdClient;

/// Registry backend configuration
#[derive(Clone)]
pub enum RegistryBackend {
    /// No registry backend configured
    None,

    #[cfg(feature = "consul-backend")]
    /// Consul KV store
    Consul(Arc<ConsulClient>),

    #[cfg(feature = "etcd-backend")]
    /// etcd key-value store
    Etcd(Arc<EtcdClient>),
}

impl std::fmt::Debug for RegistryBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => f.write_str("RegistryBackend::None"),
            #[cfg(feature = "consul-backend")]
            Self::Consul(_) => f.write_str("RegistryBackend::Consul"),
            #[cfg(feature = "etcd-backend")]
            Self::Etcd(_) => f.write_str("RegistryBackend::Etcd"),
        }
    }
}

/// Schema fetcher handles retrieving schemas from various locations
#[derive(Debug)]
pub struct SchemaFetcher {
    http_client: reqwest::Client,
    registry_backend: RegistryBackend,
}

impl SchemaFetcher {
    /// Create a new schema fetcher with default settings
    #[must_use]
    pub fn new() -> Self {
        Self::with_timeout(Duration::from_secs(30))
    }

    /// Create a new schema fetcher with custom timeout
    #[must_use]
    pub fn with_timeout(timeout: Duration) -> Self {
        let http_client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .expect("Failed to create HTTP client");

        Self {
            http_client,
            registry_backend: RegistryBackend::None,
        }
    }

    /// Set the registry backend
    #[must_use]
    pub fn with_registry_backend(mut self, backend: RegistryBackend) -> Self {
        self.registry_backend = backend;
        self
    }

    #[cfg(feature = "consul-backend")]
    /// Create a fetcher with Consul backend
    pub async fn with_consul(timeout: Duration, _consul_addr: &str) -> Result<Self> {
        // TODO: Implement Consul client once consul crate API is stabilized
        warn!("Consul backend not fully implemented yet");
        Ok(Self::with_timeout(timeout))
    }

    #[cfg(feature = "etcd-backend")]
    /// Create a fetcher with etcd backend
    pub async fn with_etcd(timeout: Duration, _etcd_endpoints: Vec<String>) -> Result<Self> {
        // TODO: Implement etcd client once etcd-client crate API is stabilized
        warn!("etcd backend not fully implemented yet");
        Ok(Self::with_timeout(timeout))
    }

    /// Fetch a schema based on its descriptor
    pub async fn fetch_schema(&self, descriptor: &SchemaDescriptor) -> Result<Value> {
        match descriptor.location.location_type {
            LocationType::HTTP => self.fetch_http(descriptor).await,
            LocationType::Registry => self.fetch_registry(descriptor).await,
            LocationType::Inline => self.fetch_inline(descriptor),
        }
    }

    /// Fetch all schemas from a manifest
    pub async fn fetch_manifest_schemas(
        &self,
        manifest: &SchemaManifest,
    ) -> Result<Vec<(SchemaDescriptor, Value)>> {
        let mut results = Vec::new();

        for descriptor in &manifest.schemas {
            match self.fetch_schema(descriptor).await {
                Ok(schema) => {
                    // Verify checksum if present
                    if !descriptor.hash.is_empty() {
                        let calculated_hash = calculate_schema_checksum(&schema)?;
                        if calculated_hash != descriptor.hash {
                            warn!(
                                schema_type = ?descriptor.schema_type,
                                expected = %descriptor.hash,
                                actual = %calculated_hash,
                                "Schema checksum mismatch"
                            );
                            return Err(Error::Farp(format!(
                                "Schema checksum mismatch for {:?}: expected {}, got {}",
                                descriptor.schema_type, descriptor.hash, calculated_hash
                            )));
                        }
                    }

                    results.push((descriptor.clone(), schema));
                }
                Err(e) => {
                    warn!(
                        schema_type = ?descriptor.schema_type,
                        error = %e,
                        "Failed to fetch schema, skipping"
                    );
                    // Continue with other schemas even if one fails
                }
            }
        }

        Ok(results)
    }

    /// Fetch schema via HTTP
    async fn fetch_http(&self, descriptor: &SchemaDescriptor) -> Result<Value> {
        let url = descriptor
            .location
            .url
            .as_ref()
            .ok_or_else(|| Error::Farp("HTTP URL is required for HTTP location".to_string()))?;

        debug!(
            url = %url,
            schema_type = ?descriptor.schema_type,
            "Fetching schema via HTTP"
        );

        let mut request = self.http_client.get(url);

        // Add custom headers if specified
        if let Some(headers) = &descriptor.location.headers {
            for (key, value) in headers {
                request = request.header(key, value);
            }
        }

        let response = request
            .send()
            .await
            .map_err(|e| Error::Farp(format!("Failed to fetch schema from {url}: {e}")))?;

        if !response.status().is_success() {
            return Err(Error::Farp(format!(
                "HTTP request failed with status {}: {}",
                response.status(),
                url
            )));
        }

        let body = response
            .text()
            .await
            .map_err(|e| Error::Farp(format!("Failed to read response body: {e}")))?;

        let schema: Value = serde_json::from_str(&body)
            .map_err(|e| Error::Farp(format!("Failed to parse schema JSON: {e}")))?;

        info!(
            url = %url,
            schema_type = ?descriptor.schema_type,
            size_bytes = body.len(),
            "Successfully fetched schema via HTTP"
        );

        Ok(schema)
    }

    /// Fetch schema from registry
    async fn fetch_registry(&self, descriptor: &SchemaDescriptor) -> Result<Value> {
        let _registry_path = descriptor.location.registry_path.as_ref().ok_or_else(|| {
            Error::Farp("Registry path is required for registry location".to_string())
        })?;

        match &self.registry_backend {
            RegistryBackend::None => {
                Err(Error::Farp(
                    "No registry backend configured. Use with_consul() or with_etcd() to configure a backend.".to_string()
                ))
            }

            #[cfg(feature = "consul-backend")]
            RegistryBackend::Consul(client) => {
                self.fetch_from_consul(client, _registry_path, descriptor).await
            }

            #[cfg(feature = "etcd-backend")]
            RegistryBackend::Etcd(client) => {
                self.fetch_from_etcd(client, _registry_path, descriptor).await
            }
        }
    }

    #[cfg(feature = "consul-backend")]
    /// Fetch schema from Consul KV store
    async fn fetch_from_consul(
        &self,
        client: &ConsulClient,
        path: &str,
        descriptor: &SchemaDescriptor,
    ) -> Result<Value> {
        info!(
            path = %path,
            schema_type = ?descriptor.schema_type,
            "Fetching schema from Consul KV"
        );

        // Use the consul crate's KV read API (synchronous)
        let (kv_pair, _meta) = consul::kv::KV::get(client, path, None)
            .map_err(|e| Error::Farp(format!("Consul KV read failed for '{path}': {e}")))?;

        let kv = kv_pair
            .ok_or_else(|| Error::Farp(format!("Schema not found in Consul KV at path: {path}")))?;

        // The consul crate returns KV values already decoded into a String
        let value_str = kv.Value;

        let schema: Value = serde_json::from_str(&value_str)
            .map_err(|e| Error::Farp(format!("Invalid JSON in Consul KV at '{path}': {e}")))?;

        info!(path = %path, "Successfully fetched schema from Consul KV");
        Ok(schema)
    }

    #[cfg(feature = "etcd-backend")]
    /// Fetch schema from etcd
    async fn fetch_from_etcd(
        &self,
        client: &EtcdClient,
        path: &str,
        descriptor: &SchemaDescriptor,
    ) -> Result<Value> {
        info!(
            path = %path,
            schema_type = ?descriptor.schema_type,
            "Fetching schema from etcd"
        );

        // etcd_client::Client::get requires &mut self, so we clone the client
        // (etcd_client::Client is cheap to clone as it wraps an Arc internally)
        let mut client = client.clone();
        let response = client
            .get(path, None)
            .await
            .map_err(|e| Error::Farp(format!("etcd get failed for '{path}': {e}")))?;

        let kv = response
            .kvs()
            .first()
            .ok_or_else(|| Error::Farp(format!("Schema not found in etcd at key: {path}")))?;

        let value_str = kv
            .value_str()
            .map_err(|e| Error::Farp(format!("Invalid UTF-8 in etcd value: {e}")))?;

        let schema: Value = serde_json::from_str(value_str)
            .map_err(|e| Error::Farp(format!("Invalid JSON in etcd at '{path}': {e}")))?;

        info!(path = %path, "Successfully fetched schema from etcd");
        Ok(schema)
    }

    /// Get inline schema (already embedded in descriptor)
    fn fetch_inline(&self, descriptor: &SchemaDescriptor) -> Result<Value> {
        debug!(
            schema_type = ?descriptor.schema_type,
            "Using inline schema"
        );

        let schema = descriptor.inline_schema.as_ref().ok_or_else(|| {
            Error::Farp("Inline schema is required for inline location".to_string())
        })?;

        info!(
            schema_type = ?descriptor.schema_type,
            "Successfully retrieved inline schema"
        );

        Ok(schema.clone())
    }
}

impl Default for SchemaFetcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{SchemaLocation, SchemaType};
    use serde_json::json;

    #[test]
    fn test_fetcher_creation() {
        let fetcher = SchemaFetcher::new();
        assert!(matches!(fetcher.registry_backend, RegistryBackend::None));
    }

    #[test]
    fn test_fetch_inline() {
        let fetcher = SchemaFetcher::new();

        let inline_schema = json!({
            "openapi": "3.1.0",
            "info": {
                "title": "Test API",
                "version": "1.0.0"
            }
        });

        let descriptor = SchemaDescriptor {
            schema_type: SchemaType::OpenAPI,
            spec_version: "3.1.0".to_string(),
            location: SchemaLocation::inline(),
            content_type: "application/json".to_string(),
            inline_schema: Some(inline_schema.clone()),
            hash: String::new(),
            size: 0,
        };

        let result = fetcher.fetch_inline(&descriptor).unwrap();
        assert_eq!(result, inline_schema);
    }

    #[test]
    fn test_fetch_inline_missing_schema() {
        let fetcher = SchemaFetcher::new();

        let descriptor = SchemaDescriptor {
            schema_type: SchemaType::OpenAPI,
            spec_version: "3.1.0".to_string(),
            location: SchemaLocation::inline(),
            content_type: "application/json".to_string(),
            inline_schema: None,
            hash: String::new(),
            size: 0,
        };

        let result = fetcher.fetch_inline(&descriptor);
        assert!(result.is_err());
    }
}
