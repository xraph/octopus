//! Security headers middleware
//!
//! Adds security-related HTTP headers to responses to protect against
//! common web vulnerabilities (XSS, clickjacking, MIME sniffing, etc.)

use async_trait::async_trait;
use http::{HeaderName, HeaderValue, Request, Response};
use octopus_core::{Body, Middleware, Next, Result};
use serde::{Deserialize, Serialize};

/// Security headers middleware configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityHeadersConfig {
    /// Strict-Transport-Security header
    /// Forces HTTPS connections
    /// Example: "max-age=31536000; includeSubDomains; preload"
    #[serde(default = "default_hsts")]
    pub hsts: Option<String>,

    /// Content-Security-Policy header
    /// Controls resources the browser is allowed to load
    /// Example: "default-src 'self'; script-src 'self' 'unsafe-inline'"
    #[serde(default)]
    pub csp: Option<String>,

    /// X-Frame-Options header
    /// Prevents clickjacking attacks
    /// Values: "DENY", "SAMEORIGIN", or "ALLOW-FROM uri"
    #[serde(default = "default_frame_options")]
    pub frame_options: Option<String>,

    /// X-Content-Type-Options header
    /// Prevents MIME type sniffing
    /// Value: "nosniff"
    #[serde(default = "default_content_type_options")]
    pub content_type_options: Option<String>,

    /// X-XSS-Protection header
    /// Enables XSS filtering in older browsers
    /// Example: "1; mode=block"
    #[serde(default = "default_xss_protection")]
    pub xss_protection: Option<String>,

    /// Referrer-Policy header
    /// Controls referrer information
    /// Example: "strict-origin-when-cross-origin"
    #[serde(default = "default_referrer_policy")]
    pub referrer_policy: Option<String>,

    /// Permissions-Policy header
    /// Controls browser features and APIs
    /// Example: "geolocation=(), microphone=()"
    #[serde(default)]
    pub permissions_policy: Option<String>,
}

fn default_hsts() -> Option<String> {
    Some("max-age=31536000; includeSubDomains".to_string())
}

fn default_frame_options() -> Option<String> {
    Some("DENY".to_string())
}

fn default_content_type_options() -> Option<String> {
    Some("nosniff".to_string())
}

fn default_xss_protection() -> Option<String> {
    Some("1; mode=block".to_string())
}

fn default_referrer_policy() -> Option<String> {
    Some("strict-origin-when-cross-origin".to_string())
}

impl Default for SecurityHeadersConfig {
    fn default() -> Self {
        Self {
            hsts: default_hsts(),
            csp: Some("default-src 'self'".to_string()),
            frame_options: default_frame_options(),
            content_type_options: default_content_type_options(),
            xss_protection: default_xss_protection(),
            referrer_policy: default_referrer_policy(),
            permissions_policy: None,
        }
    }
}

/// Security headers middleware
///
/// Automatically adds security headers to all responses.
///
/// # Example
///
/// ```
/// use octopus_middleware::SecurityHeaders;
///
/// let security = SecurityHeaders::default();
/// ```
#[derive(Debug, Clone)]
pub struct SecurityHeaders {
    config: SecurityHeadersConfig,
}

impl SecurityHeaders {
    /// Create a new security headers middleware with default configuration
    pub fn new() -> Self {
        Self {
            config: SecurityHeadersConfig::default(),
        }
    }

    /// Create a new security headers middleware with custom configuration
    pub fn with_config(config: SecurityHeadersConfig) -> Self {
        Self { config }
    }

    /// Create a strict security headers configuration
    /// Recommended for production environments
    pub fn strict() -> Self {
        Self {
            config: SecurityHeadersConfig {
                hsts: Some("max-age=63072000; includeSubDomains; preload".to_string()),
                csp: Some(
                    "default-src 'self'; script-src 'self'; object-src 'none'; base-uri 'self'; frame-ancestors 'none'"
                        .to_string(),
                ),
                frame_options: Some("DENY".to_string()),
                content_type_options: Some("nosniff".to_string()),
                xss_protection: Some("1; mode=block".to_string()),
                referrer_policy: Some("no-referrer".to_string()),
                permissions_policy: Some(
                    "geolocation=(), microphone=(), camera=(), payment=()".to_string(),
                ),
            },
        }
    }

    /// Create a permissive security headers configuration
    /// Use for development or when you need more flexibility
    pub fn permissive() -> Self {
        Self {
            config: SecurityHeadersConfig {
                hsts: None, // Don't force HTTPS in dev
                csp: Some("default-src 'self' 'unsafe-inline' 'unsafe-eval'".to_string()),
                frame_options: Some("SAMEORIGIN".to_string()),
                content_type_options: Some("nosniff".to_string()),
                xss_protection: Some("1; mode=block".to_string()),
                referrer_policy: Some("origin-when-cross-origin".to_string()),
                permissions_policy: None,
            },
        }
    }

    fn add_header(
        response: &mut Response<Body>,
        name: &'static str,
        value: Option<&str>,
    ) -> Result<()> {
        if let Some(val) = value {
            let header_name = HeaderName::from_static(name);
            let header_value = HeaderValue::from_str(val).map_err(|e| {
                octopus_core::Error::Internal(format!("Invalid {} header: {}", name, e))
            })?;
            response.headers_mut().insert(header_name, header_value);
        }
        Ok(())
    }
}

