//! RFC 7662 OAuth2 Token Introspection authentication provider.
//!
//! Accepts an opaque (or JWT) bearer token, POSTs it form-encoded to an external
//! introspection endpoint, and builds a [`Principal`] from the returned identity.
//! This is the standards-based way to plug an external identity service (e.g.
//! authsome's `/v1/introspect`) into the gateway's auth registry.

use crate::registry::{AuthProviderInstance, AuthRequest, AuthResult, Principal};
use async_trait::async_trait;
use octopus_config::types::IntrospectionProviderConfig;
use std::collections::HashMap;
use tracing::warn;

/// RFC 7662 token-introspection provider.
#[derive(Debug)]
pub struct IntrospectionProvider {
    name: String,
    config: IntrospectionProviderConfig,
    client: reqwest::Client,
}

impl IntrospectionProvider {
    /// Create from config.
    pub fn from_config(name: &str, config: &IntrospectionProviderConfig) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder().timeout(config.timeout).build()?;
        Ok(Self {
            name: name.to_string(),
            config: config.clone(),
            client,
        })
    }

    /// Create from config reusing an existing `reqwest::Client`. Used by
    /// [`ConventionAuthProvider`](crate::convention_provider::ConventionAuthProvider)
    /// so its per-namespace providers share one connection pool rather than each
    /// building their own.
    pub fn with_client(
        name: &str,
        config: &IntrospectionProviderConfig,
        client: reqwest::Client,
    ) -> Self {
        Self {
            name: name.to_string(),
            config: config.clone(),
            client,
        }
    }

    /// Extract and de-prefix the token from the configured header.
    fn extract_token(&self, req: &AuthRequest<'_>) -> Option<String> {
        let raw = req
            .headers
            .get(self.config.header_name.as_str())?
            .to_str()
            .ok()?;
        let token = raw
            .strip_prefix(&self.config.token_prefix)
            .unwrap_or(raw)
            .trim();
        if token.is_empty() {
            None
        } else {
            Some(token.to_string())
        }
    }
}

#[async_trait]
impl AuthProviderInstance for IntrospectionProvider {
    async fn authenticate(&self, req: &AuthRequest<'_>) -> anyhow::Result<AuthResult> {
        let Some(token) = self.extract_token(req) else {
            return Ok(AuthResult::Unauthenticated);
        };

        // RFC 7662: POST application/x-www-form-urlencoded with `token` (+hint).
        let mut builder = self.client.post(&self.config.endpoint).form(&[
            ("token", token.as_str()),
            ("token_type_hint", "access_token"),
        ]);
        if let Some(ref client_id) = self.config.client_id {
            builder = builder.basic_auth(client_id, self.config.client_secret.clone());
        }

        let response = match builder.send().await {
            Ok(r) => r,
            Err(e) => {
                warn!(error = %e, "Introspection request failed");
                return Ok(AuthResult::Failed(format!(
                    "Introspection service unreachable: {e}"
                )));
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Ok(AuthResult::Failed(format!(
                "Introspection endpoint returned {status}: {body}"
            )));
        }

        let body: serde_json::Value = match response.json().await {
            Ok(v) => v,
            Err(e) => {
                return Ok(AuthResult::Failed(format!(
                    "Invalid introspection response: {e}"
                )));
            }
        };

        // RFC 7662 §2.2: `active=false` (or absent) means the token is not valid.
        if !body
            .get("active")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
        {
            return Ok(AuthResult::Unauthenticated);
        }

        let obj = body.as_object().cloned().unwrap_or_default();

        let id = string_field(&obj, &self.config.subject_field)
            .or_else(|| string_field(&obj, "sub"))
            .or_else(|| string_field(&obj, "user_id"))
            .or_else(|| string_field(&obj, "username"))
            .unwrap_or_else(|| "unknown".to_string());

        let name = string_field(&obj, "username")
            .or_else(|| string_field(&obj, "name"))
            .or_else(|| string_field(&obj, "email"))
            .unwrap_or_else(|| id.clone());

        // RFC 7662 `scope` is a space-delimited string; also tolerate arrays.
        let scopes = parse_list_field(&obj, &self.config.scope_field, ' ');

        // Roles are non-standard — read from a configured field if present.
        let roles = self
            .config
            .roles_field
            .as_ref()
            .map(|field| parse_list_field(&obj, field, ','))
            .unwrap_or_default();

        // Preserve every remaining claim as an attribute for ABAC / AuthZEN context.
        let mut attributes: HashMap<String, serde_json::Value> = obj.into_iter().collect();
        attributes.remove("active");

        Ok(AuthResult::Authenticated(Principal {
            id,
            name,
            roles,
            scopes,
            provider: self.name.clone(),
            attributes,
        }))
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn provider_type(&self) -> &'static str {
        "introspection"
    }
}

fn string_field(obj: &serde_json::Map<String, serde_json::Value>, key: &str) -> Option<String> {
    obj.get(key)
        .and_then(|v| v.as_str().map(ToString::to_string))
}

/// Parse a field that may be a delimited string or an array of strings.
fn parse_list_field(
    obj: &serde_json::Map<String, serde_json::Value>,
    key: &str,
    delim: char,
) -> Vec<String> {
    match obj.get(key) {
        Some(serde_json::Value::String(s)) => s
            .split(delim)
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect(),
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(ToString::to_string))
            .collect(),
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(json: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
        json.as_object().cloned().unwrap()
    }

    #[test]
    fn parses_space_delimited_scope_and_array_roles() {
        let o = obj(serde_json::json!({
            "scope": "read write admin",
            "roles": ["user", "editor"],
        }));
        assert_eq!(
            parse_list_field(&o, "scope", ' '),
            vec!["read", "write", "admin"]
        );
        assert_eq!(parse_list_field(&o, "roles", ','), vec!["user", "editor"]);
        assert!(parse_list_field(&o, "missing", ',').is_empty());
    }

    #[test]
    fn string_field_reads_present_string_only() {
        let o = obj(serde_json::json!({ "sub": "u-1", "n": 5 }));
        assert_eq!(string_field(&o, "sub").as_deref(), Some("u-1"));
        assert_eq!(string_field(&o, "n"), None);
    }
}
