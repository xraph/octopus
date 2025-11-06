//! JWT Authentication Middleware
//!
//! Validates JSON Web Tokens (JWT) for authentication and authorization.
//! Supports RS256, HS256, and other standard JWT algorithms.

use async_trait::async_trait;
use bytes::Bytes;
use http::{header, Request, Response, StatusCode};
use http_body_util::Full;
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use octopus_core::{Middleware, Next, Result as CoreResult};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;

/// Body type alias
pub type Body = Full<Bytes>;

/// JWT Configuration
#[derive(Clone)]
pub struct JwtConfig {
    /// Secret key for HMAC algorithms (HS256, HS384, HS512)
    pub secret: Option<String>,
    
    /// Public key for RSA/ECDSA algorithms (RS256, ES256, etc.)
    pub public_key: Option<String>,
    
    /// Algorithm to use for validation
    pub algorithm: Algorithm,
    
    /// Header name to extract JWT from (default: "Authorization")
    pub header_name: String,
    
    /// Token prefix (default: "Bearer ")
    pub token_prefix: String,
    
    /// Required audience ("aud" claim)
    pub audience: Option<String>,
    
    /// Required issuer ("iss" claim)
    pub issuer: Option<String>,
    
    /// Skip paths that don't require authentication
    pub skip_paths: Vec<String>,
}

impl Default for JwtConfig {
    fn default() -> Self {
        Self {
            secret: None,
            public_key: None,
            algorithm: Algorithm::HS256,
            header_name: "Authorization".to_string(),
            token_prefix: "Bearer ".to_string(),
            audience: None,
            issuer: None,
            skip_paths: vec![],
        }
    }
}

impl fmt::Debug for JwtConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JwtConfig")
            .field("algorithm", &self.algorithm)
            .field("header_name", &self.header_name)
            .field("token_prefix", &self.token_prefix)
            .field("audience", &self.audience)
            .field("issuer", &self.issuer)
            .field("has_secret", &self.secret.is_some())
            .field("has_public_key", &self.public_key.is_some())
            .field("skip_paths", &self.skip_paths)
            .finish()
    }
}

/// Standard JWT claims
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    /// Subject (user ID)
    pub sub: String,
    
    /// Expiration time (Unix timestamp)
    pub exp: usize,
    
    /// Issued at (Unix timestamp)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iat: Option<usize>,
    
    /// Issuer
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iss: Option<String>,
    
    /// Audience
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aud: Option<String>,
    
    /// Custom claims (extensible)
    #[serde(flatten)]
    pub custom: serde_json::Value,
}

/// JWT Authentication Middleware
#[derive(Clone)]
pub struct JwtAuth {
    config: Arc<JwtConfig>,
    validation: Validation,
    decoding_key: Arc<DecodingKey>,
}

impl JwtAuth {
    /// Create a new JWT authentication middleware with HS256 secret
    pub fn new(secret: impl Into<String>) -> Self {
        let secret = secret.into();
        let config = JwtConfig {
            secret: Some(secret.clone()),
            algorithm: Algorithm::HS256,
            ..Default::default()
        };
        
        let validation = Validation::new(Algorithm::HS256);
        let decoding_key = DecodingKey::from_secret(secret.as_bytes());
        
        Self {
            config: Arc::new(config),
            validation,
            decoding_key: Arc::new(decoding_key),
        }
    }
    
