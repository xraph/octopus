//! Request size limits middleware
//!
//! Prevents resource exhaustion by limiting the size of request components.
//! Protects against:
//! - Large body attacks (memory exhaustion)
//! - Header bombing (CPU exhaustion)
//! - URI length attacks (buffer overflow)

use async_trait::async_trait;
use http::{Request, Response, StatusCode};
use octopus_core::{Body, Middleware, Next, Result};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Request limits configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestLimitsConfig {
    /// Maximum request body size in bytes
    /// Default: 10MB
    #[serde(default = "default_max_body_size")]
    pub max_body_size: usize,

    /// Maximum total header size in bytes
    /// Default: 8KB
    #[serde(default = "default_max_header_size")]
    pub max_header_size: usize,

    /// Maximum URI length in bytes
    /// Default: 8192 (8KB)
    #[serde(default = "default_max_uri_length")]
    pub max_uri_length: usize,

    /// Custom error message for body size exceeded
    #[serde(default)]
    pub body_size_error_message: Option<String>,

    /// Custom error message for header size exceeded
    #[serde(default)]
    pub header_size_error_message: Option<String>,

    /// Custom error message for URI length exceeded
    #[serde(default)]
    pub uri_length_error_message: Option<String>,
}

fn default_max_body_size() -> usize {
    10 * 1024 * 1024 // 10MB
}

fn default_max_header_size() -> usize {
    8 * 1024 // 8KB
}

fn default_max_uri_length() -> usize {
    8192 // 8KB
}

impl Default for RequestLimitsConfig {
    fn default() -> Self {
        Self {
            max_body_size: default_max_body_size(),
            max_header_size: default_max_header_size(),
            max_uri_length: default_max_uri_length(),
            body_size_error_message: None,
            header_size_error_message: None,
            uri_length_error_message: None,
        }
    }
}

/// Request limits middleware
///
/// Validates request size constraints before processing.
///
/// # Example
///
/// ```
/// use octopus_middleware::{RequestLimits, RequestLimitsConfig};
///
/// // Use defaults (10MB body, 8KB headers, 8KB URI)
/// let limits = RequestLimits::default();
///
/// // Custom limits
/// let config = RequestLimitsConfig {
///     max_body_size: 5 * 1024 * 1024, // 5MB
///     max_header_size: 4 * 1024,      // 4KB
///     max_uri_length: 4096,           // 4KB
///     ..Default::default()
/// };
/// let limits = RequestLimits::with_config(config);
/// ```
#[derive(Debug, Clone)]
pub struct RequestLimits {
    config: RequestLimitsConfig,
}

/// A request limit that was exceeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitViolation {
    /// Request URI exceeded the configured length.
    UriTooLong {
        /// Observed URI length in bytes.
        len: usize,
        /// Configured maximum.
        max: usize,
    },
    /// Total header size exceeded the configured limit.
    HeadersTooLarge {
        /// Observed total header size in bytes.
        size: usize,
        /// Configured maximum.
        max: usize,
    },
    /// Request body exceeded the configured limit.
    BodyTooLarge {
        /// Observed (or declared) body length in bytes.
        len: usize,
        /// Configured maximum.
        max: usize,
    },
}

impl LimitViolation {
    /// The HTTP status this violation should be rejected with.
    #[must_use]
    pub const fn status(&self) -> StatusCode {
        match self {
            Self::UriTooLong { .. } => StatusCode::URI_TOO_LONG,
            Self::HeadersTooLarge { .. } => StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE,
            Self::BodyTooLarge { .. } => StatusCode::PAYLOAD_TOO_LARGE,
        }
    }

    /// Client-facing message used when no custom message is configured.
    #[must_use]
    pub const fn default_message(&self) -> &'static str {
        match self {
            Self::UriTooLong { .. } => "Request URI too long",
            Self::HeadersTooLarge { .. } => "Request headers too large",
            Self::BodyTooLarge { .. } => "Request body too large",
        }
    }
}

impl fmt::Display for LimitViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::UriTooLong { len, max } => {
                write!(f, "request URI length {len} exceeds limit {max}")
            }
            Self::HeadersTooLarge { size, max } => {
                write!(f, "request header size {size} exceeds limit {max}")
            }
            Self::BodyTooLarge { len, max } => {
                write!(f, "request body size {len} exceeds limit {max}")
            }
        }
    }
}

