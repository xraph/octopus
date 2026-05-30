//! API key authentication provider

use crate::registry::{AuthProviderInstance, AuthRequest, AuthResult, Principal};
use async_trait::async_trait;
use octopus_config::types::ApiKeyProviderConfig;
use std::collections::HashMap;

/// API key auth provider
#[derive(Debug)]
pub struct ApiKeyProvider {
    name: String,
    header_name: String,
    query_param: Option<String>,
    keys: HashMap<String, KeyInfo>,
}

#[derive(Debug, Clone)]
struct KeyInfo {
    name: String,
    scopes: Vec<String>,
    rate_limit: Option<u32>,
}

impl ApiKeyProvider {
    /// Create from config
    pub fn from_config(name: &str, config: &ApiKeyProviderConfig) -> Self {
        let mut keys = HashMap::new();
        for entry in &config.keys {
            keys.insert(
                entry.key.clone(),
                KeyInfo {
                    name: entry.name.clone(),
                    scopes: entry.scopes.clone(),
                    rate_limit: entry.rate_limit,
                },
            );
        }

        Self {
            name: name.to_string(),
            header_name: config.header_name.clone(),
            query_param: config.query_param.clone(),
            keys,
        }
    }

    fn extract_key<'a>(&self, req: &'a AuthRequest<'_>) -> Option<&'a str> {
        // Try header first
        if let Some(value) = req.headers.get(&self.header_name) {
            return value.to_str().ok();
        }
        // Try query param
        if let Some(ref param_name) = self.query_param {
            if let Some(query) = req.uri.query() {
                for pair in query.split('&') {
                    let mut parts = pair.splitn(2, '=');
                    if let (Some(key), Some(value)) = (parts.next(), parts.next()) {
                        if key == param_name {
                            return Some(value);
                        }
                    }
                }
            }
        }
        None
    }
}

#[async_trait]
impl AuthProviderInstance for ApiKeyProvider {
    async fn authenticate(&self, req: &AuthRequest<'_>) -> anyhow::Result<AuthResult> {
        let key = match self.extract_key(req) {
            Some(k) => k,
            None => return Ok(AuthResult::Unauthenticated),
        };

        match self.keys.get(key) {
            Some(info) => {
                let mut attributes = HashMap::new();
                if let Some(rl) = info.rate_limit {
                    attributes.insert("rate_limit".to_string(), serde_json::json!(rl));
                }

                Ok(AuthResult::Authenticated(Principal {
                    id: format!("apikey:{}", info.name),
                    name: info.name.clone(),
                    roles: vec!["api_consumer".to_string()],
                    scopes: info.scopes.clone(),
                    provider: self.name.clone(),
                    attributes,
                }))
            }
            None => Ok(AuthResult::Failed("Invalid API key".to_string())),
        }
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn provider_type(&self) -> &str {
        "api_key"
    }
}
