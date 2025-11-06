//! # Example Logging Plugin
//!
//! Demonstrates how to build a logging plugin using both RequestInterceptor
//! and ResponseInterceptor traits.
//!
//! ## Features
//!
//! - Structured request/response logging
//! - Configurable log levels
//! - Selective field logging
//! - Performance metrics
//!
//! ## Example
//!
//! ```rust,no_run
//! use example_logging::RequestLoggerPlugin;
//! use octopus_plugin_api::prelude::*;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let mut plugin = RequestLoggerPlugin::new();
//! plugin.init(serde_json::json!({
//!     "log_headers": true,
//!     "log_body": false
//! })).await?;
//! # Ok(())
//! # }
//! ```

use async_trait::async_trait;
use bytes::Bytes;
use http::{Request, Response};
use http_body_util::Full;
use octopus_plugin_api::prelude::*;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

/// Request Logger Plugin
///
/// Logs incoming requests and outgoing responses with configurable detail levels.
#[derive(Debug)]
pub struct RequestLoggerPlugin {
    config: LoggerConfig,
}

/// Plugin configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggerConfig {
    /// Whether to log request headers
    #[serde(default = "default_log_headers")]
    pub log_headers: bool,

    /// Whether to log request/response body
    #[serde(default)]
    pub log_body: bool,

    /// Whether to log query parameters
    #[serde(default = "default_log_query")]
    pub log_query: bool,

    /// Whether to log response headers
    #[serde(default = "default_log_response_headers")]
    pub log_response_headers: bool,

    /// Paths to exclude from logging
    #[serde(default)]
    pub exclude_paths: Vec<String>,

    /// Maximum body size to log (bytes)
    #[serde(default = "default_max_body_size")]
    pub max_body_size: usize,
}

fn default_log_headers() -> bool {
    true
}

fn default_log_query() -> bool {
    true
}

fn default_log_response_headers() -> bool {
    false
}

fn default_max_body_size() -> usize {
    1024 // 1 KB
}

impl Default for LoggerConfig {
    fn default() -> Self {
        Self {
            log_headers: true,
            log_body: false,
            log_query: true,
            log_response_headers: false,
            exclude_paths: vec![],
            max_body_size: 1024,
        }
    }
}

impl RequestLoggerPlugin {
    /// Create a new request logger plugin
    pub fn new() -> Self {
        Self {
            config: LoggerConfig::default(),
        }
    }

    /// Check if a path should be excluded from logging
    fn should_exclude(&self, path: &str) -> bool {
        self.config.exclude_paths.iter().any(|exclude| {
            if exclude.ends_with('*') {
                path.starts_with(exclude.trim_end_matches('*'))
            } else {
                path == exclude
            }
        })
    }
}

