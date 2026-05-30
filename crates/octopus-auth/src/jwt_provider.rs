//! JWT authentication provider

use crate::registry::{AuthProviderInstance, AuthRequest, AuthResult, Principal};
use async_trait::async_trait;
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use octopus_config::types::JwtProviderConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// JWT claims
#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    /// Subject (user ID)
    pub sub: String,
    /// Expiration time
    pub exp: usize,
    /// Issued at
    #[serde(default)]
    pub iat: Option<usize>,
    /// Issuer
    #[serde(default)]
    pub iss: Option<String>,
    /// Audience
    #[serde(default)]
    pub aud: Option<serde_json::Value>,
    /// Roles (custom claim)
    #[serde(default)]
    pub roles: Vec<String>,
    /// Scopes (custom claim, space-separated string or array)
    #[serde(default)]
    pub scope: Option<String>,
    /// Name (custom claim)
    #[serde(default)]
    pub name: Option<String>,
}

/// JWT auth provider
pub struct JwtProvider {
    name: String,
    decoding_key: DecodingKey,
    validation: Validation,
    header_name: String,
    token_prefix: String,
}

impl std::fmt::Debug for JwtProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JwtProvider")
            .field("name", &self.name)
            .field("header_name", &self.header_name)
            .finish()
    }
}

impl JwtProvider {
    /// Create from config
    pub fn from_config(name: &str, config: &JwtProviderConfig) -> anyhow::Result<Self> {
        let algorithm = parse_algorithm(&config.algorithm)?;

        let decoding_key = if let Some(ref secret) = config.secret {
            DecodingKey::from_secret(secret.as_bytes())
        } else if let Some(ref public_key) = config.public_key {
            match algorithm {
                Algorithm::RS256 | Algorithm::RS384 | Algorithm::RS512 => {
                    DecodingKey::from_rsa_pem(public_key.as_bytes())?
                }
                Algorithm::ES256 | Algorithm::ES384 => {
                    DecodingKey::from_ec_pem(public_key.as_bytes())?
                }
                _ => {
                    return Err(anyhow::anyhow!(
                        "Public key not supported for {:?}",
                        algorithm
                    ))
                }
            }
        } else if let Some(ref key_file) = config.public_key_file {
            let key_data = std::fs::read_to_string(key_file)?;
            match algorithm {
                Algorithm::RS256 | Algorithm::RS384 | Algorithm::RS512 => {
                    DecodingKey::from_rsa_pem(key_data.as_bytes())?
                }
                Algorithm::ES256 | Algorithm::ES384 => {
                    DecodingKey::from_ec_pem(key_data.as_bytes())?
                }
                _ => {
                    return Err(anyhow::anyhow!(
                        "Public key file not supported for {:?}",
                        algorithm
                    ))
                }
            }
        } else {
            return Err(anyhow::anyhow!(
                "JWT provider '{}' requires secret, public_key, or public_key_file",
                name
            ));
        };

        let mut validation = Validation::new(algorithm);
        if let Some(ref issuer) = config.issuer {
            validation.set_issuer(&[issuer]);
        }
        if let Some(ref audience) = config.audience {
            validation.set_audience(&[audience]);
        }

        Ok(Self {
            name: name.to_string(),
            decoding_key,
            validation,
            header_name: config.header_name.clone(),
            token_prefix: config.token_prefix.clone(),
        })
    }

    /// Extract token from request
    fn extract_token<'a>(&self, req: &'a AuthRequest<'_>) -> Option<&'a str> {
        let header_value = req.headers.get(&self.header_name)?.to_str().ok()?;
        if header_value.starts_with(&self.token_prefix) {
            Some(&header_value[self.token_prefix.len()..])
        } else {
            Some(header_value)
        }
    }
}

#[async_trait]
impl AuthProviderInstance for JwtProvider {
    async fn authenticate(&self, req: &AuthRequest<'_>) -> anyhow::Result<AuthResult> {
        let token = match self.extract_token(req) {
            Some(t) => t,
            None => return Ok(AuthResult::Unauthenticated),
        };

        match decode::<Claims>(token, &self.decoding_key, &self.validation) {
            Ok(token_data) => {
                let claims = token_data.claims;
                let scopes = claims
                    .scope
                    .map(|s| s.split_whitespace().map(String::from).collect())
                    .unwrap_or_default();

                Ok(AuthResult::Authenticated(Principal {
                    id: claims.sub,
                    name: claims.name.unwrap_or_default(),
                    roles: claims.roles,
                    scopes,
                    provider: self.name.clone(),
                    attributes: HashMap::new(),
                }))
            }
            Err(e) => Ok(AuthResult::Failed(format!("Invalid JWT: {}", e))),
        }
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn provider_type(&self) -> &str {
        "jwt"
    }
}

fn parse_algorithm(s: &str) -> anyhow::Result<Algorithm> {
    match s.to_uppercase().as_str() {
        "HS256" => Ok(Algorithm::HS256),
        "HS384" => Ok(Algorithm::HS384),
        "HS512" => Ok(Algorithm::HS512),
        "RS256" => Ok(Algorithm::RS256),
        "RS384" => Ok(Algorithm::RS384),
        "RS512" => Ok(Algorithm::RS512),
        "ES256" => Ok(Algorithm::ES256),
        "ES384" => Ok(Algorithm::ES384),
        _ => Err(anyhow::anyhow!("Unsupported JWT algorithm: {}", s)),
    }
}
