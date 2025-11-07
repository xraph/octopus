//! JWT Authentication Plugin for Octopus API Gateway

#[cfg(feature = "dashboard")]
pub mod dashboard;

use async_trait::async_trait;
use bytes::Bytes;
use http::{Request, Response};
use http_body_util::Full;
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use octopus_core::{Error, Middleware, Next, Result};
use octopus_plugins::{Plugin, PluginMetadata, PluginType};
use serde::{Deserialize, Serialize};

/// JWT Claims structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// Subject (user ID)
    pub sub: String,
    /// Expiration time
    pub exp: usize,
    /// Issued at
    pub iat: usize,
    /// Optional scopes
    #[serde(default)]
    pub scopes: Vec<String>,
}

/// JWT authentication configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtConfig {
    /// JWT secret key
    pub secret: String,

    /// Algorithm (HS256, HS384, HS512, RS256, etc.)
    #[serde(default = "default_algorithm")]
    pub algorithm: String,

    /// Header name for JWT token
    #[serde(default = "default_header_name")]
    pub header_name: String,

    /// Required scopes (if any)
    #[serde(default)]
    pub required_scopes: Vec<String>,
}

fn default_algorithm() -> String {
    "HS256".to_string()
}

fn default_header_name() -> String {
    "authorization".to_string()
}

impl Default for JwtConfig {
    fn default() -> Self {
        Self {
            secret: String::new(),
            algorithm: default_algorithm(),
            header_name: default_header_name(),
            required_scopes: Vec::new(),
        }
    }
}

/// JWT authentication plugin
pub struct JwtAuthPlugin {
    metadata: PluginMetadata,
    config: JwtConfig,
    validation: Validation,
    decoding_key: DecodingKey,
}

impl JwtAuthPlugin {
    /// Create a new JWT auth plugin
    pub fn new(config: JwtConfig) -> Result<Self> {
        let algorithm = match config.algorithm.as_str() {
            "HS256" => Algorithm::HS256,
            "HS384" => Algorithm::HS384,
            "HS512" => Algorithm::HS512,
            _ => Algorithm::HS256,
        };

        let mut validation = Validation::new(algorithm);
        validation.validate_exp = true;

        let decoding_key = DecodingKey::from_secret(config.secret.as_bytes());

        let mut metadata = PluginMetadata::new("jwt-auth", "1.0.0");
        metadata.author = "Octopus Team".to_string();
        metadata.description = "JWT authentication middleware".to_string();
        metadata.plugin_type = PluginType::Static;

        Ok(Self {
            metadata,
            config,
            validation,
            decoding_key,
        })
    }

    /// Extract token from request
    fn extract_token(&self, req: &Request<Full<Bytes>>) -> Option<String> {
        let header_value = req.headers().get(&self.config.header_name)?;
        let header_str = header_value.to_str().ok()?;

        // Support both "Bearer TOKEN" and just "TOKEN"
        if header_str.starts_with("Bearer ") {
            Some(header_str.trim_start_matches("Bearer ").to_string())
        } else {
            Some(header_str.to_string())
        }
    }

    /// Validate JWT token
    fn validate_token(&self, token: &str) -> Result<Claims> {
        let token_data = decode::<Claims>(token, &self.decoding_key, &self.validation)
            .map_err(|e| Error::Authentication(format!("Invalid JWT: {}", e)))?;

        Ok(token_data.claims)
    }

    /// Check if claims have required scopes
    fn check_scopes(&self, claims: &Claims) -> bool {
        if self.config.required_scopes.is_empty() {
            return true;
        }

        self.config
            .required_scopes
            .iter()
            .all(|required| claims.scopes.contains(required))
    }
}

impl std::fmt::Debug for JwtAuthPlugin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JwtAuthPlugin")
            .field("metadata", &self.metadata)
            .field("algorithm", &self.config.algorithm)
            .finish()
    }
}

#[async_trait]
impl Plugin for JwtAuthPlugin {
    fn metadata(&self) -> &PluginMetadata {
        &self.metadata
    }

    async fn init(&mut self) -> Result<()> {
        tracing::info!("JWT authentication plugin initialized");
        Ok(())
    }
}

#[async_trait]
impl Middleware for JwtAuthPlugin {
    async fn call(&self, req: Request<Full<Bytes>>, next: Next) -> Result<Response<Full<Bytes>>> {
        // Extract token
        let token = self
            .extract_token(&req)
            .ok_or_else(|| Error::Authentication("Missing authentication token".to_string()))?;

        // Validate token
        let claims = self.validate_token(&token)?;

        // Check scopes
        if !self.check_scopes(&claims) {
            return Err(Error::Authorization("Insufficient permissions".to_string()));
        }

        tracing::debug!(
            user = %claims.sub,
            scopes = ?claims.scopes,
            "Request authenticated"
        );

        // Continue with request
        next.run(req).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{encode, EncodingKey, Header};

    fn create_test_token(secret: &str, sub: &str, scopes: Vec<String>) -> String {
        let claims = Claims {
            sub: sub.to_string(),
            exp: (chrono::Utc::now() + chrono::Duration::hours(1)).timestamp() as usize,
            iat: chrono::Utc::now().timestamp() as usize,
            scopes,
        };

        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .unwrap()
    }

    #[test]
    fn test_jwt_plugin_new() {
        let config = JwtConfig {
            secret: "test-secret".to_string(),
            ..Default::default()
        };

        let plugin = JwtAuthPlugin::new(config).unwrap();
        assert_eq!(plugin.metadata().name, "jwt-auth");
    }

    #[test]
    fn test_validate_token() {
        let secret = "test-secret";
        let config = JwtConfig {
            secret: secret.to_string(),
            ..Default::default()
        };

        let plugin = JwtAuthPlugin::new(config).unwrap();
        let token = create_test_token(secret, "user123", vec![]);

        let claims = plugin.validate_token(&token).unwrap();
        assert_eq!(claims.sub, "user123");
    }

    #[test]
    fn test_check_scopes() {
        let config = JwtConfig {
            secret: "test".to_string(),
            required_scopes: vec!["read".to_string(), "write".to_string()],
            ..Default::default()
        };

        let plugin = JwtAuthPlugin::new(config).unwrap();

        let claims_with_scopes = Claims {
            sub: "user".to_string(),
            exp: 0,
            iat: 0,
            scopes: vec!["read".to_string(), "write".to_string(), "admin".to_string()],
        };

        let claims_without_scopes = Claims {
            sub: "user".to_string(),
            exp: 0,
            iat: 0,
            scopes: vec!["read".to_string()],
        };

        assert!(plugin.check_scopes(&claims_with_scopes));
        assert!(!plugin.check_scopes(&claims_without_scopes));
    }
}
