//! Web Application Firewall (WAF) middleware
//!
//! Detects and blocks common attack patterns including SQL injection and
//! cross-site scripting (XSS). Supports configurable blocking/logging modes,
//! custom rules, and path exclusions.

use async_trait::async_trait;
use bytes::Bytes;
use http::{Request, Response, StatusCode};
use http_body_util::Full;
use octopus_core::{Middleware, Next, Result};
use regex::Regex;
use std::fmt;
use tracing::warn;

/// Body type alias
pub type Body = Full<Bytes>;

/// WAF operating mode
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WafMode {
    /// Block malicious requests with an error response
    Block,
    /// Log detections but allow requests to continue
    LogOnly,
}

impl Default for WafMode {
    fn default() -> Self {
        Self::Block
    }
}

/// What parts of the request to inspect
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WafTarget {
    /// Only inspect URL query string parameters
    QueryString,
    /// Only inspect header values
    Headers,
    /// Only inspect request body
    Body,
    /// Inspect all targets (query string, headers, and body)
    All,
}

impl Default for WafTarget {
    fn default() -> Self {
        Self::All
    }
}

/// A custom WAF rule with a named regex pattern
#[derive(Debug, Clone)]
pub struct WafRule {
    /// Human-readable rule name (used in log/error messages)
    pub name: String,
    /// Regex pattern string
    pub pattern: String,
    /// Which part of the request this rule inspects
    pub target: WafTarget,
}

/// WAF configuration
#[derive(Debug, Clone)]
pub struct WafConfig {
    /// Operating mode (block or log-only)
    pub mode: WafMode,
    /// Enable built-in SQL injection detection
    pub sql_injection: bool,
    /// Enable built-in XSS detection
    pub xss: bool,
    /// Which request targets to inspect
    pub inspect: WafTarget,
    /// Additional custom rules
    pub custom_rules: Vec<WafRule>,
    /// URL path prefixes to skip (no inspection)
    pub exclusions: Vec<String>,
    /// HTTP status code returned when blocking (default: 403)
    pub response_status: u16,
}

impl Default for WafConfig {
    fn default() -> Self {
        Self {
            mode: WafMode::Block,
            sql_injection: true,
            xss: true,
            inspect: WafTarget::All,
            custom_rules: Vec::new(),
            exclusions: Vec::new(),
            response_status: 403,
        }
    }
}

/// A compiled rule ready for matching
struct CompiledRule {
    name: String,
    regex: Regex,
    target: WafTarget,
}

impl fmt::Debug for CompiledRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CompiledRule")
            .field("name", &self.name)
            .field("target", &self.target)
            .finish()
    }
}

/// Web Application Firewall middleware
///
/// Inspects incoming requests for SQL injection and XSS attack patterns.
/// All regex patterns are pre-compiled at construction time for performance.
pub struct Waf {
    config: WafConfig,
    rules: Vec<CompiledRule>,
}

impl Waf {
    /// Create a new WAF middleware with default config
    pub fn new() -> Self {
        Self::with_config(WafConfig::default())
    }

    /// Create a new WAF middleware with custom config
    pub fn with_config(config: WafConfig) -> Self {
        let mut rules = Vec::new();

        if config.sql_injection {
            for (name, pattern) in Self::sqli_patterns() {
                if let Ok(regex) = Regex::new(pattern) {
                    rules.push(CompiledRule {
                        name: name.to_string(),
                        regex,
                        target: config.inspect.clone(),
                    });
                }
            }
        }

        if config.xss {
            for (name, pattern) in Self::xss_patterns() {
                if let Ok(regex) = Regex::new(pattern) {
                    rules.push(CompiledRule {
                        name: name.to_string(),
                        regex,
                        target: config.inspect.clone(),
                    });
                }
            }
        }

        for custom in &config.custom_rules {
            if let Ok(regex) = Regex::new(&custom.pattern) {
                rules.push(CompiledRule {
                    name: custom.name.clone(),
                    regex,
                    target: custom.target.clone(),
                });
            } else {
                warn!(
                    rule = %custom.name,
                    pattern = %custom.pattern,
                    "Failed to compile custom WAF rule pattern, skipping"
                );
            }
        }

        Self { config, rules }
    }

