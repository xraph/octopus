//! HTTP redirect middleware
//!
//! Supports regex-based path redirects, HTTPS enforcement, and trailing slash normalization.

use async_trait::async_trait;
use bytes::Bytes;
use http::{header, HeaderValue, Request, Response, StatusCode, Uri};
use http_body_util::Full;
use octopus_core::{Error, Middleware, Next, Result};
use regex::Regex;
use std::fmt;

/// Body type alias
pub type Body = Full<Bytes>;

/// Trailing slash behavior
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrailingSlash {
    /// Add a trailing slash if missing
    Add,
    /// Strip trailing slash if present
    Strip,
    /// Do nothing
    Ignore,
}

impl Default for TrailingSlash {
    fn default() -> Self {
        TrailingSlash::Ignore
    }
}

/// A single redirect rule
#[derive(Debug, Clone)]
pub struct RedirectRule {
    /// Regex pattern matched against the request path
    pub from: String,
    /// Replacement string (supports `$1`, `$2` captures)
    pub to: String,
    /// HTTP status code for the redirect (301, 302, 307, 308)
    pub status: u16,
    /// Whether to preserve the query string on redirect
    pub preserve_query: bool,
}

/// Compiled form of a redirect rule (kept internal)
struct CompiledRule {
    pattern: Regex,
    to: String,
    status: StatusCode,
    preserve_query: bool,
}

impl fmt::Debug for CompiledRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CompiledRule")
            .field("pattern", &self.pattern.as_str())
            .field("to", &self.to)
            .field("status", &self.status)
            .finish()
    }
}

/// Redirect middleware configuration
#[derive(Debug, Clone)]
pub struct RedirectConfig {
    /// Redirect rules evaluated in order (first match wins)
    pub rules: Vec<RedirectRule>,
    /// If true, redirect HTTP requests to HTTPS
    pub https_redirect: bool,
    /// Trailing slash normalization behavior
    pub trailing_slash: TrailingSlash,
}

impl Default for RedirectConfig {
    fn default() -> Self {
        Self {
            rules: Vec::new(),
            https_redirect: false,
            trailing_slash: TrailingSlash::Ignore,
        }
    }
}

/// HTTP redirect middleware
///
/// Evaluates redirect rules, HTTPS enforcement, and trailing slash normalization.
/// Returns redirect responses without calling downstream middleware (short-circuit).
pub struct Redirect {
    compiled_rules: Vec<CompiledRule>,
    https_redirect: bool,
    trailing_slash: TrailingSlash,
}

impl Redirect {
    /// Create a new Redirect middleware from config.
    ///
    /// Regex patterns are pre-compiled here; invalid patterns cause an error.
    pub fn new(config: RedirectConfig) -> Result<Self> {
        let mut compiled_rules = Vec::with_capacity(config.rules.len());
        for rule in &config.rules {
            let pattern = Regex::new(&rule.from).map_err(|e| {
                Error::Config(format!(
                    "Invalid redirect regex '{}': {}",
                    rule.from, e
                ))
            })?;
            let status = StatusCode::from_u16(rule.status).map_err(|_| {
                Error::Config(format!("Invalid redirect status code: {}", rule.status))
            })?;
            compiled_rules.push(CompiledRule {
                pattern,
                to: rule.to.clone(),
                status,
                preserve_query: rule.preserve_query,
            });
        }
        Ok(Self {
            compiled_rules,
            https_redirect: config.https_redirect,
            trailing_slash: config.trailing_slash,
        })
    }

    /// Build a redirect response
    fn redirect_response(location: &str, status: StatusCode) -> Result<Response<Body>> {
        let header_val = HeaderValue::from_str(location)
            .map_err(|e| Error::Internal(format!("Invalid redirect location: {}", e)))?;
        Response::builder()
            .status(status)
            .header(header::LOCATION, header_val)
            .body(Full::new(Bytes::new()))
            .map_err(|e| Error::Internal(e.to_string()))
    }

    /// Check and perform HTTPS redirect
    fn check_https_redirect(&self, uri: &Uri, host: Option<&str>) -> Option<String> {
        if !self.https_redirect {
            return None;
        }
        // Only redirect if scheme is explicitly http
        let scheme = uri.scheme_str().unwrap_or("http");
        if scheme == "http" {
            let authority = host
                .or_else(|| uri.authority().map(|a| a.as_str()))
                .unwrap_or("localhost");
            let path_and_query = uri
                .path_and_query()
                .map(|pq| pq.as_str())
                .unwrap_or("/");
            Some(format!("https://{}{}", authority, path_and_query))
        } else {
            None
        }
    }

