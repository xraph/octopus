//! FARP client for fetching service manifests and schemas

use crate::manifest::SchemaManifest;
use bytes::Bytes;
use http::Uri;
use http_body_util::Full;
use hyper::Request;
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use octopus_core::{Error, Result};
use std::time::Duration;

/// FARP client configuration
#[derive(Debug, Clone)]
pub struct FarpClientConfig {
    /// Request timeout
    pub timeout: Duration,
    /// Number of retry attempts
    pub retry_attempts: u32,
    /// Retry backoff duration
    pub retry_backoff: Duration,
}

impl Default for FarpClientConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            retry_attempts: 3,
            retry_backoff: Duration::from_secs(1),
        }
    }
}

/// FARP client for fetching service manifests
#[derive(Debug, Clone)]
pub struct FarpClient {
    client: Client<HttpConnector, Full<Bytes>>,
    config: FarpClientConfig,
}

impl Default for FarpClient {
    fn default() -> Self {
        Self::new(FarpClientConfig::default())
    }
}

impl FarpClient {
    /// Create a new FARP client
    pub fn new(config: FarpClientConfig) -> Self {
        let client = Client::builder(TokioExecutor::new()).build_http();
        Self { client, config }
    }

    /// Fetch a service manifest from a URL
    pub async fn fetch_manifest(&self, url: &str) -> Result<SchemaManifest> {
        let response_body = self.fetch_with_retry(url).await?;
        let manifest: SchemaManifest = serde_json::from_str(&response_body)
            .map_err(|e| Error::Farp(format!("Failed to parse manifest: {}", e)))?;

        // Verify checksum if present
        if !manifest.verify_checksum()? {
            return Err(Error::Farp("Manifest checksum verification failed".to_string()));
        }

        Ok(manifest)
    }

    /// Fetch a schema from a URL
    pub async fn fetch_schema(&self, url: &str) -> Result<String> {
        self.fetch_with_retry(url).await
    }

    /// Fetch with retry logic
    async fn fetch_with_retry(&self, url: &str) -> Result<String> {
        let mut attempts = 0;
        let mut last_error = None;

        while attempts < self.config.retry_attempts {
            match self.fetch_once(url).await {
                Ok(body) => return Ok(body),
                Err(e) => {
                    last_error = Some(e);
                    attempts += 1;
                    if attempts < self.config.retry_attempts {
                        tokio::time::sleep(self.config.retry_backoff * attempts).await;
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| Error::Farp("Failed to fetch".to_string())))
    }

    /// Fetch once without retry
    async fn fetch_once(&self, url: &str) -> Result<String> {
        let uri: Uri = url
            .parse()
            .map_err(|e| Error::Farp(format!("Invalid URL: {}", e)))?;

        let req = Request::builder()
            .uri(uri)
            .body(Full::new(Bytes::new()))
            .map_err(|e| Error::Farp(format!("Failed to build request: {}", e)))?;

        let response = tokio::time::timeout(self.config.timeout, self.client.request(req))
            .await
            .map_err(|_| Error::UpstreamTimeout)?
            .map_err(|e| Error::Farp(format!("Request failed: {}", e)))?;

        let status = response.status();
        if !status.is_success() {
            return Err(Error::Farp(format!("HTTP error: {}", status)));
        }

        // Read body
        let body_bytes = http_body_util::BodyExt::collect(response.into_body())
            .await
            .map_err(|e| Error::Farp(format!("Failed to read body: {}", e)))?
            .to_bytes();

        String::from_utf8(body_bytes.to_vec())
            .map_err(|e| Error::Farp(format!("Invalid UTF-8: {}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::ServiceInfo;
    use std::collections::HashMap;
    use wiremock::{matchers::{method, path}, Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn test_fetch_manifest() {
        let mock_server = MockServer::start().await;

        let service_info = ServiceInfo {
            name: "test-service".to_string(),
            version: "1.0.0".to_string(),
            description: "Test".to_string(),
            base_url: "http://localhost:8080".to_string(),
            metadata: HashMap::new(),
        };

        let mut manifest = SchemaManifest::new(service_info);
        manifest.calculate_checksum().unwrap();
        let manifest_json = serde_json::to_string(&manifest).unwrap();

        Mock::given(method("GET"))
            .and(path("/manifest.json"))
            .respond_with(ResponseTemplate::new(200).set_body_string(manifest_json))
            .mount(&mock_server)
            .await;

        let client = FarpClient::default();
        let url = format!("{}/manifest.json", mock_server.uri());
        let fetched = client.fetch_manifest(&url).await.unwrap();

        assert_eq!(fetched.service.name, "test-service");
        assert_eq!(fetched.service.version, "1.0.0");
    }

    #[tokio::test]
    async fn test_fetch_schema() {
        let mock_server = MockServer::start().await;

        let schema_content = r#"{"openapi": "3.0.0", "info": {"title": "Test API"}}"#;

        Mock::given(method("GET"))
            .and(path("/schema.json"))
            .respond_with(ResponseTemplate::new(200).set_body_string(schema_content))
            .mount(&mock_server)
            .await;

        let client = FarpClient::default();
        let url = format!("{}/schema.json", mock_server.uri());
        let fetched = client.fetch_schema(&url).await.unwrap();

        assert!(fetched.contains("openapi"));
        assert!(fetched.contains("Test API"));
    }

    #[tokio::test]
    async fn test_fetch_404_error() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/notfound"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&mock_server)
            .await;

        let client = FarpClient::default();
        let url = format!("{}/notfound", mock_server.uri());
        let result = client.fetch_manifest(&url).await;

        assert!(result.is_err());
    }
}