    /// Built-in SQL injection detection patterns
    fn sqli_patterns() -> Vec<(&'static str, &'static str)> {
        vec![
            ("sqli-union-select", r"(?i)(\bunion\b\s+\bselect\b)"),
            ("sqli-or-1-eq-1", r"(?i)(\bor\b\s+1\s*=\s*1)"),
            ("sqli-and-1-eq-1", r"(?i)(\band\b\s+1\s*=\s*1)"),
            (
                "sqli-dangerous-stmts",
                r"(?i)(;\s*(drop|delete|insert|update|alter)\b)",
            ),
            ("sqli-comment-injection", r"(?i)(--\s*$|/\*[\s\S]*?\*/)"),
            (
                "sqli-exec-declare",
                r"(?i)(\bexec\b|\bexecute\b|\bdeclare\b)",
            ),
            (
                "sqli-quote-or-and",
                r"(?i)('|\%27)(\s|\+)*(or|and)(\s|\+)*('|\%27)?",
            ),
            (
                "sqli-time-based",
                r"(?i)(sleep\s*\(|benchmark\s*\(|waitfor\s+delay)",
            ),
            ("sqli-hex-encoding", r"(?i)(0x[0-9a-f]+)"),
            (
                "sqli-info-schema",
                r"(?i)(information_schema|sysobjects|syscolumns)",
            ),
            ("sqli-char-function", r"(?i)(char\s*\(\s*\d+)"),
            ("sqli-concat-function", r"(?i)(concat\s*\()"),
            (
                "sqli-load-outfile",
                r"(?i)(\bload_file\b|\binto\s+outfile\b|\binto\s+dumpfile\b)",
            ),
            ("sqli-having-clause", r"(?i)(\bhaving\b\s+\d+\s*[=<>])"),
            (
                "sqli-group-by-injection",
                r"(?i)(\bgroup\s+by\b.+\bhaving\b)",
            ),
            ("sqli-order-by-injection", r"(?i)(\border\s+by\b\s+\d+)"),
            (
                "sqli-stacked-queries",
                r"(?i)(;\s*(select|union|insert|update|delete|drop|alter|create)\b)",
            ),
            ("sqli-boolean-based", r"(?i)(\band\b\s+\d+\s*[=<>]\s*\d+)"),
            (
                "sqli-extractvalue",
                r"(?i)(extractvalue\s*\(|updatexml\s*\()",
            ),
            (
                "sqli-substr-function",
                r"(?i)(substr\s*\(|substring\s*\(|mid\s*\()",
            ),
            ("sqli-ascii-function", r"(?i)(ascii\s*\(|ord\s*\()"),
            ("sqli-if-function", r"(?i)(\bif\s*\(\s*\d)"),
            ("sqli-case-when", r"(?i)(\bcase\s+when\b.*\bthen\b)"),
            ("sqli-convert-cast", r"(?i)(\bconvert\s*\(|\bcast\s*\()"),
            (
                "sqli-pg-sleep",
                r"(?i)(pg_sleep\s*\(|dbms_pipe\.receive_message)",
            ),
        ]
    }

