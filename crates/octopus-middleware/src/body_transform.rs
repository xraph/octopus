//! JSON body transformation middleware
//!
//! Applies field-level transformations (remove, rename, set, redact) to
//! JSON request and response bodies. Only operates on payloads with
//! `Content-Type: application/json`.

use async_trait::async_trait;
use bytes::Bytes;
use http::{header, Request, Response};
use http_body_util::{BodyExt, Full};
use octopus_core::{Error, Middleware, Next, Result};
use serde_json::Value;
use std::fmt;

/// Body type alias
pub type Body = Full<Bytes>;

/// JSON body transformation configuration
#[derive(Debug, Clone)]
pub struct BodyTransformConfig {
    /// Rules applied to the request body before forwarding
    pub request_rules: Vec<BodyRule>,
    /// Rules applied to the response body before returning to the client
    pub response_rules: Vec<BodyRule>,
}

impl Default for BodyTransformConfig {
    fn default() -> Self {
        Self {
            request_rules: Vec::new(),
            response_rules: Vec::new(),
        }
    }
}

/// A single body transformation rule
#[derive(Debug, Clone)]
pub enum BodyRule {
    /// Remove a field at the given dot-separated JSON path
    RemoveField(String),
    /// Rename a field from one path to another
    RenameField {
        /// Source dot-separated path
        from: String,
        /// Destination dot-separated path
        to: String,
    },
    /// Set a field at the given path to a fixed value
    SetField {
        /// Dot-separated path
        path: String,
        /// Value to set
        value: Value,
    },
    /// Replace the value at the given path with "***REDACTED***"
    RedactField(String),
}

/// JSON body transformation middleware
#[derive(Clone)]
pub struct BodyTransform {
    config: BodyTransformConfig,
}

impl BodyTransform {
    /// Create a new body transform middleware with default (empty) configuration
    pub fn new() -> Self {
        Self::with_config(BodyTransformConfig::default())
    }

    /// Create a new body transform middleware with the given configuration
    pub fn with_config(config: BodyTransformConfig) -> Self {
        Self { config }
    }

    /// Check whether a Content-Type header value indicates JSON
    fn is_json_content_type(headers: &http::HeaderMap) -> bool {
        headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|ct| ct.contains("application/json"))
            .unwrap_or(false)
    }

    /// Apply a list of rules to a JSON value
    fn apply_rules(mut value: Value, rules: &[BodyRule]) -> Value {
        for rule in rules {
            match rule {
                BodyRule::RemoveField(path) => {
                    remove_at_path(&mut value, path);
                }
                BodyRule::RenameField { from, to } => {
                    if let Some(extracted) = remove_at_path(&mut value, from) {
                        set_at_path(&mut value, to, extracted);
                    }
                }
                BodyRule::SetField { path, value: val } => {
                    set_at_path(&mut value, path, val.clone());
                }
                BodyRule::RedactField(path) => {
                    if get_at_path(&value, path).is_some() {
                        set_at_path(
                            &mut value,
                            path,
                            Value::String("***REDACTED***".to_string()),
                        );
                    }
                }
            }
        }
        value
    }
}

impl Default for BodyTransform {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for BodyTransform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BodyTransform")
            .field("request_rules", &self.config.request_rules.len())
            .field("response_rules", &self.config.response_rules.len())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// JSON path helpers (dot-separated)
// ---------------------------------------------------------------------------

/// Split a dot-separated path into segments.
fn path_segments(path: &str) -> Vec<&str> {
    path.split('.').filter(|s| !s.is_empty()).collect()
}

/// Navigate to the parent of the target key and return a mutable reference to
/// the parent object together with the final key segment.
fn navigate_to_parent_mut<'a, 'b>(
    root: &'a mut Value,
    segments: &'b [&'b str],
) -> Option<(&'a mut Value, &'b str)> {
    if segments.is_empty() {
        return None;
    }
    if segments.len() == 1 {
        return Some((root, segments[0]));
    }
    let mut current = root;
    for &seg in &segments[..segments.len() - 1] {
        current = current.get_mut(seg)?;
    }
    Some((current, segments[segments.len() - 1]))
}

/// Remove the value at a dot-separated path, returning the removed value.
fn remove_at_path(root: &mut Value, path: &str) -> Option<Value> {
    let segs = path_segments(path);
    let (parent, key) = navigate_to_parent_mut(root, &segs)?;
    parent.as_object_mut()?.remove(key)
}

/// Set a value at a dot-separated path, creating intermediate objects as needed.
fn set_at_path(root: &mut Value, path: &str, val: Value) {
    let segs = path_segments(path);
    if segs.is_empty() {
        return;
    }
    let mut current = root;
    for &seg in &segs[..segs.len() - 1] {
        if !current.get(seg).map(|v| v.is_object()).unwrap_or(false) {
            current[seg] = Value::Object(serde_json::Map::new());
        }
        current = current.get_mut(seg).unwrap();
    }
    current[segs[segs.len() - 1]] = val;
}

/// Get a reference to the value at a dot-separated path.
fn get_at_path<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    let segs = path_segments(path);
    let mut current = root;
    for seg in segs {
        current = current.get(seg)?;
    }
    Some(current)
}

