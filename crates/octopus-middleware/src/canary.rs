//! Traffic splitting / canary deployment middleware

use async_trait::async_trait;
use bytes::Bytes;
use http::{Request, Response};
use http_body_util::Full;
use octopus_core::{Middleware, Next, Result};
use std::fmt;

/// Body type alias
pub type Body = Full<Bytes>;

/// Marker stored in request extensions when a canary upstream is selected.
/// The proxy/handler layer can inspect this to route to the canary backend.
#[derive(Debug, Clone)]
pub struct CanaryUpstream(pub String);

/// A single canary routing rule
#[derive(Debug, Clone)]
pub struct CanaryRule {
    /// Path prefix to match (e.g., "/api/v2")
    pub path_prefix: String,
    /// Upstream address for canary traffic
    pub canary_upstream: String,
    /// Weight 0-100 representing percentage of traffic routed to canary
    pub weight: u32,
    /// Optional header name; if present with value "true" or "1", force canary
    pub header_override: Option<String>,
    /// Optional cookie name; if present with value "true" or "1", force canary
    pub cookie_override: Option<String>,
}

/// Canary deployment configuration
#[derive(Debug, Clone)]
pub struct CanaryConfig {
    /// List of canary routing rules evaluated in order
    pub rules: Vec<CanaryRule>,
}

impl Default for CanaryConfig {
    fn default() -> Self {
        Self { rules: Vec::new() }
    }
}

/// Canary deployment middleware
///
/// Evaluates canary rules against each incoming request to decide whether
/// the request should be routed to a canary upstream. When a canary is
/// selected, a [`CanaryUpstream`] value is inserted into the request
/// extensions so downstream handlers can read it.
#[derive(Clone)]
pub struct Canary {
    config: CanaryConfig,
}

impl Canary {
    /// Create a new Canary middleware with the given config
    pub fn new(config: CanaryConfig) -> Self {
        Self { config }
    }

    /// Deterministic hash of input bytes, returning a value in 0..99
    fn hash_to_percent(data: &[u8]) -> u64 {
        data.iter()
            .fold(0u64, |acc, &b| acc.wrapping_mul(31).wrapping_add(b as u64))
            % 100
    }

    /// Check if a header is present with a truthy value ("true" or "1")
    fn header_is_truthy(req: &Request<Body>, header_name: &str) -> bool {
        req.headers()
            .get(header_name)
            .and_then(|v| v.to_str().ok())
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false)
    }

    /// Check if a cookie with the given name has a truthy value ("true" or "1")
    fn cookie_is_truthy(req: &Request<Body>, cookie_name: &str) -> bool {
        req.headers()
            .get(http::header::COOKIE)
            .and_then(|v| v.to_str().ok())
            .map(|cookie_header| {
                cookie_header.split(';').any(|pair| {
                    let pair = pair.trim();
                    if let Some((name, value)) = pair.split_once('=') {
                        name.trim() == cookie_name
                            && (value.trim() == "true" || value.trim() == "1")
                    } else {
                        false
                    }
                })
            })
            .unwrap_or(false)
    }
}

impl Default for Canary {
    fn default() -> Self {
        Self::new(CanaryConfig::default())
    }
}

impl fmt::Debug for Canary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Canary")
            .field("rules", &self.config.rules)
            .finish()
    }
}

