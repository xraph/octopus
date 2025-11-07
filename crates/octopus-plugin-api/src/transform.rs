//! Request/Response transformation plugin traits

use crate::error::Result;
use crate::interceptor::Body;
use crate::plugin::Plugin;
use async_trait::async_trait;
use http::{Request, Response};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Request/Response transformation plugin
///
/// Transforms requests before routing and responses before returning.
#[async_trait]
pub trait TransformPlugin: Plugin {
    /// Transform request
    ///
    /// Modify request according to configuration.
    async fn transform_request(
        &self,
        req: &mut Request<Body>,
        config: &TransformConfig,
    ) -> Result<()>;

    /// Transform response
    ///
    /// Modify response according to configuration.
    async fn transform_response(
        &self,
        res: &mut Response<Body>,
        config: &TransformConfig,
    ) -> Result<()>;
}

/// Transform configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformConfig {
    /// Headers to add
    #[serde(default)]
    pub add_headers: HashMap<String, String>,

    /// Headers to remove
    #[serde(default)]
    pub remove_headers: Vec<String>,

    /// Headers to rename (old -> new)
    #[serde(default)]
    pub rename_headers: HashMap<String, String>,

    /// Path rewrite pattern
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rewrite_path: Option<PathRewrite>,

    /// Body transformation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modify_body: Option<BodyTransform>,

    /// Query parameters to add
    #[serde(default)]
    pub add_query_params: HashMap<String, String>,

    /// Query parameters to remove
    #[serde(default)]
    pub remove_query_params: Vec<String>,
}

impl Default for TransformConfig {
    fn default() -> Self {
        Self {
            add_headers: HashMap::new(),
            remove_headers: Vec::new(),
            rename_headers: HashMap::new(),
            rewrite_path: None,
            modify_body: None,
            add_query_params: HashMap::new(),
            remove_query_params: Vec::new(),
        }
    }
}

impl TransformConfig {
    /// Create a new empty config
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a header
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.add_headers.insert(name.into(), value.into());
        self
    }

    /// Remove a header
    pub fn without_header(mut self, name: impl Into<String>) -> Self {
        self.remove_headers.push(name.into());
        self
    }

    /// Rewrite path
    pub fn with_path_rewrite(
        mut self,
        pattern: impl Into<String>,
        replacement: impl Into<String>,
    ) -> Self {
        self.rewrite_path = Some(PathRewrite {
            pattern: pattern.into(),
            replacement: replacement.into(),
        });
        self
    }
}

/// Path rewrite configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathRewrite {
    /// Pattern to match (regex)
    pub pattern: String,

    /// Replacement string
    pub replacement: String,
}

/// Body transformation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BodyTransform {
    /// Replace body with static content
    Replace {
        /// New body content
        content: String,
        /// Content-Type header value
        content_type: String,
    },

    /// JSON transformation
    JsonTransform {
        /// JSONPath expressions to apply
        operations: Vec<JsonOperation>,
    },

    /// Template-based transformation
    Template {
        /// Template string
        template: String,
    },

    /// Custom transformation (plugin-specific)
    Custom(serde_json::Value),
}

/// JSON transformation operation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum JsonOperation {
    /// Add a field
    Add {
        /// Path to add (JSONPath)
        path: String,
        /// Value to add
        value: serde_json::Value,
    },

    /// Remove a field
    Remove {
        /// Path to remove (JSONPath)
        path: String,
    },

    /// Replace a field value
    Replace {
        /// Path to replace (JSONPath)
        path: String,
        /// New value
        value: serde_json::Value,
    },

    /// Rename a field
    Rename {
        /// Old path (JSONPath)
        from: String,
        /// New path (JSONPath)
        to: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transform_config() {
        let config = TransformConfig::new()
            .with_header("X-Custom", "value")
            .without_header("X-Remove")
            .with_path_rewrite("^/old", "/new");

        assert_eq!(
            config.add_headers.get("X-Custom"),
            Some(&"value".to_string())
        );
        assert_eq!(config.remove_headers, vec!["X-Remove"]);
        assert!(config.rewrite_path.is_some());
    }

    #[test]
    fn test_json_operation() {
        let op = JsonOperation::Add {
            path: "$.field".to_string(),
            value: serde_json::json!("value"),
        };

        let serialized = serde_json::to_string(&op).unwrap();
        assert!(serialized.contains("\"op\":\"add\""));
    }
}