// ---------------------------------------------------------------------------
// Middleware implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl Middleware for BodyTransform {
    async fn call(&self, req: Request<Body>, next: Next) -> Result<Response<Body>> {
        let has_request_rules = !self.config.request_rules.is_empty();
        let has_response_rules = !self.config.response_rules.is_empty();

        // --- Transform request body ---
        let req = if has_request_rules && Self::is_json_content_type(req.headers()) {
            let (parts, body) = req.into_parts();
            let body_bytes = body
                .collect()
                .await
                .map(|c| c.to_bytes())
                .unwrap_or_default();

            match serde_json::from_slice::<Value>(&body_bytes) {
                Ok(json) => {
                    let transformed = Self::apply_rules(json, &self.config.request_rules);
                    let new_bytes = serde_json::to_vec(&transformed)
                        .map_err(|e| Error::Internal(format!("JSON serialization failed: {e}")))?;
                    let len = new_bytes.len();
                    let mut new_req = Request::from_parts(parts, Full::new(Bytes::from(new_bytes)));
                    new_req.headers_mut().insert(
                        header::CONTENT_LENGTH,
                        http::HeaderValue::from_str(&len.to_string()).unwrap(),
                    );
                    new_req
                }
                Err(_) => {
                    // Not valid JSON -- pass through unchanged
                    Request::from_parts(parts, Full::new(body_bytes))
                }
            }
        } else {
            req
        };

        // --- Forward to next middleware ---
        let response = next.run(req).await?;

        // --- Transform response body ---
        if has_response_rules && Self::is_json_content_type(response.headers()) {
            let (parts, body) = response.into_parts();
            let body_bytes = body
                .collect()
                .await
                .map(|c| c.to_bytes())
                .unwrap_or_default();

            match serde_json::from_slice::<Value>(&body_bytes) {
                Ok(json) => {
                    let transformed = Self::apply_rules(json, &self.config.response_rules);
                    let new_bytes = serde_json::to_vec(&transformed)
                        .map_err(|e| Error::Internal(format!("JSON serialization failed: {e}")))?;
                    let len = new_bytes.len();
                    let mut new_resp =
                        Response::from_parts(parts, Full::new(Bytes::from(new_bytes)));
                    new_resp.headers_mut().insert(
                        header::CONTENT_LENGTH,
                        http::HeaderValue::from_str(&len.to_string()).unwrap(),
                    );
                    Ok(new_resp)
                }
                Err(_) => Ok(Response::from_parts(parts, Full::new(body_bytes))),
            }
        } else {
            Ok(response)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::StatusCode;
    use http_body_util::BodyExt;
    use std::sync::Arc;

    #[derive(Debug)]
    struct EchoHandler;

    #[async_trait]
    impl Middleware for EchoHandler {
        async fn call(&self, req: Request<Body>, _next: Next) -> Result<Response<Body>> {
            // Echo the request body back as the response body
            let (_parts, body) = req.into_parts();
            let body_bytes = body
                .collect()
                .await
                .map(|c| c.to_bytes())
                .unwrap_or_default();
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Full::new(body_bytes))
                .map_err(|e| Error::Internal(e.to_string()))
        }
    }

    /// A handler that returns a fixed JSON body
    #[derive(Debug)]
    struct JsonResponseHandler {
        json: Value,
    }

    #[async_trait]
    impl Middleware for JsonResponseHandler {
        async fn call(&self, _req: Request<Body>, _next: Next) -> Result<Response<Body>> {
            let bytes = serde_json::to_vec(&self.json).unwrap();
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Full::new(Bytes::from(bytes)))
                .map_err(|e| Error::Internal(e.to_string()))
        }
    }

    fn parse_response_json(body_bytes: &[u8]) -> Value {
        serde_json::from_slice(body_bytes).unwrap()
    }

    #[tokio::test]
    async fn test_remove_field() {
        let config = BodyTransformConfig {
            request_rules: vec![BodyRule::RemoveField("user.secret".to_string())],
            response_rules: vec![],
        };
        let transform = BodyTransform::with_config(config);
        let handler = EchoHandler;

        let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([Arc::new(transform), Arc::new(handler)]);

        let input = serde_json::json!({
            "user": {
                "name": "alice",
                "secret": "s3cret"
            }
        });

        let next = Next::new(stack);
        let req = Request::builder()
            .uri("/test")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Full::new(Bytes::from(serde_json::to_vec(&input).unwrap())))
            .unwrap();

        let response = next.run(req).await.unwrap();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json = parse_response_json(&body);

        assert_eq!(json["user"]["name"], "alice");
        assert!(json["user"].get("secret").is_none());
    }

    #[tokio::test]
    async fn test_redact_field() {
        let config = BodyTransformConfig {
            request_rules: vec![],
            response_rules: vec![BodyRule::RedactField("data.email".to_string())],
        };
        let transform = BodyTransform::with_config(config);
        let handler = JsonResponseHandler {
            json: serde_json::json!({
                "data": {
                    "email": "alice@example.com",
                    "name": "Alice"
                }
            }),
        };

        let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([Arc::new(transform), Arc::new(handler)]);

        let next = Next::new(stack);
        let req = Request::builder()
            .uri("/test")
            .body(Body::from(""))
            .unwrap();

        let response = next.run(req).await.unwrap();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json = parse_response_json(&body);

        assert_eq!(json["data"]["email"], "***REDACTED***");
        assert_eq!(json["data"]["name"], "Alice");
    }

    #[tokio::test]
    async fn test_set_field() {
        let config = BodyTransformConfig {
            request_rules: vec![BodyRule::SetField {
                path: "meta.version".to_string(),
                value: Value::String("v2".to_string()),
            }],
            response_rules: vec![],
        };
        let transform = BodyTransform::with_config(config);
        let handler = EchoHandler;

        let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([Arc::new(transform), Arc::new(handler)]);

        let input = serde_json::json!({ "data": 42 });

        let next = Next::new(stack);
        let req = Request::builder()
            .uri("/test")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Full::new(Bytes::from(serde_json::to_vec(&input).unwrap())))
            .unwrap();

        let response = next.run(req).await.unwrap();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json = parse_response_json(&body);

        assert_eq!(json["data"], 42);
        assert_eq!(json["meta"]["version"], "v2");
    }

    #[tokio::test]
    async fn test_rename_field() {
        let config = BodyTransformConfig {
            request_rules: vec![BodyRule::RenameField {
                from: "old_name".to_string(),
                to: "new_name".to_string(),
            }],
            response_rules: vec![],
        };
        let transform = BodyTransform::with_config(config);
        let handler = EchoHandler;

        let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([Arc::new(transform), Arc::new(handler)]);

        let input = serde_json::json!({ "old_name": "value", "keep": true });

        let next = Next::new(stack);
        let req = Request::builder()
            .uri("/test")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Full::new(Bytes::from(serde_json::to_vec(&input).unwrap())))
            .unwrap();

        let response = next.run(req).await.unwrap();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json = parse_response_json(&body);

        assert!(json.get("old_name").is_none());
        assert_eq!(json["new_name"], "value");
        assert_eq!(json["keep"], true);
    }

    #[tokio::test]
    async fn test_non_json_passthrough() {
        let config = BodyTransformConfig {
            request_rules: vec![BodyRule::RemoveField("secret".to_string())],
            response_rules: vec![],
        };
        let transform = BodyTransform::with_config(config);
        let handler = EchoHandler;

        let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([Arc::new(transform), Arc::new(handler)]);

        let next = Next::new(stack);
        // text/plain -- rules should not be applied
        let req = Request::builder()
            .uri("/test")
            .header(header::CONTENT_TYPE, "text/plain")
            .body(Full::new(Bytes::from("plain text body")))
            .unwrap();

        let response = next.run(req).await.unwrap();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"plain text body");
    }

    #[tokio::test]
    async fn test_multiple_rules_combined() {
        let config = BodyTransformConfig {
            request_rules: vec![
                BodyRule::RemoveField("internal_id".to_string()),
                BodyRule::RedactField("user.ssn".to_string()),
                BodyRule::SetField {
                    path: "processed".to_string(),
                    value: Value::Bool(true),
                },
            ],
            response_rules: vec![],
        };
        let transform = BodyTransform::with_config(config);
        let handler = EchoHandler;

        let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([Arc::new(transform), Arc::new(handler)]);

        let input = serde_json::json!({
            "internal_id": 12345,
            "user": {
                "name": "Bob",
                "ssn": "123-45-6789"
            }
        });

        let next = Next::new(stack);
        let req = Request::builder()
            .uri("/test")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Full::new(Bytes::from(serde_json::to_vec(&input).unwrap())))
            .unwrap();

        let response = next.run(req).await.unwrap();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json = parse_response_json(&body);

        assert!(json.get("internal_id").is_none());
        assert_eq!(json["user"]["name"], "Bob");
        assert_eq!(json["user"]["ssn"], "***REDACTED***");
        assert_eq!(json["processed"], true);
    }

    // Unit tests for path helpers
    #[test]
    fn test_get_at_path() {
        let v = serde_json::json!({ "a": { "b": { "c": 42 } } });
        assert_eq!(get_at_path(&v, "a.b.c"), Some(&Value::from(42)));
        assert_eq!(get_at_path(&v, "a.b"), Some(&serde_json::json!({"c": 42})));
        assert!(get_at_path(&v, "a.x").is_none());
    }

    #[test]
    fn test_set_at_path_creates_intermediates() {
        let mut v = serde_json::json!({});
        set_at_path(&mut v, "x.y.z", Value::from(99));
        assert_eq!(v["x"]["y"]["z"], 99);
    }

    #[test]
    fn test_remove_at_path() {
        let mut v = serde_json::json!({ "a": { "b": 1, "c": 2 } });
        let removed = remove_at_path(&mut v, "a.b");
        assert_eq!(removed, Some(Value::from(1)));
        assert!(v["a"].get("b").is_none());
        assert_eq!(v["a"]["c"], 2);
    }
}
