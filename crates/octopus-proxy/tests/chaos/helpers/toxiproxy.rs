//! Toxiproxy client for chaos testing

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const TOXIPROXY_API: &str = "http://localhost:8474";

/// Toxiproxy client
#[derive(Clone)]
pub struct ToxiproxyClient {
    client: reqwest::Client,
    base_url: String,
}

impl ToxiproxyClient {
    /// Create a new Toxiproxy client
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: TOXIPROXY_API.to_string(),
        }
    }

    /// Check if Toxiproxy is available
    pub async fn is_available(&self) -> bool {
        self.client
            .get(&format!("{}/version", self.base_url))
            .send()
            .await
            .is_ok()
    }

    /// Get proxy by name
    pub async fn get_proxy(&self, name: &str) -> Result<Proxy> {
        let url = format!("{}/proxies/{}", self.base_url, name);
        let response = self.client.get(&url).send().await?;
        let proxy = response.json().await?;
        Ok(proxy)
    }

    /// Create a new proxy
    pub async fn create_proxy(&self, config: ProxyConfig) -> Result<Proxy> {
        let url = format!("{}/proxies", self.base_url);
        let response = self.client.post(&url).json(&config).send().await?;
        
        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            let error: ErrorResponse = response.json().await.unwrap_or_default();
            anyhow::bail!("Failed to create proxy: {}", error.title)
        }
    }

    /// Delete a proxy
    pub async fn delete_proxy(&self, name: &str) -> Result<()> {
        let url = format!("{}/proxies/{}", self.base_url, name);
        self.client.delete(&url).send().await?;
        Ok(())
    }

    /// Enable a proxy
    pub async fn enable_proxy(&self, name: &str) -> Result<()> {
        let url = format!("{}/proxies/{}", self.base_url, name);
        let update = ProxyUpdate { enabled: true };
        self.client.post(&url).json(&update).send().await?;
        Ok(())
    }

    /// Disable a proxy (simulates upstream down)
    pub async fn disable_proxy(&self, name: &str) -> Result<()> {
        let url = format!("{}/proxies/{}", self.base_url, name);
        let update = ProxyUpdate { enabled: false };
        self.client.post(&url).json(&update).send().await?;
        Ok(())
    }

    /// Add a toxic to a proxy
    pub async fn add_toxic(&self, proxy_name: &str, toxic: Toxic) -> Result<Toxic> {
        let url = format!("{}/proxies/{}/toxics", self.base_url, proxy_name);
        let response = self.client.post(&url).json(&toxic).send().await?;
        
        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            let error: ErrorResponse = response.json().await.unwrap_or_default();
            anyhow::bail!("Failed to add toxic: {}", error.title)
        }
    }

    /// Remove a toxic from a proxy
    pub async fn remove_toxic(&self, proxy_name: &str, toxic_name: &str) -> Result<()> {
        let url = format!("{}/proxies/{}/toxics/{}", self.base_url, proxy_name, toxic_name);
        self.client.delete(&url).send().await?;
        Ok(())
    }

    /// Remove all toxics from a proxy
    pub async fn remove_all_toxics(&self, proxy_name: &str) -> Result<()> {
        let url = format!("{}/proxies/{}/toxics", self.base_url, proxy_name);
        let response = self.client.get(&url).send().await?;
        let toxics: Vec<Toxic> = response.json().await?;
        
        for toxic in toxics {
            self.remove_toxic(proxy_name, &toxic.name).await?;
        }
        Ok(())
    }

    /// Reset proxy (remove all toxics and enable)
    pub async fn reset_proxy(&self, proxy_name: &str) -> Result<()> {
        self.remove_all_toxics(proxy_name).await?;
        self.enable_proxy(proxy_name).await?;
        Ok(())
    }

    /// List all proxies
    pub async fn list_proxies(&self) -> Result<HashMap<String, Proxy>> {
        let url = format!("{}/proxies", self.base_url);
        let response = self.client.get(&url).send().await?;
        let proxies = response.json().await?;
        Ok(proxies)
    }
}