impl RequestLimits {
    /// Create a new request limits middleware with default configuration
    pub fn new() -> Self {
        Self {
            config: RequestLimitsConfig::default(),
        }
    }

    /// Create a new request limits middleware with custom configuration
    pub fn with_config(config: RequestLimitsConfig) -> Self {
        Self { config }
    }

    /// Create a strict request limits configuration
    /// Recommended for public APIs
    pub fn strict() -> Self {
        Self {
            config: RequestLimitsConfig {
                max_body_size: 1024 * 1024, // 1MB
                max_header_size: 4 * 1024,  // 4KB
                max_uri_length: 2048,       // 2KB
                body_size_error_message: Some(
                    "Request body too large (max 1MB allowed)".to_string(),
                ),
                header_size_error_message: Some(
                    "Request headers too large (max 4KB allowed)".to_string(),
                ),
                uri_length_error_message: Some(
                    "Request URI too long (max 2KB allowed)".to_string(),
                ),
            },
        }
    }

    /// Create a permissive request limits configuration
    /// Use for internal APIs or file uploads
    pub fn permissive() -> Self {
        Self {
            config: RequestLimitsConfig {
                max_body_size: 100 * 1024 * 1024, // 100MB
                max_header_size: 16 * 1024,       // 16KB
                max_uri_length: 16384,            // 16KB
                body_size_error_message: None,
                header_size_error_message: None,
                uri_length_error_message: None,
            },
        }
    }

    /// Build limits from gateway configuration.
    ///
    /// `gateway_max_body_size` is `gateway.max_body_size`, used when
    /// `config.max_body_size` is unset. Unset header and URI limits are left
    /// unenforced (`usize::MAX`) rather than defaulted, so enabling this on an
    /// existing deployment cannot start rejecting traffic that worked before.
    #[must_use]
    pub fn from_config(
        config: &octopus_config::types::RequestLimitsConfig,
        gateway_max_body_size: usize,
    ) -> Self {
        Self {
            config: RequestLimitsConfig {
                max_body_size: config.max_body_size.unwrap_or(gateway_max_body_size),
                max_header_size: config.max_header_size.unwrap_or(usize::MAX),
                max_uri_length: config.max_uri_length.unwrap_or(usize::MAX),
                body_size_error_message: config.body_size_error_message.clone(),
                header_size_error_message: config.header_size_error_message.clone(),
                uri_length_error_message: config.uri_length_error_message.clone(),
            },
        }
    }

    /// The resolved body-size cap, for wrapping a streaming body.
    #[must_use]
    pub const fn max_body_size(&self) -> usize {
        self.config.max_body_size
    }

    /// Check everything knowable before the body is read: URI length, total
    /// header size, and a declared `Content-Length`.
    ///
    /// This is the single implementation of the policy — both the [`Middleware`]
    /// impl and the gateway's pre-buffering path call it.
    ///
    /// A request passing this check is **not** proven to be within the body
    /// limit: `Content-Length` is absent on chunked, HTTP/2 and HTTP/3 requests.
    /// Callers that can must additionally cap the body stream at
    /// [`max_body_size`](Self::max_body_size).
    ///
    /// # Errors
    /// Returns the first [`LimitViolation`] found.
    pub fn check_parts(
        &self,
        uri: &http::Uri,
        headers: &http::HeaderMap,
    ) -> std::result::Result<(), LimitViolation> {
        let uri_len = uri.to_string().len();
        if uri_len > self.config.max_uri_length {
            return Err(LimitViolation::UriTooLong {
                len: uri_len,
                max: self.config.max_uri_length,
            });
        }

        let header_size = Self::calculate_header_size(headers);
        if header_size > self.config.max_header_size {
            return Err(LimitViolation::HeadersTooLarge {
                size: header_size,
                max: self.config.max_header_size,
            });
        }

        if let Some(len) = headers
            .get(http::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<usize>().ok())
        {
            if len > self.config.max_body_size {
                return Err(LimitViolation::BodyTooLarge {
                    len,
                    max: self.config.max_body_size,
                });
            }
        }

        Ok(())
    }

    /// Build the rejection response for a violation, applying any configured
    /// custom message.
    #[must_use]
    pub fn violation_response(&self, violation: &LimitViolation) -> Response<Body> {
        let custom = match violation {
            LimitViolation::UriTooLong { .. } => self.config.uri_length_error_message.as_deref(),
            LimitViolation::HeadersTooLarge { .. } => {
                self.config.header_size_error_message.as_deref()
            }
            LimitViolation::BodyTooLarge { .. } => self.config.body_size_error_message.as_deref(),
        };
        Self::error_response(
            violation.status(),
            custom.unwrap_or_else(|| violation.default_message()),
        )
    }

