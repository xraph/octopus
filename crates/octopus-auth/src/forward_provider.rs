//! Forward authentication provider - delegates to external auth service

use crate::registry::{AuthProviderInstance, AuthRequest, AuthResult, Principal};
use async_trait::async_trait;
use octopus_config::types::ForwardAuthProviderConfig;
use std::collections::HashMap;
use tracing::warn;

/// Forward auth provider
#[derive(Debug)]
pub struct ForwardAuthProvider {
    name: String,
    config: ForwardAuthProviderConfig,
    client: reqwest::Client,
}

impl ForwardAuthProvider {
    /// Create from config
    pub fn from_config(name: &str, config: &ForwardAuthProviderConfig) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder().timeout(config.timeout).build()?;

        Ok(Self {
            name: name.to_string(),
            config: config.clone(),
            client,
        })
    }
}

#[async_trait]
impl AuthProviderInstance for ForwardAuthProvider {
    async fn authenticate(&self, req: &AuthRequest<'_>) -> anyhow::Result<AuthResult> {
        // Build subrequest to auth service
        let mut builder = self.client.get(&self.config.endpoint);

        // Forward configured headers
        for header_name in &self.config.forward_headers {
            if let Some(value) = req.headers.get(header_name.as_str()) {
                if let Ok(v) = value.to_str() {
                    builder = builder.header(header_name, v);
                }
            }
        }

        // Add original request info
        builder = builder
            .header("X-Original-URI", req.uri.to_string())
            .header("X-Original-Method", req.method.as_str());

        // Send request
        let response = match builder.send().await {
            Ok(r) => r,
            Err(e) => {
                warn!(error = %e, "Forward auth request failed");
                return Ok(AuthResult::Failed(format!("Auth service unreachable: {e}")));
            }
        };

        let status = response.status();

        if status.is_success() {
            // Extract principal from response headers
            let response_headers = response.headers().clone();
            let subject = response_headers
                .get("X-Auth-Subject")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("unknown")
                .to_string();

            let roles: Vec<String> = response_headers
                .get("X-Auth-Role")
                .and_then(|v| v.to_str().ok())
                .map(|r| r.split(',').map(|s| s.trim().to_string()).collect())
                .unwrap_or_default();

            let scopes: Vec<String> = response_headers
                .get("X-Auth-Scopes")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.split(',').map(|s| s.trim().to_string()).collect())
                .unwrap_or_default();

            Ok(AuthResult::Authenticated(Principal {
                id: subject.clone(),
                name: subject,
                roles,
                scopes,
                provider: self.name.clone(),
                attributes: HashMap::new(),
            }))
        } else if status.as_u16() == 401 {
            Ok(AuthResult::Unauthenticated)
        } else {
            let body = response.text().await.unwrap_or_default();
            Ok(AuthResult::Failed(format!(
                "Auth service returned {status}: {body}"
            )))
        }
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn provider_type(&self) -> &'static str {
        "forward_auth"
    }
}