    /// Normalize trailing slash and return redirect target if changed
    fn normalize_trailing_slash(&self, path: &str) -> Option<String> {
        match self.trailing_slash {
            TrailingSlash::Ignore => None,
            TrailingSlash::Add => {
                if path == "/" || path.ends_with('/') {
                    None
                } else {
                    Some(format!("{}/", path))
                }
            }
            TrailingSlash::Strip => {
                if path == "/" || !path.ends_with('/') {
                    None
                } else {
                    Some(path.trim_end_matches('/').to_string())
                }
            }
        }
    }

    /// Try to match a redirect rule against the path
    fn match_rule(&self, path: &str, query: Option<&str>) -> Option<(String, StatusCode)> {
        for rule in &self.compiled_rules {
            if let Some(caps) = rule.pattern.captures(path) {
                let mut target = rule.to.clone();
                // Replace capture groups ($1, $2, etc.)
                for i in 1..caps.len() {
                    if let Some(m) = caps.get(i) {
                        target = target.replace(&format!("${}", i), m.as_str());
                    }
                }
                // Preserve query string if configured
                if rule.preserve_query {
                    if let Some(q) = query {
                        if !q.is_empty() {
                            if target.contains('?') {
                                target = format!("{}&{}", target, q);
                            } else {
                                target = format!("{}?{}", target, q);
                            }
                        }
                    }
                }
                return Some((target, rule.status));
            }
        }
        None
    }
}

impl fmt::Debug for Redirect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Redirect")
            .field("rules", &self.compiled_rules.len())
            .field("https_redirect", &self.https_redirect)
            .field("trailing_slash", &self.trailing_slash)
            .finish()
    }
}

