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

#[cfg(feature = "etcd-backend")]
use etcd_client::Client as EtcdClient;

/// Registry backend configuration
#[derive(Clone)]
pub enum RegistryBackend {
    /// No registry backend configured
    None,

    #[cfg(feature = "consul-backend")]
    /// Consul KV store
    Consul(Arc<ConsulKvClient>),

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

/// Async client for the Consul KV HTTP API.
///
/// It reads a key with `GET /v1/kv/<key>?raw`, which hands back the stored
/// value as-is. We talk to the HTTP API directly instead of going through the
/// `consul` crate: that crate's client is blocking and pins reqwest 0.11
/// (hyper 0.14, h2 0.3), which carries open advisories.
#[cfg(feature = "consul-backend")]
#[derive(Debug, Clone)]
pub struct ConsulKvClient {
    http: reqwest::Client,
    address: String,
    token: Option<String>,
}

#[cfg(feature = "consul-backend")]
impl ConsulKvClient {
    /// Create a client for the Consul agent at `address`
    /// (for example `http://127.0.0.1:8500`).
    pub fn new(http: reqwest::Client, address: impl Into<String>) -> Self {
        Self {
            http,
            address: address.into().trim_end_matches('/').to_string(),
            token: None,
        }
    }

    /// Send this ACL token with every request (`X-Consul-Token`).
    #[must_use]
    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    /// Read the raw value stored at `key`. Returns `Ok(None)` when the key
    /// does not exist.
    pub async fn get_raw(&self, key: &str) -> Result<Option<String>> {
        let url = format!("{}/v1/kv/{}?raw", self.address, key.trim_start_matches('/'));

        let mut request = self.http.get(&url);
        if let Some(token) = &self.token {
            request = request.header("X-Consul-Token", token);
        }

        let response = request
            .send()
            .await
            .map_err(|e| Error::Farp(format!("Consul KV read failed for '{key}': {e}")))?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !response.status().is_success() {
            return Err(Error::Farp(format!(
                "Consul KV read for '{key}' failed with status {}",
                response.status()
            )));
        }

        let body = response
            .text()
            .await
            .map_err(|e| Error::Farp(format!("Failed to read Consul KV value for '{key}': {e}")))?;
        Ok(Some(body))
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
    pub async fn with_consul(timeout: Duration, consul_addr: &str) -> Result<Self> {
        let fetcher = Self::with_timeout(timeout);
        let client = ConsulKvClient::new(fetcher.http_client.clone(), consul_addr);
        Ok(fetcher.with_registry_backend(RegistryBackend::Consul(Arc::new(client))))
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
        client: &ConsulKvClient,
        path: &str,
        descriptor: &SchemaDescriptor,
    ) -> Result<Value> {
        info!(
            path = %path,
            schema_type = ?descriptor.schema_type,
            "Fetching schema from Consul KV"
        );

        let value_str = client
            .get_raw(path)
            .await?
            .ok_or_else(|| Error::Farp(format!("Schema not found in Consul KV at path: {path}")))?;

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

    #[cfg(feature = "consul-backend")]
    #[tokio::test]
    async fn test_fetch_from_consul_kv() {
        use wiremock::matchers::{header, method, path, query_param};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let schema = json!({"openapi": "3.1.0", "info": {"title": "KV API"}});

        Mock::given(method("GET"))
            .and(path("/v1/kv/farp/orders/openapi"))
            .and(query_param("raw", ""))
            .and(header("X-Consul-Token", "secret"))
            .respond_with(ResponseTemplate::new(200).set_body_string(schema.to_string()))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/kv/farp/missing"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let fetcher = SchemaFetcher::new();
        let client = ConsulKvClient::new(fetcher.http_client.clone(), format!("{}/", server.uri()))
            .with_token("secret");
        let fetcher = fetcher.with_registry_backend(RegistryBackend::Consul(Arc::new(client)));

        let descriptor = |key: &str| SchemaDescriptor {
            schema_type: SchemaType::OpenAPI,
            spec_version: "3.1.0".to_string(),
            location: SchemaLocation::registry(key),
            content_type: "application/json".to_string(),
            inline_schema: None,
            hash: String::new(),
            size: 0,
        };

        let fetched = fetcher
            .fetch_schema(&descriptor("/farp/orders/openapi"))
            .await
            .unwrap();
        assert_eq!(fetched, schema);

        let missing = fetcher.fetch_schema(&descriptor("farp/missing")).await;
        assert!(missing.unwrap_err().to_string().contains("not found"));
    }
}
