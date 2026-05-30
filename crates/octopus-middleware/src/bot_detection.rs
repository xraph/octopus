//! Bot detection/blocking middleware based on User-Agent patterns

use async_trait::async_trait;
use bytes::Bytes;
use http::{Request, Response, StatusCode};
use http_body_util::Full;
use octopus_core::{Middleware, Next, Result};
use regex::Regex;
use std::fmt;

/// Body type alias
pub type Body = Full<Bytes>;

/// Bot detection mode
#[derive(Debug, Clone)]
pub enum BotMode {
    /// Block matching bots (403)
    Block,
    /// Only allow matching patterns (block everything else)
    Allow,
    /// Log but don't block
    LogOnly,
}

/// Bot detection configuration
#[derive(Debug, Clone)]
pub struct BotDetectionConfig {
    /// Detection mode
    pub mode: BotMode,
    /// Regex patterns to block
    pub block_patterns: Vec<String>,
    /// Always allow (override block)
    pub allow_patterns: Vec<String>,
    /// Block empty User-Agent (default: false)
    pub block_empty_ua: bool,
    /// HTTP status code for blocked requests (default: 403)
    pub response_status: u16,
    /// Response body message for blocked requests (default: "Forbidden")
    pub response_message: String,
}

impl Default for BotDetectionConfig {
    fn default() -> Self {
        Self {
            mode: BotMode::Block,
            block_patterns: Vec::new(),
            allow_patterns: Vec::new(),
            block_empty_ua: false,
            response_status: 403,
            response_message: "Forbidden".to_string(),
        }
    }
}

impl BotDetectionConfig {
    /// Create a config pre-populated with common bot User-Agent patterns
    pub fn with_common_bots() -> Self {
        Self {
            mode: BotMode::Block,
            block_patterns: vec![
                r"(?i)(bot|crawler|spider|scraper|curl|wget|python-requests|go-http-client|java/|libwww)"
                    .to_string(),
            ],
            allow_patterns: Vec::new(),
            block_empty_ua: false,
            response_status: 403,
            response_message: "Forbidden".to_string(),
        }
    }
}

/// Bot detection middleware
///
/// Inspects the User-Agent header and blocks, allows, or logs
/// requests based on configurable regex patterns.
#[derive(Clone)]
pub struct BotDetection {
    config: BotDetectionConfig,
    block_regexes: Vec<Regex>,
    allow_regexes: Vec<Regex>,
}

impl BotDetection {
    /// Create a new BotDetection middleware with the given config.
    /// Pre-compiles all regex patterns.
    pub fn new(config: BotDetectionConfig) -> Self {
        let block_regexes = config
            .block_patterns
            .iter()
            .filter_map(|p| Regex::new(p).ok())
            .collect();
        let allow_regexes = config
            .allow_patterns
            .iter()
            .filter_map(|p| Regex::new(p).ok())
            .collect();
        Self {
            config,
            block_regexes,
            allow_regexes,
        }
    }

    /// Create a bot detection middleware using common bot patterns
    pub fn common() -> Self {
        Self::new(BotDetectionConfig::with_common_bots())
    }

    /// Build the blocked response
    fn blocked_response(&self) -> Response<Body> {
        let status =
            StatusCode::from_u16(self.config.response_status).unwrap_or(StatusCode::FORBIDDEN);
        Response::builder()
            .status(status)
            .header("Content-Type", "text/plain")
            .body(Full::new(Bytes::from(self.config.response_message.clone())))
            .expect("Failed to build bot detection response")
    }

    /// Check if user agent matches any allow pattern
    fn matches_allow(&self, ua: &str) -> bool {
        self.allow_regexes.iter().any(|r| r.is_match(ua))
    }

    /// Check if user agent matches any block pattern
    fn matches_block(&self, ua: &str) -> bool {
        self.block_regexes.iter().any(|r| r.is_match(ua))
    }
}

impl fmt::Debug for BotDetection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BotDetection")
            .field("mode", &self.config.mode)
            .field("block_patterns", &self.config.block_patterns)
            .field("allow_patterns", &self.config.allow_patterns)
            .field("block_empty_ua", &self.config.block_empty_ua)
            .finish()
    }
}