    fn calculate_header_size(headers: &http::HeaderMap) -> usize {
        headers
            .iter()
            .map(|(name, value)| name.as_str().len() + value.len() + 4) // ": " + "\r\n"
            .sum()
    }

    fn error_response(status: StatusCode, message: &str) -> Response<Body> {
        use bytes::Bytes;
        use http_body_util::Full;

        let body = serde_json::json!({
            "error": "request_limit_exceeded",
            "message": message,
            "status": status.as_u16(),
        })
        .to_string();

        Response::builder()
            .status(status)
            .header("content-type", "application/json")
            .body(Full::new(Bytes::from(body)))
            .expect("Failed to build error response")
    }
}

impl Default for RequestLimits {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Middleware for RequestLimits {
    async fn call(&self, req: Request<Body>, next: Next) -> Result<Response<Body>> {
        // The chain's Body is already buffered, so this can only enforce the
        // header-derived limits. The gateway calls check_parts directly before
        // buffering; this impl exists for embedders building their own chain.
        if let Err(violation) = self.check_parts(req.uri(), req.headers()) {
            tracing::warn!(%violation, "Request limit exceeded");
            return Ok(self.violation_response(&violation));
        }

        next.run(req).await
    }
}

#[cfg(test)]
mod pre_buffer_tests {
    use super::*;
    use http::{HeaderMap, HeaderValue, Uri};

    fn limits() -> RequestLimits {
        RequestLimits::with_config(RequestLimitsConfig {
            max_body_size: 100,
            max_header_size: 200,
            max_uri_length: 50,
            ..RequestLimitsConfig::default()
        })
    }

    #[test]
    fn within_limits_passes() {
        let uri: Uri = "/short".parse().unwrap();
        assert!(limits().check_parts(&uri, &HeaderMap::new()).is_ok());
    }

    #[test]
    fn over_long_uri_is_a_violation() {
        let uri: Uri = format!("/{}", "a".repeat(200)).parse().unwrap();
        assert!(matches!(
            limits().check_parts(&uri, &HeaderMap::new()),
            Err(LimitViolation::UriTooLong { .. })
        ));
    }

    #[test]
    fn oversized_headers_are_a_violation() {
        let mut headers = HeaderMap::new();
        headers.insert("x-big", HeaderValue::from_str(&"v".repeat(300)).unwrap());
        let uri: Uri = "/short".parse().unwrap();
        assert!(matches!(
            limits().check_parts(&uri, &headers),
            Err(LimitViolation::HeadersTooLarge { .. })
        ));
    }

    #[test]
    fn content_length_over_limit_is_a_violation() {
        let mut headers = HeaderMap::new();
        headers.insert("content-length", HeaderValue::from_static("101"));
        let uri: Uri = "/short".parse().unwrap();
        assert!(matches!(
            limits().check_parts(&uri, &headers),
            Err(LimitViolation::BodyTooLarge { .. })
        ));
    }

    #[test]
    fn missing_content_length_passes_the_header_check() {
        // Chunked, HTTP/2 and HTTP/3 requests routinely omit content-length.
        // The pre-buffer check cannot catch those; the streaming cap at the
        // collect site is what actually bounds them. This test pins that
        // division of responsibility so the header check is never mistaken
        // for complete body enforcement.
        let uri: Uri = "/short".parse().unwrap();
        assert!(limits().check_parts(&uri, &HeaderMap::new()).is_ok());
        assert_eq!(limits().max_body_size(), 100);
    }

    #[test]
    fn from_config_falls_back_to_gateway_max_body_size() {
        let cfg = octopus_config::types::RequestLimitsConfig::default();
        let limits = RequestLimits::from_config(&cfg, 4096);
        assert_eq!(limits.max_body_size(), 4096);
    }

    #[test]
    fn from_config_prefers_explicit_body_size() {
        let cfg = octopus_config::types::RequestLimitsConfig {
            max_body_size: Some(64),
            ..Default::default()
        };
        assert_eq!(RequestLimits::from_config(&cfg, 4096).max_body_size(), 64);
    }

