//! # Example Transform Plugin
//!
//! Demonstrates how to build a transform plugin for request/response manipulation.
//!
//! ## Features
//!
//! - Add, remove, rename headers
//! - Path rewriting with regex
//! - Query parameter manipulation
//! - Conditional transformations
//!
//! ## Example
//!
//! ```rust,no_run
//! use example_transform::HeaderTransformPlugin;
//! use octopus_plugin_api::prelude::*;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let mut plugin = HeaderTransformPlugin::new();
//! plugin.init(serde_json::json!({
//!     "add_headers": {
//!         "X-Custom-Header": "value"
//!     },
//!     "remove_headers": ["X-Unwanted"]
//! })).await?;
//! # Ok(())
//! # }
//! ```

use async_trait::async_trait;
use bytes::Bytes;
use http::{Request, Response};
use http_body_util::Full;
use octopus_plugin_api::prelude::*;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::debug;

/// Header Transform Plugin
///
/// Transforms requests and responses by manipulating headers,
/// rewriting paths, and modifying query parameters.
#[derive(Debug)]
pub struct HeaderTransformPlugin {
    config: TransformConfig,
    path_regex: Option<Regex>,
}

impl HeaderTransformPlugin {
    /// Create a new transform plugin
    pub fn new() -> Self {
        Self {
            config: TransformConfig::default(),
            path_regex: None,
        }
    }
}

impl Default for HeaderTransformPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for HeaderTransformPlugin {
    fn name(&self) -> &str {
        "header-transform"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn description(&self) -> &str {
        "HTTP header and path transformation plugin"
    }

    fn author(&self) -> &str {
        "Octopus Team"
    }

    async fn init(&mut self, config: serde_json::Value) -> Result<(), PluginError> {
        self.config = serde_json::from_value(config).map_err(|e| {
            PluginError::config(format!("Invalid configuration: {}", e))
        })?;

        // Compile path regex if provided
        if let Some(ref rewrite) = self.config.rewrite_path {
            self.path_regex = Some(
                Regex::new(&rewrite.pattern).map_err(|e| {
                    PluginError::config(format!("Invalid regex pattern: {}", e))
                })?,
            );
        }

        debug!(
            add_headers = ?self.config.add_headers.keys().collect::<Vec<_>>(),
            remove_headers = ?self.config.remove_headers,
            "Transform plugin initialized"
        );

        Ok(())
    }

    async fn start(&mut self) -> Result<(), PluginError> {
        debug!("Transform plugin started");
        Ok(())
    }

    async fn stop(&mut self) -> Result<(), PluginError> {
        debug!("Transform plugin stopped");
        Ok(())
    }
}

#[async_trait]
impl TransformPlugin for HeaderTransformPlugin {
    async fn transform_request(
        &self,
        req: &mut Request<Full<Bytes>>,
        _config: &TransformConfig,
    ) -> Result<(), PluginError> {
        // Add headers
        for (name, value) in &self.config.add_headers {
            let header_name: http::HeaderName = name.parse().map_err(|e| {
                PluginError::transform(format!("Invalid header name: {}", e))
            })?;
            let header_value: http::HeaderValue = value.parse().map_err(|e| {
                PluginError::transform(format!("Invalid header value: {}", e))
            })?;
            req.headers_mut().insert(header_name, header_value);
        }

        // Remove headers
        for name in &self.config.remove_headers {
            req.headers_mut().remove(name);
        }

        // Rename headers
        for (old_name, new_name) in &self.config.rename_headers {
            if let Some(value) = req.headers_mut().remove(old_name) {
                let header_name: http::HeaderName = new_name.parse().map_err(|e| {
                    PluginError::transform(format!("Invalid header name: {}", e))
                })?;
                req.headers_mut().insert(header_name, value);
            }
        }

        // Rewrite path
        if let (Some(regex), Some(ref rewrite)) = (&self.path_regex, &self.config.rewrite_path) {
            let path = req.uri().path().to_string();
            let new_path = regex.replace(&path, &rewrite.replacement);

            if new_path != path {
                let old_path = path.clone();
                let mut parts = req.uri().clone().into_parts();
                parts.path_and_query = Some(
                    new_path
                        .parse()
                        .map_err(|e| PluginError::transform(format!("Invalid path: {}", e)))?,
                );

                *req.uri_mut() = http::Uri::from_parts(parts)
                    .map_err(|e| PluginError::transform(format!("Failed to build URI: {}", e)))?;

                debug!(old_path = %old_path, new_path = %new_path, "Path rewritten");
            }
        }

        Ok(())
    }

    async fn transform_response(
        &self,
        res: &mut Response<Full<Bytes>>,
        _config: &TransformConfig,
    ) -> Result<(), PluginError> {
        // Add response headers
        for (name, value) in &self.config.add_headers {
            let header_name: http::HeaderName = name.parse().map_err(|e| {
                PluginError::transform(format!("Invalid header name: {}", e))
            })?;
            let header_value: http::HeaderValue = value.parse().map_err(|e| {
                PluginError::transform(format!("Invalid header value: {}", e))
            })?;
            res.headers_mut().insert(header_name, header_value);
        }

        // Remove response headers
        for name in &self.config.remove_headers {
            res.headers_mut().remove(name);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_plugin_init() {
        let mut plugin = HeaderTransformPlugin::new();

        let config = serde_json::json!({
            "add_headers": {
                "X-Custom": "value"
            },
            "remove_headers": ["X-Remove"]
        });

        plugin.init(config).await.unwrap();
        assert_eq!(plugin.config.add_headers.get("X-Custom"), Some(&"value".to_string()));
        assert_eq!(plugin.config.remove_headers, vec!["X-Remove"]);
    }

    #[tokio::test]
    async fn test_add_headers() {
        let mut plugin = HeaderTransformPlugin::new();
        plugin.init(serde_json::json!({
            "add_headers": {
                "X-Test": "test-value"
            }
        })).await.unwrap();

        let mut req = Request::builder()
            .uri("/test")
            .body(Full::new(Bytes::new()))
            .unwrap();

        plugin.transform_request(&mut req, &plugin.config.clone()).await.unwrap();

        assert_eq!(
            req.headers().get("X-Test").unwrap(),
            "test-value"
        );
    }

    #[tokio::test]
    async fn test_remove_headers() {
        let mut plugin = HeaderTransformPlugin::new();
        plugin.init(serde_json::json!({
            "remove_headers": ["X-Remove"]
        })).await.unwrap();

        let mut req = Request::builder()
            .uri("/test")
            .header("X-Remove", "value")
            .header("X-Keep", "value")
            .body(Full::new(Bytes::new()))
            .unwrap();

        plugin.transform_request(&mut req, &plugin.config.clone()).await.unwrap();

        assert!(req.headers().get("X-Remove").is_none());
        assert!(req.headers().get("X-Keep").is_some());
    }

    #[tokio::test]
    async fn test_path_rewrite() {
        let mut plugin = HeaderTransformPlugin::new();
        plugin.init(serde_json::json!({
            "rewrite_path": {
                "pattern": "^/old",
                "replacement": "/new"
            }
        })).await.unwrap();

        let mut req = Request::builder()
            .uri("/old/path")
            .body(Full::new(Bytes::new()))
            .unwrap();

        plugin.transform_request(&mut req, &plugin.config.clone()).await.unwrap();

        assert_eq!(req.uri().path(), "/new/path");
    }
}