impl Default for ToxiproxyClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Proxy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    pub name: String,
    pub listen: String,
    pub upstream: String,
    pub enabled: bool,
}

/// Proxy information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proxy {
    pub name: String,
    pub listen: String,
    pub upstream: String,
    pub enabled: bool,
    #[serde(default)]
    pub toxics: Vec<Toxic>,
}

/// Proxy update
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProxyUpdate {
    enabled: bool,
}

/// Toxic (network condition)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Toxic {
    pub name: String,
    #[serde(rename = "type")]
    pub toxic_type: String,
    pub stream: String,
    pub toxicity: f32,
    pub attributes: serde_json::Value,
}

impl Toxic {
    /// Create a latency toxic
    pub fn latency(name: impl Into<String>, latency_ms: u32) -> Self {
        Self {
            name: name.into(),
            toxic_type: "latency".to_string(),
            stream: "downstream".to_string(),
            toxicity: 1.0,
            attributes: serde_json::json!({
                "latency": latency_ms,
                "jitter": 0
            }),
        }
    }

    /// Create a latency toxic with jitter
    pub fn latency_with_jitter(name: impl Into<String>, latency_ms: u32, jitter_ms: u32) -> Self {
        Self {
            name: name.into(),
            toxic_type: "latency".to_string(),
            stream: "downstream".to_string(),
            toxicity: 1.0,
            attributes: serde_json::json!({
                "latency": latency_ms,
                "jitter": jitter_ms
            }),
        }
    }

    /// Create a bandwidth limit toxic
    pub fn bandwidth(name: impl Into<String>, rate_kbps: u32) -> Self {
        Self {
            name: name.into(),
            toxic_type: "bandwidth".to_string(),
            stream: "downstream".to_string(),
            toxicity: 1.0,
            attributes: serde_json::json!({
                "rate": rate_kbps
            }),
        }
    }

    /// Create a timeout toxic
    pub fn timeout(name: impl Into<String>, timeout_ms: u32) -> Self {
        Self {
            name: name.into(),
            toxic_type: "timeout".to_string(),
            stream: "downstream".to_string(),
            toxicity: 1.0,
            attributes: serde_json::json!({
                "timeout": timeout_ms
            }),
        }
    }

    /// Create a slow close toxic
    pub fn slow_close(name: impl Into<String>, delay_ms: u32) -> Self {
        Self {
            name: name.into(),
            toxic_type: "slow_close".to_string(),
            stream: "downstream".to_string(),
            toxicity: 1.0,
            attributes: serde_json::json!({
                "delay": delay_ms
            }),
        }
    }

    /// Create a slicer toxic (slices data into smaller packets)
    pub fn slicer(name: impl Into<String>, average_size: u32, delay_us: u32) -> Self {
        Self {
            name: name.into(),
            toxic_type: "slicer".to_string(),
            stream: "downstream".to_string(),
            toxicity: 1.0,
            attributes: serde_json::json!({
                "average_size": average_size,
                "size_variation": average_size / 2,
                "delay": delay_us
            }),
        }
    }

    /// Create a limit data toxic
    pub fn limit_data(name: impl Into<String>, bytes: u64) -> Self {
        Self {
            name: name.into(),
            toxic_type: "limit_data".to_string(),
            stream: "downstream".to_string(),
            toxicity: 1.0,
            attributes: serde_json::json!({
                "bytes": bytes
            }),
        }
    }
}

/// Error response from Toxiproxy
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ErrorResponse {
    #[serde(default)]
    title: String,
    #[serde(default)]
    status: u16,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires Toxiproxy to be running
    async fn test_toxiproxy_connection() {
        let client = ToxiproxyClient::new();
        assert!(client.is_available().await, "Toxiproxy should be available");
    }

    #[tokio::test]
    #[ignore] // Requires Toxiproxy to be running
    async fn test_list_proxies() {
        let client = ToxiproxyClient::new();
        let proxies = client.list_proxies().await.unwrap();
        assert!(!proxies.is_empty(), "Should have at least one proxy");
    }
}