#[async_trait]
impl Middleware for Canary {
    async fn call(&self, mut req: Request<Body>, next: Next) -> Result<Response<Body>> {
        let path = req.uri().path().to_string();

        for rule in &self.config.rules {
            if !path.starts_with(&rule.path_prefix) {
                continue;
            }

            // Check header override
            if let Some(ref header_name) = rule.header_override {
                if Self::header_is_truthy(&req, header_name) {
                    req.extensions_mut()
                        .insert(CanaryUpstream(rule.canary_upstream.clone()));
                    return next.run(req).await;
                }
            }

            // Check cookie override
            if let Some(ref cookie_name) = rule.cookie_override {
                if Self::cookie_is_truthy(&req, cookie_name) {
                    req.extensions_mut()
                        .insert(CanaryUpstream(rule.canary_upstream.clone()));
                    return next.run(req).await;
                }
            }

            // Weight-based routing: hash URI path to get deterministic 0-99 value
            let hash_value = Self::hash_to_percent(path.as_bytes());
            if hash_value < rule.weight as u64 {
                req.extensions_mut()
                    .insert(CanaryUpstream(rule.canary_upstream.clone()));
                return next.run(req).await;
            }

            // First matching rule wins (even if no canary selected)
            break;
        }

        next.run(req).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::StatusCode;
    use octopus_core::Error;
    use std::sync::Arc;

    #[derive(Debug)]
    struct TestHandler;

    #[async_trait]
    impl Middleware for TestHandler {
        async fn call(&self, req: Request<Body>, _next: Next) -> Result<Response<Body>> {
            // Echo back whether canary was selected
            let is_canary = req.extensions().get::<CanaryUpstream>().is_some();
            let body = if is_canary { "canary" } else { "primary" };
            Response::builder()
                .status(StatusCode::OK)
                .body(Full::new(Bytes::from(body)))
                .map_err(|e| Error::Internal(e.to_string()))
        }
    }

    fn make_stack(canary: Canary) -> Arc<[Arc<dyn Middleware>]> {
        Arc::new([
            Arc::new(canary) as Arc<dyn Middleware>,
            Arc::new(TestHandler) as Arc<dyn Middleware>,
        ])
    }

    #[tokio::test]
    async fn test_header_override_routes_to_canary() {
        let config = CanaryConfig {
            rules: vec![CanaryRule {
                path_prefix: "/api".to_string(),
                canary_upstream: "canary-backend:8080".to_string(),
                weight: 0,
                header_override: Some("X-Canary".to_string()),
                cookie_override: None,
            }],
        };
        let canary = Canary::new(config);
        let stack = make_stack(canary);

        let next = Next::new(stack);
        let req = Request::builder()
            .uri("/api/test")
            .header("X-Canary", "true")
            .body(Body::from(""))
            .unwrap();

        let response = next.run(req).await.unwrap();
        let body = http_body_util::BodyExt::collect(response.into_body())
            .await
            .unwrap()
            .to_bytes();
        assert_eq!(&body[..], b"canary");
    }

    #[tokio::test]
    async fn test_weight_zero_never_canary() {
        let config = CanaryConfig {
            rules: vec![CanaryRule {
                path_prefix: "/api".to_string(),
                canary_upstream: "canary-backend:8080".to_string(),
                weight: 0,
                header_override: None,
                cookie_override: None,
            }],
        };
        let canary = Canary::new(config);
        let stack = make_stack(canary);

        // Try multiple paths to ensure none route to canary
        for path in &["/api/a", "/api/b", "/api/c", "/api/xyz"] {
            let next = Next::new(stack.clone());
            let req = Request::builder().uri(*path).body(Body::from("")).unwrap();

            let response = next.run(req).await.unwrap();
            let body = http_body_util::BodyExt::collect(response.into_body())
                .await
                .unwrap()
                .to_bytes();
            assert_eq!(
                &body[..],
                b"primary",
                "path {path} should not route to canary"
            );
        }
    }

    #[tokio::test]
    async fn test_weight_100_always_canary() {
        let config = CanaryConfig {
            rules: vec![CanaryRule {
                path_prefix: "/api".to_string(),
                canary_upstream: "canary-backend:8080".to_string(),
                weight: 100,
                header_override: None,
                cookie_override: None,
            }],
        };
        let canary = Canary::new(config);
        let stack = make_stack(canary);

        for path in &["/api/a", "/api/b", "/api/c", "/api/xyz"] {
            let next = Next::new(stack.clone());
            let req = Request::builder().uri(*path).body(Body::from("")).unwrap();

            let response = next.run(req).await.unwrap();
            let body = http_body_util::BodyExt::collect(response.into_body())
                .await
                .unwrap()
                .to_bytes();
            assert_eq!(&body[..], b"canary", "path {path} should route to canary");
        }
    }

    #[tokio::test]
    async fn test_cookie_override_routes_to_canary() {
        let config = CanaryConfig {
            rules: vec![CanaryRule {
                path_prefix: "/api".to_string(),
                canary_upstream: "canary-backend:8080".to_string(),
                weight: 0,
                header_override: None,
                cookie_override: Some("canary".to_string()),
            }],
        };
        let canary = Canary::new(config);
        let stack = make_stack(canary);

        let next = Next::new(stack);
        let req = Request::builder()
            .uri("/api/test")
            .header("Cookie", "session=abc; canary=1; other=value")
            .body(Body::from(""))
            .unwrap();

        let response = next.run(req).await.unwrap();
        let body = http_body_util::BodyExt::collect(response.into_body())
            .await
            .unwrap()
            .to_bytes();
        assert_eq!(&body[..], b"canary");
    }

    #[tokio::test]
    async fn test_non_matching_path_skips_rule() {
        let config = CanaryConfig {
            rules: vec![CanaryRule {
                path_prefix: "/api".to_string(),
                canary_upstream: "canary-backend:8080".to_string(),
                weight: 100,
                header_override: None,
                cookie_override: None,
            }],
        };
        let canary = Canary::new(config);
        let stack = make_stack(canary);

        let next = Next::new(stack);
        let req = Request::builder()
            .uri("/other/test")
            .body(Body::from(""))
            .unwrap();

        let response = next.run(req).await.unwrap();
        let body = http_body_util::BodyExt::collect(response.into_body())
            .await
            .unwrap()
            .to_bytes();
        assert_eq!(&body[..], b"primary");
    }
}