    /// Create a new JWT authentication middleware with custom config
    pub fn with_config(config: JwtConfig) -> CoreResult<Self> {
        let decoding_key = if let Some(ref secret) = config.secret {
            DecodingKey::from_secret(secret.as_bytes())
        } else if let Some(ref public_key) = config.public_key {
            match config.algorithm {
                Algorithm::RS256 | Algorithm::RS384 | Algorithm::RS512 => {
                    DecodingKey::from_rsa_pem(public_key.as_bytes())
                        .map_err(|e| octopus_core::Error::Internal(format!("Invalid RSA public key: {}", e)))?
                }
                Algorithm::ES256 | Algorithm::ES384 => {
                    DecodingKey::from_ec_pem(public_key.as_bytes())
                        .map_err(|e| octopus_core::Error::Internal(format!("Invalid EC public key: {}", e)))?
                }
                _ => {
                    return Err(octopus_core::Error::Internal(
                        "Unsupported algorithm for public key".to_string(),
                    ));
                }
            }
        } else {
            return Err(octopus_core::Error::Internal(
                "Either secret or public_key must be provided".to_string(),
            ));
        };
        
        let mut validation = Validation::new(config.algorithm);
        
        // Set audience if configured
        if let Some(ref aud) = config.audience {
            validation.set_audience(&[aud]);
        }
        
        // Set issuer if configured
        if let Some(ref iss) = config.issuer {
            validation.set_issuer(&[iss]);
        }
        
        Ok(Self {
            config: Arc::new(config),
            validation,
            decoding_key: Arc::new(decoding_key),
        })
    }
    
    /// Extract token from request
    fn extract_token(&self, req: &Request<Body>) -> Option<String> {
        req.headers()
            .get(&self.config.header_name)
            .and_then(|value| value.to_str().ok())
            .and_then(|auth_header| {
                if auth_header.starts_with(&self.config.token_prefix) {
                    Some(auth_header[self.config.token_prefix.len()..].to_string())
                } else {
                    None
                }
            })
    }
    
    /// Check if path should skip authentication
    fn should_skip(&self, path: &str) -> bool {
        self.config.skip_paths.iter().any(|skip_path| {
            if skip_path.ends_with('*') {
                // Wildcard matching
                let prefix = &skip_path[..skip_path.len() - 1];
                path.starts_with(prefix)
            } else {
                path == skip_path
            }
        })
    }
    
    /// Build unauthorized response
    fn unauthorized_response(&self, message: &str) -> Response<Body> {
        let body = serde_json::json!({
            "error": "unauthorized",
            "message": message
        });
        
        Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::WWW_AUTHENTICATE, "Bearer")
            .body(Full::new(Bytes::from(body.to_string())))
            .expect("Failed to build unauthorized response")
    }
}

impl fmt::Debug for JwtAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JwtAuth")
            .field("config", &self.config)
            .finish()
    }
}

