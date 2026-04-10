//! Forward authentication middleware
//!
//! Delegates authentication to an external service by sending a subrequest.
//! The external service responds with 200 to allow or 401/403 to deny.

use async_trait::async_trait;
use bytes::Bytes;
use http::{Request, Response, StatusCode};
use http_body_util::Full;
use octopus_core::{Middleware, Next, Result};
use std::fmt;
use std::time::Duration;

/// Body type alias
pub type Body = Full<Bytes>;

/// Forward auth configuration
#[derive(Debug, Clone)]
pub struct ForwardAuthConfig {
    /// Auth service endpoint URL (e.g., "http://auth-service:8080/verify")
    pub endpoint: String,
    /// Headers to forward from the original request to the auth service
    pub forward_headers: Vec<String>,
    /// Headers to copy from the auth response back to the upstream request
    pub response_headers: Vec<String>,
    /// Timeout for the auth subrequest
    pub timeout: Duration,
    /// Paths to skip authentication (supports simple glob with trailing `*`)
    pub skip_paths: Vec<String>,
}

impl Default for ForwardAuthConfig {
    fn default() -> Self {
        Self {
            endpoint: String::new(),
            forward_headers: vec![
                "Authorization".to_string(),
                "Cookie".to_string(),
                "X-Forwarded-For".to_string(),
            ],
            response_headers: vec![
                "X-Auth-Subject".to_string(),
                "X-Auth-Role".to_string(),
                "X-Auth-Scopes".to_string(),
            ],
            timeout: Duration::from_secs(5),
            skip_paths: vec![],
        }
    }
}

/// Forward authentication middleware
///
/// On each request, sends a subrequest to an external auth service.
/// If the auth service returns 200, the request proceeds.
/// If the auth service returns non-200 (401/403), the response is returned to the client.
#[derive(Clone)]
pub struct ForwardAuth {
    config: ForwardAuthConfig,
    client: ForwardAuthClient,
}

/// Abstraction over the HTTP client for testability
#[derive(Clone)]
struct ForwardAuthClient {
    endpoint: String,
    timeout: Duration,
}

/// Auth subrequest result
#[derive(Debug)]
struct AuthResult {
    status: StatusCode,
    headers: Vec<(String, String)>,
}

impl ForwardAuthClient {
    fn new(endpoint: String, timeout: Duration) -> Self {
        Self { endpoint, timeout }
    }

    /// Send an auth subrequest
    async fn verify(
        &self,
        original_uri: &str,
        original_method: &str,
        forward_headers: &[(String, String)],
    ) -> std::result::Result<AuthResult, ForwardAuthError> {
        // Build the auth request using hyper
        use http::header::HeaderValue;
        use hyper_util::client::legacy::Client;
        use hyper_util::rt::TokioExecutor;

        let mut builder = Request::builder()
            .method("GET")
            .uri(&self.endpoint)
            .header("X-Original-URI", original_uri)
            .header("X-Original-Method", original_method);

        for (name, value) in forward_headers {
            if let Ok(hv) = HeaderValue::from_str(value) {
                builder = builder.header(name.as_str(), hv);
            }
        }

        let req = builder
            .body(Full::<Bytes>::new(Bytes::new()))
            .map_err(|e| ForwardAuthError::RequestBuild(e.to_string()))?;

        let client =
            Client::builder(TokioExecutor::new()).build_http::<Full<Bytes>>();

        let result = tokio::time::timeout(self.timeout, client.request(req))
            .await
            .map_err(|_| ForwardAuthError::Timeout)?
            .map_err(|e| ForwardAuthError::Connection(e.to_string()))?;

        let status = result.status();
        let mut headers = Vec::new();
        for (name, value) in result.headers() {
            if let Ok(v) = value.to_str() {
                headers.push((name.to_string(), v.to_string()));
            }
        }

        Ok(AuthResult { status, headers })
    }
}

/// Errors from forward auth
#[derive(Debug)]
enum ForwardAuthError {
    Timeout,
    Connection(String),
    RequestBuild(String),
}

impl ForwardAuth {
    /// Create a new ForwardAuth with custom config
    pub fn with_config(config: ForwardAuthConfig) -> Self {
        let client = ForwardAuthClient::new(config.endpoint.clone(), config.timeout);
        Self { config, client }
    }

    /// Check if a path should skip authentication
    fn should_skip(&self, path: &str) -> bool {
        for skip in &self.config.skip_paths {
            if skip.ends_with('*') {
                let prefix = &skip[..skip.len() - 1];
                if path.starts_with(prefix) {
                    return true;
                }
            } else if path == skip {
                return true;
            }
        }
        false
    }