    /// Built-in XSS detection patterns
    fn xss_patterns() -> Vec<(&'static str, &'static str)> {
        vec![
            ("xss-script-tag", r"(?i)(<script[\s>])"),
            ("xss-event-handler", r"(?i)(on\w+\s*=)"),
            ("xss-javascript-proto", r"(?i)(javascript\s*:)"),
            ("xss-iframe-tag", r"(?i)(<iframe[\s>])"),
            ("xss-svg-event", r"(?i)(<svg[\s/].*?on\w+\s*=)"),
            ("xss-img-onerror", r"(?i)(<img[^>]+onerror\s*=)"),
            (
                "xss-document-access",
                r"(?i)(document\.(cookie|location|write))",
            ),
            (
                "xss-eval-settimeout",
                r"(?i)(eval\s*\(|setTimeout\s*\(|setInterval\s*\()",
            ),
            ("xss-object-tag", r"(?i)(<object[\s>])"),
            ("xss-embed-tag", r"(?i)(<embed[\s>])"),
            ("xss-form-tag", r"(?i)(<form[\s>])"),
            ("xss-base-tag", r"(?i)(<base[\s>])"),
            ("xss-vbscript-proto", r"(?i)(vbscript\s*:)"),
            ("xss-data-uri", r"(?i)(data\s*:\s*text/html)"),
            (
                "xss-expression-css",
                r"(?i)(expression\s*\(|url\s*\(\s*javascript)",
            ),
        ]
    }