impl Default for SecurityHeaders {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Middleware for SecurityHeaders {
    async fn call(&self, req: Request<Body>, next: Next) -> Result<Response<Body>> {
        let mut response = next.run(req).await?;

        // Add all configured security headers
        Self::add_header(
            &mut response,
            "strict-transport-security",
            self.config.hsts.as_deref(),
        )?;

        Self::add_header(
            &mut response,
            "content-security-policy",
            self.config.csp.as_deref(),
        )?;

        Self::add_header(
            &mut response,
            "x-frame-options",
            self.config.frame_options.as_deref(),
        )?;

        Self::add_header(
            &mut response,
            "x-content-type-options",
            self.config.content_type_options.as_deref(),
        )?;

        Self::add_header(
            &mut response,
            "x-xss-protection",
            self.config.xss_protection.as_deref(),
        )?;

        Self::add_header(
            &mut response,
            "referrer-policy",
            self.config.referrer_policy.as_deref(),
        )?;

        Self::add_header(
            &mut response,
            "permissions-policy",
            self.config.permissions_policy.as_deref(),
        )?;

        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use http::StatusCode;
    use http_body_util::Full;
    use std::sync::Arc;

    type TestBody = Full<Bytes>;

    // Mock handler for testing
    #[derive(Debug, Clone)]
    struct TestHandler;

    #[async_trait]
    impl Middleware for TestHandler {
        async fn call(&self, _req: Request<TestBody>, _next: Next) -> Result<Response<TestBody>> {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(Full::new(Bytes::from("test")))
                .unwrap())
        }
    }

    #[tokio::test]
    async fn test_default_security_headers() {
        let security = SecurityHeaders::default();
        let handler = TestHandler;
        let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([Arc::new(security), Arc::new(handler)]);

        let req = Request::builder()
            .uri("/test")
            .body(Full::new(Bytes::from("")))
            .unwrap();

        let next = Next::new(stack);
        let response = next.run(req).await.unwrap();

        // Check default headers
        assert!(response.headers().contains_key("strict-transport-security"));
        assert_eq!(
            response
                .headers()
                .get("x-content-type-options")
                .unwrap()
                .to_str()
                .unwrap(),
            "nosniff"
        );
        assert_eq!(
            response
                .headers()
                .get("x-frame-options")
                .unwrap()
                .to_str()
                .unwrap(),
            "DENY"
        );
    }

    #[tokio::test]
    async fn test_strict_security_headers() {
        let security = SecurityHeaders::strict();
        let handler = TestHandler;
        let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([Arc::new(security), Arc::new(handler)]);

        let req = Request::builder()
            .uri("/test")
            .body(Full::new(Bytes::from("")))
            .unwrap();

        let next = Next::new(stack);
        let response = next.run(req).await.unwrap();

        // Check strict headers
        let hsts = response
            .headers()
            .get("strict-transport-security")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(hsts.contains("max-age=63072000"));
        assert!(hsts.contains("includeSubDomains"));
        assert!(hsts.contains("preload"));

        let csp = response
            .headers()
            .get("content-security-policy")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(csp.contains("default-src 'self'"));
        assert!(csp.contains("frame-ancestors 'none'"));

        assert!(response.headers().contains_key("permissions-policy"));
    }

    #[tokio::test]
    async fn test_permissive_security_headers() {
        let security = SecurityHeaders::permissive();
        let handler = TestHandler;
        let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([Arc::new(security), Arc::new(handler)]);

        let req = Request::builder()
            .uri("/test")
            .body(Full::new(Bytes::from("")))
            .unwrap();

        let next = Next::new(stack);
        let response = next.run(req).await.unwrap();

        // Should not have HSTS in permissive mode
        assert!(!response.headers().contains_key("strict-transport-security"));

        // Should have permissive CSP
        let csp = response
            .headers()
            .get("content-security-policy")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(csp.contains("unsafe-inline"));
        assert!(csp.contains("unsafe-eval"));
    }

    #[tokio::test]
    async fn test_custom_security_headers() {
        let config = SecurityHeadersConfig {
            hsts: Some("max-age=0".to_string()),
            csp: Some("default-src 'none'".to_string()),
            frame_options: Some("SAMEORIGIN".to_string()),
            content_type_options: Some("nosniff".to_string()),
            xss_protection: None,
            referrer_policy: Some("no-referrer".to_string()),
            permissions_policy: Some("geolocation=()".to_string()),
        };

        let security = SecurityHeaders::with_config(config);
        let handler = TestHandler;
        let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([Arc::new(security), Arc::new(handler)]);

        let req = Request::builder()
            .uri("/test")
            .body(Full::new(Bytes::from("")))
            .unwrap();

        let next = Next::new(stack);
        let response = next.run(req).await.unwrap();

        assert_eq!(
            response
                .headers()
                .get("strict-transport-security")
                .unwrap()
                .to_str()
                .unwrap(),
            "max-age=0"
        );
        assert_eq!(
            response
                .headers()
                .get("content-security-policy")
                .unwrap()
                .to_str()
                .unwrap(),
            "default-src 'none'"
        );
        assert!(!response.headers().contains_key("x-xss-protection"));
    }
}
