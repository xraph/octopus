//! OIDC authentication provider with auto-discovery and JWKS refresh

use crate::jwt_provider::Claims;
use crate::registry::{AuthProviderInstance, AuthRequest, AuthResult, Principal};
use async_trait::async_trait;
use dashmap::DashMap;
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use octopus_config::types::OidcProviderConfig;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{error, info, warn};

/// OIDC discovery document
#[derive(Debug, Deserialize)]
struct OidcDiscovery {
    issuer: String,
    jwks_uri: String,
    #[serde(default)]
    id_token_signing_alg_values_supported: Vec<String>,
}

/// JWKS response
#[derive(Debug, Deserialize)]
struct JwksResponse {
    keys: Vec<JwkKey>,
}

/// Individual JWK key
#[derive(Debug, Clone, Deserialize)]
struct JwkKey {
    kid: Option<String>,
    kty: String,
    alg: Option<String>,
    #[serde(rename = "use")]
    use_: Option<String>,
    n: Option<String>,
    e: Option<String>,
    x: Option<String>,
    y: Option<String>,
    crv: Option<String>,
}

/// Cached JWKS keys
struct CachedKeys {
    keys: DashMap<String, (DecodingKey, Algorithm)>,
    fetched_at: Instant,
}

impl std::fmt::Debug for CachedKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CachedKeys")
            .field("key_count", &self.keys.len())
            .field("fetched_at", &self.fetched_at)
            .finish()
    }
}

/// OIDC auth provider
#[derive(Debug)]
pub struct OidcProvider {
    name: String,
    config: OidcProviderConfig,
    cached_keys: Arc<RwLock<Option<CachedKeys>>>,
    issuer: Arc<RwLock<Option<String>>>,
    jwks_uri: Arc<RwLock<Option<String>>>,
    client: reqwest::Client,
}

impl OidcProvider {
    /// Create from config and start background refresh
    pub async fn from_config(name: &str, config: &OidcProviderConfig) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()?;

        let provider = Self {
            name: name.to_string(),
            config: config.clone(),
            cached_keys: Arc::new(RwLock::new(None)),
            issuer: Arc::new(RwLock::new(None)),
            jwks_uri: Arc::new(RwLock::new(None)),
            client,
        };

        // Initial discovery
        if let Err(e) = provider.discover().await {
            warn!(name = %name, error = %e, "Initial OIDC discovery failed; will retry on next request");
        }

        // Spawn background refresh task
        let keys = Arc::clone(&provider.cached_keys);
        let issuer = Arc::clone(&provider.issuer);
        let jwks_uri = Arc::clone(&provider.jwks_uri);
        let refresh_interval = config.jwks_refresh_interval;
        let issuer_url = config.issuer_url.clone();
        let client = provider.client.clone();
        let provider_name = name.to_string();

        tokio::spawn(async move {
            loop {
                tokio::time::sleep(refresh_interval).await;
                if let Err(e) =
                    refresh_keys(&client, &issuer_url, &keys, &issuer, &jwks_uri).await
                {
                    warn!(provider = %provider_name, error = %e, "JWKS refresh failed; keeping last known good keys");
                }
            }
        });

        Ok(provider)
    }

    /// Perform OIDC discovery
    async fn discover(&self) -> anyhow::Result<()> {
        let discovery_url = format!(
            "{}/.well-known/openid-configuration",
            self.config.issuer_url.trim_end_matches('/')
        );

        info!(url = %discovery_url, "Fetching OIDC discovery document");

        let discovery: OidcDiscovery = self.client.get(&discovery_url).send().await?.json().await?;

        *self.issuer.write().await = Some(discovery.issuer);
        *self.jwks_uri.write().await = Some(discovery.jwks_uri.clone());

        // Fetch JWKS
        self.fetch_jwks(&discovery.jwks_uri).await?;

        info!(provider = %self.name, "OIDC discovery complete");
        Ok(())
    }

    /// Fetch and cache JWKS keys
    async fn fetch_jwks(&self, jwks_uri: &str) -> anyhow::Result<()> {
        let jwks: JwksResponse = self.client.get(jwks_uri).send().await?.json().await?;
        let keys = DashMap::new();

        for jwk in &jwks.keys {
            if let Some((kid, decoding_key, algorithm)) = parse_jwk(jwk) {
                keys.insert(kid, (decoding_key, algorithm));
            }
        }

        info!(provider = %self.name, key_count = keys.len(), "Cached JWKS keys");

        *self.cached_keys.write().await = Some(CachedKeys {
            keys,
            fetched_at: Instant::now(),
        });

        Ok(())
    }

    fn extract_token<'a>(&self, req: &'a AuthRequest<'_>) -> Option<&'a str> {
        let header_value = req.headers.get(&self.config.header_name)?.to_str().ok()?;
        if header_value.starts_with(&self.config.token_prefix) {
            Some(&header_value[self.config.token_prefix.len()..])
        } else {
            Some(header_value)
        }
    }
}