#[async_trait]
impl Middleware for Redirect {
    async fn call(&self, req: Request<Body>, next: Next) -> Result<Response<Body>> {
        let uri = req.uri().clone();
        let host = req
            .headers()
            .get(header::HOST)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        // 1. HTTPS redirect
        if let Some(location) = self.check_https_redirect(&uri, host.as_deref()) {
            return Self::redirect_response(&location, StatusCode::MOVED_PERMANENTLY);
        }

        let path = uri.path();
        let query = uri.query();

        // 2. Redirect rules (first match wins)
        if let Some((location, status)) = self.match_rule(path, query) {
            return Self::redirect_response(&location, status);
        }

        // 3. Trailing slash normalization
        if let Some(new_path) = self.normalize_trailing_slash(path) {
            let location = if let Some(q) = query {
                format!("{}?{}", new_path, q)
            } else {
                new_path
            };
            return Self::redirect_response(&location, StatusCode::MOVED_PERMANENTLY);
        }

        // No redirect, pass through
        next.run(req).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::StatusCode;

    #[derive(Debug)]
    struct PassthroughHandler;

    #[async_trait]
    impl Middleware for PassthroughHandler {
        async fn call(&self, _req: Request<Body>, _next: Next) -> Result<Response<Body>> {
            Response::builder()
                .status(StatusCode::OK)
                .body(Full::new(Bytes::from("passthrough")))
                .map_err(|e| Error::Internal(e.to_string()))
        }
    }

    fn make_stack(
        mw: Redirect,
    ) -> std::sync::Arc<[std::sync::Arc<dyn Middleware>]> {
        std::sync::Arc::new([
            std::sync::Arc::new(mw) as std::sync::Arc<dyn Middleware>,
            std::sync::Arc::new(PassthroughHandler),
        ])
    }

    #[tokio::test]
    async fn test_https_redirect() {
        let config = RedirectConfig {
            https_redirect: true,
            ..Default::default()
        };
        let mw = Redirect::new(config).unwrap();
        let stack = make_stack(mw);
        let next = Next::new(stack);

        let req = Request::builder()
            .uri("http://example.com/path?q=1")
            .header(header::HOST, "example.com")
            .body(Body::from(""))
            .unwrap();

        let resp = next.run(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::MOVED_PERMANENTLY);
        assert_eq!(
            resp.headers()
                .get(header::LOCATION)
                .unwrap()
                .to_str()
                .unwrap(),
            "https://example.com/path?q=1"
        );
    }

    #[tokio::test]
    async fn test_regex_redirect_with_captures() {
        let config = RedirectConfig {
            rules: vec![RedirectRule {
                from: r"^/old/(.+)/(.+)$".to_string(),
                to: "/new/$1/$2".to_string(),
                status: 301,
                preserve_query: false,
            }],
            ..Default::default()
        };
        let mw = Redirect::new(config).unwrap();
        let stack = make_stack(mw);
        let next = Next::new(stack);

        let req = Request::builder()
            .uri("/old/foo/bar")
            .body(Body::from(""))
            .unwrap();

        let resp = next.run(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::MOVED_PERMANENTLY);
        assert_eq!(
            resp.headers()
                .get(header::LOCATION)
                .unwrap()
                .to_str()
                .unwrap(),
            "/new/foo/bar"
        );
    }

    #[tokio::test]
    async fn test_trailing_slash_add() {
        let config = RedirectConfig {
            trailing_slash: TrailingSlash::Add,
            ..Default::default()
        };
        let mw = Redirect::new(config).unwrap();
        let stack = make_stack(mw);
        let next = Next::new(stack);

        let req = Request::builder()
            .uri("/no-slash")
            .body(Body::from(""))
            .unwrap();

        let resp = next.run(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::MOVED_PERMANENTLY);
        assert_eq!(
            resp.headers()
                .get(header::LOCATION)
                .unwrap()
                .to_str()
                .unwrap(),
            "/no-slash/"
        );
    }

    #[tokio::test]
    async fn test_trailing_slash_strip() {
        let config = RedirectConfig {
            trailing_slash: TrailingSlash::Strip,
            ..Default::default()
        };
        let mw = Redirect::new(config).unwrap();
        let stack = make_stack(mw);
        let next = Next::new(stack);

        let req = Request::builder()
            .uri("/has-slash/")
            .body(Body::from(""))
            .unwrap();

        let resp = next.run(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::MOVED_PERMANENTLY);
        assert_eq!(
            resp.headers()
                .get(header::LOCATION)
                .unwrap()
                .to_str()
                .unwrap(),
            "/has-slash"
        );
    }

    #[tokio::test]
    async fn test_trailing_slash_root_unchanged() {
        let config = RedirectConfig {
            trailing_slash: TrailingSlash::Strip,
            ..Default::default()
        };
        let mw = Redirect::new(config).unwrap();
        let stack = make_stack(mw);
        let next = Next::new(stack);

        let req = Request::builder()
            .uri("/")
            .body(Body::from(""))
            .unwrap();

        let resp = next.run(req).await.unwrap();
        // Root path should not be stripped
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_no_match_passes_through() {
        let config = RedirectConfig {
            rules: vec![RedirectRule {
                from: r"^/old$".to_string(),
                to: "/new".to_string(),
                status: 302,
                preserve_query: false,
            }],
            ..Default::default()
        };
        let mw = Redirect::new(config).unwrap();
        let stack = make_stack(mw);
        let next = Next::new(stack);

        let req = Request::builder()
            .uri("/something-else")
            .body(Body::from(""))
            .unwrap();

        let resp = next.run(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_preserve_query_string() {
        let config = RedirectConfig {
            rules: vec![RedirectRule {
                from: r"^/old$".to_string(),
                to: "/new".to_string(),
                status: 307,
                preserve_query: true,
            }],
            ..Default::default()
        };
        let mw = Redirect::new(config).unwrap();
        let stack = make_stack(mw);
        let next = Next::new(stack);

        let req = Request::builder()
            .uri("/old?foo=bar&baz=1")
            .body(Body::from(""))
            .unwrap();

        let resp = next.run(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::TEMPORARY_REDIRECT);
        assert_eq!(
            resp.headers()
                .get(header::LOCATION)
                .unwrap()
                .to_str()
                .unwrap(),
            "/new?foo=bar&baz=1"
        );
    }

    #[tokio::test]
    async fn test_invalid_regex_returns_error() {
        let config = RedirectConfig {
            rules: vec![RedirectRule {
                from: r"[invalid".to_string(),
                to: "/new".to_string(),
                status: 301,
                preserve_query: false,
            }],
            ..Default::default()
        };
        let result = Redirect::new(config);
        assert!(result.is_err());
    }
}