    /// URL-decode a string (handles %XX sequences)
    fn url_decode(input: &str) -> String {
        let mut result = String::with_capacity(input.len());
        let mut chars = input.chars();
        while let Some(c) = chars.next() {
            if c == '%' {
                let hex: String = chars.by_ref().take(2).collect();
                if hex.len() == 2 {
                    if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                        result.push(byte as char);
                        continue;
                    }
                }
                // Not a valid escape, keep as-is
                result.push('%');
                result.push_str(&hex);
            } else if c == '+' {
                result.push(' ');
            } else {
                result.push(c);
            }
        }
        result
    }

    /// Check if a path should be excluded from inspection
    fn is_excluded(&self, path: &str) -> bool {
        self.config
            .exclusions
            .iter()
            .any(|excl| path.starts_with(excl))
    }

    /// Check a single string payload against all rules that match the given target
    fn check_payload(&self, payload: &str, target: &WafTarget) -> Option<&str> {
        let decoded = Self::url_decode(payload);
        for rule in &self.rules {
            if !target_matches(&rule.target, target) {
                continue;
            }
            if rule.regex.is_match(&decoded) {
                return Some(&rule.name);
            }
        }
        None
    }

    /// Inspect the query string for attacks
    fn check_query_string(&self, uri: &http::Uri) -> Option<&str> {
        if let Some(query) = uri.query() {
            // Check entire query string
            if let Some(rule) = self.check_payload(query, &WafTarget::QueryString) {
                return Some(rule);
            }
            // Also check individual parameter values
            for part in query.split('&') {
                if let Some((_key, value)) = part.split_once('=') {
                    if let Some(rule) = self.check_payload(value, &WafTarget::QueryString) {
                        return Some(rule);
                    }
                }
            }
        }
        None
    }

    /// Inspect header values for attacks
    fn check_headers(&self, headers: &http::HeaderMap) -> Option<String> {
        for (_name, value) in headers.iter() {
            if let Ok(v) = value.to_str() {
                if let Some(rule) = self.check_payload(v, &WafTarget::Headers) {
                    return Some(rule.to_string());
                }
            }
        }
        None
    }

    /// Inspect body content for attacks
    fn check_body(&self, body: &[u8]) -> Option<String> {
        // Try as UTF-8 text first
        if let Ok(text) = std::str::from_utf8(body) {
            if let Some(rule) = self.check_payload(text, &WafTarget::Body) {
                return Some(rule.to_string());
            }

            // If it looks like JSON, also inspect individual field values
            if text.trim_start().starts_with('{') || text.trim_start().starts_with('[') {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(text) {
                    if let Some(rule) = self.check_json_value(&json) {
                        return Some(rule);
                    }
                }
            }
        }
        None
    }

    /// Recursively inspect JSON values
    fn check_json_value(&self, value: &serde_json::Value) -> Option<String> {
        match value {
            serde_json::Value::String(s) => self
                .check_payload(s, &WafTarget::Body)
                .map(|r| r.to_string()),
            serde_json::Value::Object(map) => {
                for (_k, v) in map {
                    if let Some(rule) = self.check_json_value(v) {
                        return Some(rule);
                    }
                }
                None
            }
            serde_json::Value::Array(arr) => {
                for v in arr {
                    if let Some(rule) = self.check_json_value(v) {
                        return Some(rule);
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// Build a block response
    fn block_response(&self, rule_name: &str) -> Response<Body> {
        let status =
            StatusCode::from_u16(self.config.response_status).unwrap_or(StatusCode::FORBIDDEN);

        let body = serde_json::json!({
            "error": "request_blocked",
            "message": "Request blocked by WAF",
            "rule": rule_name
        });

        Response::builder()
            .status(status)
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(body.to_string())))
            .expect("Failed to build WAF block response")
    }
}

impl Default for Waf {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Waf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Waf")
            .field("mode", &self.config.mode)
            .field("sql_injection", &self.config.sql_injection)
            .field("xss", &self.config.xss)
            .field("rules_count", &self.rules.len())
            .field("exclusions", &self.config.exclusions)
            .finish()
    }
}

/// Check if a rule target is compatible with an inspection target
fn target_matches(rule_target: &WafTarget, inspection_target: &WafTarget) -> bool {
    matches!(
        (rule_target, inspection_target),
        (WafTarget::All, _)
            | (_, WafTarget::All)
            | (WafTarget::QueryString, WafTarget::QueryString)
            | (WafTarget::Headers, WafTarget::Headers)
            | (WafTarget::Body, WafTarget::Body)
    )
}

#[async_trait]
impl Middleware for Waf {
    async fn call(&self, req: Request<Body>, next: Next) -> Result<Response<Body>> {
        let path = req.uri().path().to_string();

        // Skip excluded paths
        if self.is_excluded(&path) {
            return next.run(req).await;
        }

        // Check query string
        if matches!(self.config.inspect, WafTarget::All | WafTarget::QueryString) {
            if let Some(rule_name) = self.check_query_string(req.uri()) {
                warn!(
                    rule = %rule_name,
                    path = %path,
                    target = "query_string",
                    "WAF detection: potential attack in query string"
                );
                if self.config.mode == WafMode::Block {
                    return Ok(self.block_response(rule_name));
                }
            }
        }

        // Check headers
        if matches!(self.config.inspect, WafTarget::All | WafTarget::Headers) {
            if let Some(rule_name) = self.check_headers(req.headers()) {
                warn!(
                    rule = %rule_name,
                    path = %path,
                    target = "headers",
                    "WAF detection: potential attack in headers"
                );
                if self.config.mode == WafMode::Block {
                    return Ok(self.block_response(&rule_name));
                }
            }
        }

        // Check body
        if matches!(self.config.inspect, WafTarget::All | WafTarget::Body) {
            // The body is Full<Bytes>, we can inspect its data
            let body_data = {
                let body_clone = req.body().clone();
                http_body_util::BodyExt::collect(body_clone)
                    .await
                    .map(|c| c.to_bytes())
                    .unwrap_or_default()
            };
            if !body_data.is_empty() {
                if let Some(rule_name) = self.check_body(&body_data) {
                    warn!(
                        rule = %rule_name,
                        path = %path,
                        target = "body",
                        "WAF detection: potential attack in request body"
                    );
                    if self.config.mode == WafMode::Block {
                        return Ok(self.block_response(&rule_name));
                    }
                }
            }
        }

        next.run(req).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use octopus_core::Error;
    use std::sync::Arc;

    #[derive(Debug)]
    struct TestHandler;

    #[async_trait]
    impl Middleware for TestHandler {
        async fn call(&self, _req: Request<Body>, _next: Next) -> Result<Response<Body>> {
            Response::builder()
                .status(StatusCode::OK)
                .body(Full::new(Bytes::from("success")))
                .map_err(|e| Error::Internal(e.to_string()))
        }
    }

    fn make_stack(waf: Waf) -> Arc<[Arc<dyn Middleware>]> {
        Arc::new([
            Arc::new(waf) as Arc<dyn Middleware>,
            Arc::new(TestHandler) as Arc<dyn Middleware>,
        ])
    }

    #[tokio::test]
    async fn test_sqli_blocked() {
        let waf = Waf::new();
        let stack = make_stack(waf);
        let next = Next::new(stack);

        // SQL injection in query string
        let req = Request::builder()
            .uri("/api/users?id=1%20OR%201=1")
            .body(Body::from(""))
            .unwrap();

        let resp = next.run(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_sqli_union_select_blocked() {
        let waf = Waf::new();
        let stack = make_stack(waf);
        let next = Next::new(stack);

        let req = Request::builder()
            .uri("/api/data?q=1+UNION+SELECT+username,password+FROM+users")
            .body(Body::from(""))
            .unwrap();

        let resp = next.run(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_xss_blocked() {
        let waf = Waf::new();
        let stack = make_stack(waf);
        let next = Next::new(stack);

        let req = Request::builder()
            .uri("/search?q=%3Cscript%3Ealert('xss')%3C/script%3E")
            .body(Body::from(""))
            .unwrap();

        let resp = next.run(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_xss_in_body_blocked() {
        let waf = Waf::new();
        let stack = make_stack(waf);
        let next = Next::new(stack);

        let body_json = r#"{"name": "<script>alert('xss')</script>"}"#;
        let req = Request::builder()
            .uri("/api/submit")
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(body_json)))
            .unwrap();

        let resp = next.run(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_clean_request_passes() {
        let waf = Waf::new();
        let stack = make_stack(waf);
        let next = Next::new(stack);

        let req = Request::builder()
            .uri("/api/users?page=1&limit=10")
            .body(Body::from(""))
            .unwrap();

        let resp = next.run(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_exclusion_skips_waf() {
        let config = WafConfig {
            exclusions: vec!["/health".to_string(), "/internal/".to_string()],
            ..Default::default()
        };
        let waf = Waf::with_config(config);
        let stack = make_stack(waf);
        let next = Next::new(stack);

        // This request has SQLi but the path is excluded
        let req = Request::builder()
            .uri("/health?id=1%20OR%201=1")
            .body(Body::from(""))
            .unwrap();

        let resp = next.run(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_log_only_mode_allows() {
        let config = WafConfig {
            mode: WafMode::LogOnly,
            ..Default::default()
        };
        let waf = Waf::with_config(config);
        let stack = make_stack(waf);
        let next = Next::new(stack);

        let req = Request::builder()
            .uri("/api/users?id=1%20OR%201=1")
            .body(Body::from(""))
            .unwrap();

        let resp = next.run(req).await.unwrap();
        // LogOnly mode: request should pass through even with SQLi
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_custom_rule() {
        let config = WafConfig {
            sql_injection: false,
            xss: false,
            custom_rules: vec![WafRule {
                name: "block-debug".to_string(),
                pattern: r"(?i)(debug=true)".to_string(),
                target: WafTarget::QueryString,
            }],
            ..Default::default()
        };
        let waf = Waf::with_config(config);
        let stack = make_stack(waf);
        let next = Next::new(stack);

        let req = Request::builder()
            .uri("/api/data?debug=true")
            .body(Body::from(""))
            .unwrap();

        let resp = next.run(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_url_decode_handling() {
        let waf = Waf::new();
        let stack = make_stack(waf);
        let next = Next::new(stack);

        // Double-encoded SQLi: %27 = '
        let req = Request::builder()
            .uri("/api?name=%27%20OR%20%271%27=%271")
            .body(Body::from(""))
            .unwrap();

        let resp = next.run(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}