impl Default for RequestLoggerPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for RequestLoggerPlugin {
    fn name(&self) -> &str {
        "request-logger"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn description(&self) -> &str {
        "HTTP request/response logging plugin"
    }

    fn author(&self) -> &str {
        "Octopus Team"
    }

    async fn init(&mut self, config: serde_json::Value) -> Result<(), PluginError> {
        self.config = serde_json::from_value(config).map_err(|e| {
            PluginError::config(format!("Invalid configuration: {}", e))
        })?;

        debug!(
            log_headers = self.config.log_headers,
            log_body = self.config.log_body,
            exclude_paths = ?self.config.exclude_paths,
            "Request logger plugin initialized"
        );

        Ok(())
    }

    async fn start(&mut self) -> Result<(), PluginError> {
        debug!("Request logger plugin started");
        Ok(())
    }

    async fn stop(&mut self) -> Result<(), PluginError> {
        debug!("Request logger plugin stopped");
        Ok(())
    }
}

#[async_trait]
impl RequestInterceptor for RequestLoggerPlugin {
    async fn intercept_request(
        &self,
        req: &mut Request<Full<Bytes>>,
        ctx: &RequestContext,
    ) -> Result<InterceptorAction, PluginError> {
        let path = req.uri().path();

        // Skip logging for excluded paths
        if self.should_exclude(path) {
            return Ok(InterceptorAction::Continue);
        }

        let method = req.method();
        let uri = req.uri();

        // Build log message with configurable fields
        let mut log_fields = vec![
            ("request_id", ctx.request_id.clone()),
            ("method", method.to_string()),
            ("path", path.to_string()),
        ];

        // Add query string
        if self.config.log_query {
            if let Some(query) = uri.query() {
                log_fields.push(("query", query.to_string()));
            }
        }

        // Add headers
        if self.config.log_headers {
            let headers: Vec<String> = req
                .headers()
                .iter()
                .map(|(name, value)| {
                    format!(
                        "{}={}",
                        name.as_str(),
                        value.to_str().unwrap_or("<invalid>")
                    )
                })
                .collect();
            log_fields.push(("headers", headers.join(", ")));
        }

        // Log request
        info!(
            request_id = %ctx.request_id,
            method = %method,
            path = %path,
            remote_addr = %ctx.remote_addr,
            "Incoming request"
        );

        Ok(InterceptorAction::Continue)
    }
}

#[async_trait]
impl ResponseInterceptor for RequestLoggerPlugin {
    async fn intercept_response(
        &self,
        res: &mut Response<Full<Bytes>>,
        ctx: &ResponseContext,
    ) -> Result<InterceptorAction, PluginError> {
        let status = res.status();
        let duration_ms = ctx.duration.as_millis();

        // Log response with appropriate level
        if status.is_success() {
            info!(
                request_id = %ctx.request_id,
                status = status.as_u16(),
                duration_ms = duration_ms,
                "Request completed successfully"
            );
        } else if status.is_client_error() {
            warn!(
                request_id = %ctx.request_id,
                status = status.as_u16(),
                duration_ms = duration_ms,
                "Request completed with client error"
            );
        } else if status.is_server_error() {
            warn!(
                request_id = %ctx.request_id,
                status = status.as_u16(),
                duration_ms = duration_ms,
                "Request completed with server error"
            );
        } else {
            debug!(
                request_id = %ctx.request_id,
                status = status.as_u16(),
                duration_ms = duration_ms,
                "Request completed"
            );
        }

        // Log response headers if configured
        if self.config.log_response_headers {
            let headers: Vec<String> = res
                .headers()
                .iter()
                .map(|(name, value)| {
                    format!(
                        "{}={}",
                        name.as_str(),
                        value.to_str().unwrap_or("<invalid>")
                    )
                })
                .collect();
            debug!(
                request_id = %ctx.request_id,
                headers = %headers.join(", "),
                "Response headers"
            );
        }

        Ok(InterceptorAction::Continue)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_plugin_init() {
        let mut plugin = RequestLoggerPlugin::new();

        let config = serde_json::json!({
            "log_headers": true,
            "log_body": false,
            "exclude_paths": ["/health"]
        });

        plugin.init(config).await.unwrap();
        assert!(plugin.config.log_headers);
        assert!(!plugin.config.log_body);
        assert_eq!(plugin.config.exclude_paths, vec!["/health"]);
    }

    #[test]
    fn test_should_exclude() {
        let plugin = RequestLoggerPlugin {
            config: LoggerConfig {
                exclude_paths: vec!["/health".to_string(), "/metrics/*".to_string()],
                ..Default::default()
            },
        };

        assert!(plugin.should_exclude("/health"));
        assert!(plugin.should_exclude("/metrics/foo"));
        assert!(plugin.should_exclude("/metrics/bar/baz"));
        assert!(!plugin.should_exclude("/api/users"));
    }

    #[tokio::test]
    async fn test_request_interceptor() {
        let mut plugin = RequestLoggerPlugin::new();
        plugin.init(serde_json::json!({})).await.unwrap();

        let mut req = Request::builder()
            .uri("/test")
            .body(Full::new(Bytes::new()))
            .unwrap();

        let ctx = RequestContext::new(
            "req-123".to_string(),
            "127.0.0.1:8080".parse().unwrap(),
        );

        let result = plugin.intercept_request(&mut req, &ctx).await.unwrap();
        assert!(result.is_continue());
    }
}