#[async_trait]
impl Middleware for JwtAuth {
    async fn call(&self, req: Request<Body>, next: Next) -> CoreResult<Response<Body>> {
        let path = req.uri().path();
        
        // Skip authentication for configured paths
        if self.should_skip(path) {
            return next.run(req).await;
        }
        
        // Extract token
        let token = match self.extract_token(&req) {
            Some(token) => token,
            None => {
                tracing::warn!(path = %path, "Missing authentication token");
                return Ok(self.unauthorized_response("Missing authentication token"));
            }
        };
        
        // Validate token
        match decode::<Claims>(&token, &self.decoding_key, &self.validation) {
            Ok(token_data) => {
                tracing::debug!(
                    path = %path,
                    sub = %token_data.claims.sub,
                    "Authentication successful"
                );
                
                // TODO: Add claims to request extensions for downstream middleware
                next.run(req).await
            }
            Err(e) => {
                tracing::warn!(path = %path, error = %e, "Token validation failed");
                let message = match e.kind() {
                    jsonwebtoken::errors::ErrorKind::ExpiredSignature => "Token has expired",
                    jsonwebtoken::errors::ErrorKind::InvalidToken => "Invalid token format",
                    jsonwebtoken::errors::ErrorKind::InvalidSignature => "Invalid token signature",
                    jsonwebtoken::errors::ErrorKind::InvalidIssuer => "Invalid token issuer",
                    jsonwebtoken::errors::ErrorKind::InvalidAudience => "Invalid token audience",
                    _ => "Token validation failed",
                };
                
                Ok(self.unauthorized_response(message))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{encode, EncodingKey, Header};
    use octopus_core::Error;
    use std::time::{SystemTime, UNIX_EPOCH};
    
    #[derive(Debug)]
    struct TestHandler;
    
    #[async_trait]
    impl Middleware for TestHandler {
        async fn call(&self, _req: Request<Body>, _next: Next) -> CoreResult<Response<Body>> {
            Response::builder()
                .status(StatusCode::OK)
                .body(Full::new(Bytes::from("success")))
                .map_err(|e| Error::Internal(e.to_string()))
        }
    }
    
    fn create_test_token(secret: &str, exp_offset: i64) -> String {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as usize;
        
        let claims = Claims {
            sub: "test-user".to_string(),
            exp: (now as i64 + exp_offset) as usize,
            iat: Some(now),
            iss: None,
            aud: None,
            custom: serde_json::json!({}),
        };
        
        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .unwrap()
    }
    
    #[tokio::test]
    async fn test_jwt_auth_success() {
        let secret = "test-secret";
        let jwt_auth = JwtAuth::new(secret);
        let handler = TestHandler;
        
        let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([
            Arc::new(jwt_auth),
            Arc::new(handler),
        ]);
        
        // Create valid token (expires in 1 hour)
        let token = create_test_token(secret, 3600);
        
        let next = Next::new(stack.clone());
        let req = Request::builder()
            .uri("/protected")
            .header("Authorization", format!("Bearer {}", token))
            .body(Body::from(""))
            .unwrap();
        
        let response = next.run(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
    
    #[tokio::test]
    async fn test_jwt_auth_missing_token() {
        let jwt_auth = JwtAuth::new("test-secret");
        let handler = TestHandler;
        
        let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([
            Arc::new(jwt_auth),
            Arc::new(handler),
        ]);
        
        let next = Next::new(stack.clone());
        let req = Request::builder()
            .uri("/protected")
            .body(Body::from(""))
            .unwrap();
        
        let response = next.run(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(response.headers().contains_key("WWW-Authenticate"));
    }
    
    #[tokio::test]
    async fn test_jwt_auth_invalid_token() {
        let jwt_auth = JwtAuth::new("test-secret");
        let handler = TestHandler;
        
        let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([
            Arc::new(jwt_auth),
            Arc::new(handler),
        ]);
        
        let next = Next::new(stack.clone());
        let req = Request::builder()
            .uri("/protected")
            .header("Authorization", "Bearer invalid-token")
            .body(Body::from(""))
            .unwrap();
        
        let response = next.run(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    
    #[tokio::test]
    async fn test_jwt_auth_expired_token() {
        let secret = "test-secret";
        let jwt_auth = JwtAuth::new(secret);
        let handler = TestHandler;
        
        let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([
            Arc::new(jwt_auth),
            Arc::new(handler),
        ]);
        
        // Create expired token (expired 1 hour ago)
        let token = create_test_token(secret, -3600);
        
        let next = Next::new(stack.clone());
        let req = Request::builder()
            .uri("/protected")
            .header("Authorization", format!("Bearer {}", token))
            .body(Body::from(""))
            .unwrap();
        
        let response = next.run(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    
    #[tokio::test]
    async fn test_jwt_auth_skip_paths() {
        let config = JwtConfig {
            secret: Some("test-secret".to_string()),
            skip_paths: vec!["/health".to_string(), "/public/*".to_string()],
            ..Default::default()
        };
        
        let jwt_auth = JwtAuth::with_config(config).unwrap();
        let handler = TestHandler;
        
        let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([
            Arc::new(jwt_auth),
            Arc::new(handler),
        ]);
        
        // Test exact match
        let next = Next::new(stack.clone());
        let req = Request::builder()
            .uri("/health")
            .body(Body::from(""))
            .unwrap();
        let response = next.run(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        
        // Test wildcard match
        let next = Next::new(stack.clone());
        let req = Request::builder()
            .uri("/public/docs")
            .body(Body::from(""))
            .unwrap();
        let response = next.run(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
    
    #[tokio::test]
    async fn test_jwt_auth_wrong_secret() {
        let jwt_auth = JwtAuth::new("correct-secret");
        let handler = TestHandler;
        
        let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([
            Arc::new(jwt_auth),
            Arc::new(handler),
        ]);
        
        // Create token with wrong secret
        let token = create_test_token("wrong-secret", 3600);
        
        let next = Next::new(stack.clone());
        let req = Request::builder()
            .uri("/protected")
            .header("Authorization", format!("Bearer {}", token))
            .body(Body::from(""))
            .unwrap();
        
        let response = next.run(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}