#[async_trait]
impl Middleware for BotDetection {
    async fn call(&self, req: Request<Body>, next: Next) -> Result<Response<Body>> {
        let ua = req
            .headers()
            .get(http::header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        // Block empty User-Agent if configured
        if ua.is_empty() && self.config.block_empty_ua {
            tracing::warn!(uri = %req.uri(), "Blocked request with empty User-Agent");
            return Ok(self.blocked_response());
        }

        // Allow patterns always take priority
        if !ua.is_empty() && self.matches_allow(ua) {
            return next.run(req).await;
        }

        match self.config.mode {
            BotMode::Block => {
                if !ua.is_empty() && self.matches_block(ua) {
                    tracing::warn!(uri = %req.uri(), user_agent = %ua, "Blocked bot request");
                    return Ok(self.blocked_response());
                }
            }
            BotMode::Allow => {
                // In Allow mode, if no allow pattern matched (checked above), block
                if !ua.is_empty() && !self.matches_allow(ua) {
                    tracing::warn!(uri = %req.uri(), user_agent = %ua, "Blocked non-allowed User-Agent");
                    return Ok(self.blocked_response());
                }
            }
            BotMode::LogOnly => {
                if !ua.is_empty() && self.matches_block(ua) {
                    tracing::info!(uri = %req.uri(), user_agent = %ua, "Detected bot request (log only)");
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

    fn make_stack(bot: BotDetection) -> Arc<[Arc<dyn Middleware>]> {
        Arc::new([
            Arc::new(bot) as Arc<dyn Middleware>,
            Arc::new(TestHandler) as Arc<dyn Middleware>,
        ])
    }

    #[tokio::test]
    async fn test_block_bot_user_agent() {
        let bot = BotDetection::common();
        let stack = make_stack(bot);

        let next = Next::new(stack);
        let req = Request::builder()
            .uri("/test")
            .header("User-Agent", "Mozilla/5.0 (compatible; Googlebot/2.1)")
            .body(Body::from(""))
            .unwrap();

        let response = next.run(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_allow_legitimate_user_agent() {
        let bot = BotDetection::common();
        let stack = make_stack(bot);

        let next = Next::new(stack);
        let req = Request::builder()
            .uri("/test")
            .header(
                "User-Agent",
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
            )
            .body(Body::from(""))
            .unwrap();

        let response = next.run(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_block_empty_user_agent() {
        let config = BotDetectionConfig {
            block_empty_ua: true,
            ..BotDetectionConfig::with_common_bots()
        };
        let bot = BotDetection::new(config);
        let stack = make_stack(bot);

        let next = Next::new(stack);
        let req = Request::builder()
            .uri("/test")
            .body(Body::from(""))
            .unwrap();

        let response = next.run(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_allow_pattern_overrides_block() {
        let config = BotDetectionConfig {
            mode: BotMode::Block,
            block_patterns: vec![r"(?i)bot".to_string()],
            allow_patterns: vec![r"(?i)goodbot".to_string()],
            ..Default::default()
        };
        let bot = BotDetection::new(config);
        let stack = make_stack(bot);

        let next = Next::new(stack);
        let req = Request::builder()
            .uri("/test")
            .header("User-Agent", "GoodBot/1.0")
            .body(Body::from(""))
            .unwrap();

        let response = next.run(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_allow_mode_blocks_unmatched() {
        let config = BotDetectionConfig {
            mode: BotMode::Allow,
            allow_patterns: vec![r"(?i)myapp".to_string()],
            ..Default::default()
        };
        let bot = BotDetection::new(config);
        let stack = make_stack(bot);

        let next = Next::new(stack);
        let req = Request::builder()
            .uri("/test")
            .header("User-Agent", "RandomClient/1.0")
            .body(Body::from(""))
            .unwrap();

        let response = next.run(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_log_only_mode_passes_through() {
        let config = BotDetectionConfig {
            mode: BotMode::LogOnly,
            block_patterns: vec![r"(?i)bot".to_string()],
            ..Default::default()
        };
        let bot = BotDetection::new(config);
        let stack = make_stack(bot);

        let next = Next::new(stack);
        let req = Request::builder()
            .uri("/test")
            .header("User-Agent", "SomeBot/1.0")
            .body(Body::from(""))
            .unwrap();

        let response = next.run(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}