    #[test]
    fn from_config_leaves_unset_header_and_uri_limits_unenforced() {
        let cfg = octopus_config::types::RequestLimitsConfig::default();
        let limits = RequestLimits::from_config(&cfg, 4096);

        let mut headers = HeaderMap::new();
        headers.insert("x-big", HeaderValue::from_str(&"v".repeat(9000)).unwrap());
        let uri: Uri = format!("/{}", "a".repeat(9000)).parse().unwrap();

        // Neither limit was configured, so neither rejects.
        assert!(limits.check_parts(&uri, &headers).is_ok());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
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
                .body(Full::new(Bytes::from("success")))
                .unwrap())
        }
    }

    #[tokio::test]
    async fn test_default_limits_accept_normal_request() {
        let limits = RequestLimits::default();
        let handler = TestHandler;
        let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([Arc::new(limits), Arc::new(handler)]);

        let req = Request::builder()
            .uri("/test")
            .header("content-length", "1024")
            .body(Full::new(Bytes::from("test")))
            .unwrap();

        let next = Next::new(stack);
        let response = next.run(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_reject_large_body() {
        let config = RequestLimitsConfig {
            max_body_size: 1024, // 1KB limit
            ..Default::default()
        };
        let limits = RequestLimits::with_config(config);
        let handler = TestHandler;
        let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([Arc::new(limits), Arc::new(handler)]);

        let req = Request::builder()
            .uri("/test")
            .header("content-length", "2048") // 2KB body
            .body(Full::new(Bytes::from("test")))
            .unwrap();

        let next = Next::new(stack);
        let response = next.run(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn test_reject_long_uri() {
        let config = RequestLimitsConfig {
            max_uri_length: 100,
            ..Default::default()
        };
        let limits = RequestLimits::with_config(config);
        let handler = TestHandler;
        let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([Arc::new(limits), Arc::new(handler)]);

        let long_path = format!("/test/{}", "a".repeat(200));
        let req = Request::builder()
            .uri(long_path)
            .body(Full::new(Bytes::from("")))
            .unwrap();

        let next = Next::new(stack);
        let response = next.run(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::URI_TOO_LONG);
    }

    #[tokio::test]
    async fn test_reject_large_headers() {
        let config = RequestLimitsConfig {
            max_header_size: 100,
            ..Default::default()
        };
        let limits = RequestLimits::with_config(config);
        let handler = TestHandler;
        let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([Arc::new(limits), Arc::new(handler)]);

        let req = Request::builder()
            .uri("/test")
            .header("x-custom-header", "a".repeat(200))
            .body(Full::new(Bytes::from("")))
            .unwrap();

        let next = Next::new(stack);
        let response = next.run(req).await.unwrap();

        assert_eq!(
            response.status(),
            StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE
        );
    }

    #[tokio::test]
    async fn test_strict_limits() {
        let limits = RequestLimits::strict();
        let handler = TestHandler;
        let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([Arc::new(limits), Arc::new(handler)]);

        // Should reject 2MB body (strict allows only 1MB)
        let req = Request::builder()
            .uri("/test")
            .header("content-length", "2097152") // 2MB
            .body(Full::new(Bytes::from("test")))
            .unwrap();

        let next = Next::new(stack);
        let response = next.run(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn test_permissive_limits() {
        let limits = RequestLimits::permissive();
        let handler = TestHandler;
        let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([Arc::new(limits), Arc::new(handler)]);

        // Should accept 50MB body (permissive allows up to 100MB)
        let req = Request::builder()
            .uri("/test")
            .header("content-length", "52428800") // 50MB
            .body(Full::new(Bytes::from("test")))
            .unwrap();

        let next = Next::new(stack);
        let response = next.run(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_custom_error_messages() {
        let config = RequestLimitsConfig {
            max_body_size: 1024,
            body_size_error_message: Some("Custom error message".to_string()),
            ..Default::default()
        };
        let limits = RequestLimits::with_config(config);
        let handler = TestHandler;
        let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([Arc::new(limits), Arc::new(handler)]);

        let req = Request::builder()
            .uri("/test")
            .header("content-length", "2048")
            .body(Full::new(Bytes::from("test")))
            .unwrap();

        let next = Next::new(stack);
        let response = next.run(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);

        use http_body_util::BodyExt;
        let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body_str = String::from_utf8_lossy(&body_bytes);
        assert!(body_str.contains("Custom error message"));
    }
}