    /// Build an error response
    fn error_response(status: StatusCode, message: &str) -> Response<Body> {
        Response::builder()
            .status(status)
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(
                serde_json::json!({
                    "error": "authentication_failed",
                    "message": message
                })
                .to_string(),
            )))
            .expect("Failed to build error response")
    }
}

impl fmt::Debug for ForwardAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ForwardAuth")
            .field("endpoint", &self.config.endpoint)
            .field("skip_paths", &self.config.skip_paths)
            .finish()
    }
}

#[async_trait]
impl Middleware for ForwardAuth {
    async fn call(&self, mut req: Request<Body>, next: Next) -> Result<Response<Body>> {
        let path = req.uri().path().to_string();

        // Check if path should skip auth
        if self.should_skip(&path) {
            return next.run(req).await;
        }

        let method = req.method().to_string();

        // Collect headers to forward
        let forward_headers: Vec<(String, String)> = self
            .config
            .forward_headers
            .iter()
            .filter_map(|name| {
                req.headers()
                    .get(name.as_str())
                    .and_then(|v| v.to_str().ok())
                    .map(|v| (name.clone(), v.to_string()))
            })
            .collect();

        // Send auth subrequest
        let auth_result = match self.client.verify(&path, &method, &forward_headers).await {
            Ok(result) => result,
            Err(ForwardAuthError::Timeout) => {
                tracing::warn!(endpoint = %self.config.endpoint, "Forward auth timeout");
                return Ok(Self::error_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Authentication service timeout",
                ));
            }
            Err(ForwardAuthError::Connection(e)) => {
                tracing::warn!(endpoint = %self.config.endpoint, error = %e, "Forward auth connection error");
                return Ok(Self::error_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Authentication service unavailable",
                ));
            }
            Err(ForwardAuthError::RequestBuild(e)) => {
                tracing::error!(error = %e, "Failed to build forward auth request");
                return Ok(Self::error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal authentication error",
                ));
            }
        };

        if auth_result.status == StatusCode::OK {
            // Copy configured response headers from auth response to upstream request
            for (name, value) in &auth_result.headers {
                if self.config.response_headers.iter().any(|h| h.eq_ignore_ascii_case(name)) {
                    if let Ok(hv) = http::header::HeaderValue::from_str(value) {
                        if let Ok(hn) = http::header::HeaderName::from_bytes(name.as_bytes()) {
                            req.headers_mut().insert(hn, hv);
                        }
                    }
                }
            }
            next.run(req).await
        } else {
            // Return the auth service's status to client
            let status = auth_result.status;
            let message = match status {
                StatusCode::UNAUTHORIZED => "Unauthorized",
                StatusCode::FORBIDDEN => "Forbidden",
                _ => "Authentication failed",
            };
            tracing::warn!(
                uri = %path,
                auth_status = %status,
                "Forward auth rejected request"
            );
            Ok(Self::error_response(status, message))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_skip_exact_path() {
        let config = ForwardAuthConfig {
            skip_paths: vec!["/health".to_string(), "/ready".to_string()],
            ..Default::default()
        };
        let fa = ForwardAuth::with_config(config);
        assert!(fa.should_skip("/health"));
        assert!(fa.should_skip("/ready"));
        assert!(!fa.should_skip("/api/data"));
    }

    #[test]
    fn test_should_skip_wildcard_path() {
        let config = ForwardAuthConfig {
            skip_paths: vec!["/public/*".to_string()],
            ..Default::default()
        };
        let fa = ForwardAuth::with_config(config);
        assert!(fa.should_skip("/public/images/logo.png"));
        assert!(fa.should_skip("/public/"));
        assert!(!fa.should_skip("/api/public"));
    }

    #[test]
    fn test_default_config() {
        let config = ForwardAuthConfig::default();
        assert_eq!(config.timeout, Duration::from_secs(5));
        assert!(config.forward_headers.contains(&"Authorization".to_string()));
        assert!(config.forward_headers.contains(&"Cookie".to_string()));
        assert!(config.response_headers.contains(&"X-Auth-Subject".to_string()));
        assert!(config.response_headers.contains(&"X-Auth-Role".to_string()));
    }

    #[test]
    fn test_skip_paths_empty() {
        let config = ForwardAuthConfig::default();
        let fa = ForwardAuth::with_config(config);
        assert!(!fa.should_skip("/anything"));
    }

    #[test]
    fn test_error_response_status_codes() {
        let resp = ForwardAuth::error_response(StatusCode::UNAUTHORIZED, "test");
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let resp = ForwardAuth::error_response(StatusCode::FORBIDDEN, "test");
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        let resp = ForwardAuth::error_response(StatusCode::SERVICE_UNAVAILABLE, "test");
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}