#[async_trait]
impl AuthProviderInstance for OidcProvider {
    async fn authenticate(&self, req: &AuthRequest<'_>) -> anyhow::Result<AuthResult> {
        let token = match self.extract_token(req) {
            Some(t) => t,
            None => return Ok(AuthResult::Unauthenticated),
        };

        // Get cached keys
        let keys_guard = self.cached_keys.read().await;
        let cached = match keys_guard.as_ref() {
            Some(c) => c,
            None => {
                drop(keys_guard);
                // Try discovery again
                if let Err(e) = self.discover().await {
                    error!(error = %e, "OIDC discovery failed");
                    return Ok(AuthResult::Failed("OIDC provider unavailable".to_string()));
                }
                let keys_guard = self.cached_keys.read().await;
                match keys_guard.as_ref() {
                    Some(c) => {
                        // Validate inline since we hold the guard
                        return validate_with_keys(
                            c,
                            token,
                            &self.name,
                            &self.config,
                            self.issuer.read().await.as_deref(),
                        );
                    }
                    None => {
                        return Ok(AuthResult::Failed("No JWKS keys available".to_string()));
                    }
                }
            }
        };

        let issuer_guard = self.issuer.read().await;
        validate_with_keys(
            cached,
            token,
            &self.name,
            &self.config,
            issuer_guard.as_deref(),
        )
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn provider_type(&self) -> &str {
        "oidc"
    }
}

fn validate_with_keys(
    cached: &CachedKeys,
    token: &str,
    provider_name: &str,
    config: &OidcProviderConfig,
    issuer: Option<&str>,
) -> anyhow::Result<AuthResult> {
    // Decode header to get kid
    let header = jsonwebtoken::decode_header(token)
        .map_err(|e| anyhow::anyhow!("Invalid JWT header: {}", e))?;

    let kid = header.kid.unwrap_or_default();

    // Find key by kid, or try all keys if no kid
    let keys_to_try: Vec<_> = if !kid.is_empty() {
        cached
            .keys
            .get(&kid)
            .map(|e| vec![(e.value().0.clone(), e.value().1)])
            .unwrap_or_default()
    } else {
        cached
            .keys
            .iter()
            .map(|e| (e.value().0.clone(), e.value().1))
            .collect()
    };

    if keys_to_try.is_empty() {
        return Ok(AuthResult::Failed("No matching JWKS key found".to_string()));
    }

    for (decoding_key, algorithm) in &keys_to_try {
        let mut validation = Validation::new(*algorithm);
        if let Some(issuer) = issuer {
            validation.set_issuer(&[issuer]);
        }
        if let Some(ref audience) = config.audience {
            validation.set_audience(&[audience]);
        }

        match decode::<Claims>(token, decoding_key, &validation) {
            Ok(token_data) => {
                let claims = token_data.claims;

                // Check required scopes
                let token_scopes: Vec<String> = claims
                    .scope
                    .map(|s| s.split_whitespace().map(String::from).collect())
                    .unwrap_or_default();

                if !config.required_scopes.is_empty() {
                    let has_all = config
                        .required_scopes
                        .iter()
                        .all(|s| token_scopes.contains(s));
                    if !has_all {
                        return Ok(AuthResult::Failed(format!(
                            "Missing required scopes: {:?}",
                            config.required_scopes
                        )));
                    }
                }

                return Ok(AuthResult::Authenticated(Principal {
                    id: claims.sub,
                    name: claims.name.unwrap_or_default(),
                    roles: claims.roles,
                    scopes: token_scopes,
                    provider: provider_name.to_string(),
                    attributes: HashMap::new(),
                }));
            }
            Err(_) => continue, // Try next key
        }
    }

    Ok(AuthResult::Failed("Token validation failed with all keys".to_string()))
}

fn parse_jwk(jwk: &JwkKey) -> Option<(String, DecodingKey, Algorithm)> {
    let kid = jwk.kid.clone().unwrap_or_else(|| "default".to_string());
    let alg = jwk
        .alg
        .as_deref()
        .unwrap_or("RS256");

    let algorithm = match alg {
        "RS256" => Algorithm::RS256,
        "RS384" => Algorithm::RS384,
        "RS512" => Algorithm::RS512,
        "ES256" => Algorithm::ES256,
        "ES384" => Algorithm::ES384,
        _ => return None,
    };

    let decoding_key = match jwk.kty.as_str() {
        "RSA" => {
            let n = jwk.n.as_ref()?;
            let e = jwk.e.as_ref()?;
            DecodingKey::from_rsa_components(n, e).ok()?
        }
        "EC" => {
            let x = jwk.x.as_ref()?;
            let y = jwk.y.as_ref()?;
            DecodingKey::from_ec_components(x, y).ok()?
        }
        _ => return None,
    };

    Some((kid, decoding_key, algorithm))
}

/// Background refresh function
async fn refresh_keys(
    client: &reqwest::Client,
    issuer_url: &str,
    cached_keys: &Arc<RwLock<Option<CachedKeys>>>,
    issuer: &Arc<RwLock<Option<String>>>,
    jwks_uri: &Arc<RwLock<Option<String>>>,
) -> anyhow::Result<()> {
    let uri = {
        let guard = jwks_uri.read().await;
        match guard.as_ref() {
            Some(u) => u.clone(),
            None => {
                // Re-discover
                let discovery_url = format!(
                    "{}/.well-known/openid-configuration",
                    issuer_url.trim_end_matches('/')
                );
                let discovery: OidcDiscovery =
                    client.get(&discovery_url).send().await?.json().await?;
                *issuer.write().await = Some(discovery.issuer);
                let uri = discovery.jwks_uri;
                *jwks_uri.write().await = Some(uri.clone());
                uri
            }
        }
    };

    let jwks: JwksResponse = client.get(&uri).send().await?.json().await?;
    let keys = DashMap::new();

    for jwk in &jwks.keys {
        if let Some((kid, decoding_key, algorithm)) = parse_jwk(&jwk) {
            keys.insert(kid, (decoding_key, algorithm));
        }
    }

    *cached_keys.write().await = Some(CachedKeys {
        keys,
        fetched_at: Instant::now(),
    });

    Ok(())
}
