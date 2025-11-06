//! Script execution context

use http::{HeaderName, HeaderValue, Method, StatusCode, Uri, Version};
use std::collections::HashMap;
use std::str::FromStr;

/// Request context exposed to scripts
#[derive(Debug, Clone)]
pub struct RequestContext {
    /// HTTP method
    pub method: String,
    /// Request URI
    pub uri: String,
    /// HTTP version
    pub version: String,
    /// Request headers
    pub headers: HashMap<String, String>,
    /// Request body (if available)
    pub body: Option<Vec<u8>>,
    /// Query parameters
    pub query: HashMap<String, String>,
    /// Path parameters (from router)
    pub path_params: HashMap<String, String>,
    /// Custom metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

impl RequestContext {
    /// Create from HTTP request
    pub fn from_request<B>(req: &http::Request<B>) -> Self {
        let uri = req.uri();
        let query = uri
            .query()
            .map(|q| {
                form_urlencoded::parse(q.as_bytes())
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        Self {
            method: req.method().to_string(),
            uri: uri.to_string(),
            version: format!("{:?}", req.version()),
            headers: req
                .headers()
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
                .collect(),
            body: None, // Will be populated if needed
            query,
            path_params: HashMap::new(),
            metadata: HashMap::new(),
        }
    }

    /// Apply changes back to HTTP request
    pub fn apply_to_request<B>(&self, req: &mut http::Request<B>) -> Result<(), String> {
        // Update method
        *req.method_mut() = Method::from_str(&self.method)
            .map_err(|e| format!("Invalid method: {}", e))?;

        // Update URI
        *req.uri_mut() = Uri::from_str(&self.uri)
            .map_err(|e| format!("Invalid URI: {}", e))?;

        // Update version
        *req.version_mut() = match self.version.as_str() {
            "HTTP/0.9" => Version::HTTP_09,
            "HTTP/1.0" => Version::HTTP_10,
            "HTTP/1.1" => Version::HTTP_11,
            "HTTP/2.0" => Version::HTTP_2,
            "HTTP/3.0" => Version::HTTP_3,
            _ => Version::HTTP_11,
        };

        // Update headers
        req.headers_mut().clear();
        for (key, value) in &self.headers {
            let header_name = HeaderName::from_str(key)
                .map_err(|e| format!("Invalid header name '{}': {}", key, e))?;
            let header_value = HeaderValue::from_str(value)
                .map_err(|e| format!("Invalid header value for '{}': {}", key, e))?;
            req.headers_mut().insert(header_name, header_value);
        }

        Ok(())
    }

    /// Get body as string
    pub fn body_string(&self) -> Option<String> {
        self.body.as_ref().and_then(|b| String::from_utf8(b.clone()).ok())
    }

    /// Set body from string
    pub fn set_body_string(&mut self, body: String) {
        self.body = Some(body.into_bytes());
    }

    /// Get body as JSON
    pub fn body_json(&self) -> Option<serde_json::Value> {
        self.body_string().and_then(|s| serde_json::from_str(&s).ok())
    }

    /// Set body from JSON
    pub fn set_body_json(&mut self, value: serde_json::Value) {
        if let Ok(json_str) = serde_json::to_string(&value) {
            self.set_body_string(json_str);
        }
    }
}

/// Response context exposed to scripts
#[derive(Debug, Clone)]
pub struct ResponseContext {
    /// HTTP status code
    pub status: u16,
    /// Response headers
    pub headers: HashMap<String, String>,
    /// Response body (if available)
    pub body: Option<Vec<u8>>,
    /// Custom metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

impl ResponseContext {
    /// Create from HTTP response
    pub fn from_response<B>(res: &http::Response<B>) -> Self {
        Self {
            status: res.status().as_u16(),
            headers: res
                .headers()
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
                .collect(),
            body: None, // Will be populated if needed
            metadata: HashMap::new(),
        }
    }

    /// Apply changes back to HTTP response
    pub fn apply_to_response<B>(&self, res: &mut http::Response<B>) -> Result<(), String> {
        // Update status
        *res.status_mut() = StatusCode::from_u16(self.status)
            .map_err(|e| format!("Invalid status code: {}", e))?;

        // Update headers
        res.headers_mut().clear();
        for (key, value) in &self.headers {
            let header_name = HeaderName::from_str(key)
                .map_err(|e| format!("Invalid header name '{}': {}", key, e))?;
            let header_value = HeaderValue::from_str(value)
                .map_err(|e| format!("Invalid header value for '{}': {}", key, e))?;
            res.headers_mut().insert(header_name, header_value);
        }

        Ok(())
    }

    /// Get body as string
    pub fn body_string(&self) -> Option<String> {
        self.body.as_ref().and_then(|b| String::from_utf8(b.clone()).ok())
    }

    /// Set body from string
    pub fn set_body_string(&mut self, body: String) {
        self.body = Some(body.into_bytes());
    }

    /// Get body as JSON
    pub fn body_json(&self) -> Option<serde_json::Value> {
        self.body_string().and_then(|s| serde_json::from_str(&s).ok())
    }

    /// Set body from JSON
    pub fn set_body_json(&mut self, value: serde_json::Value) {
        if let Ok(json_str) = serde_json::to_string(&value) {
            self.set_body_string(json_str);
        }
    }
}

/// Combined script context (used internally)
#[derive(Debug, Clone)]
pub enum ScriptContext {
    /// Request context
    Request(RequestContext),
    /// Response context
    Response(ResponseContext),
}

impl ScriptContext {
    /// Get as request context
    pub fn as_request(&self) -> Option<&RequestContext> {
        match self {
            Self::Request(ctx) => Some(ctx),
            _ => None,
        }
    }

    /// Get as mutable request context
    pub fn as_request_mut(&mut self) -> Option<&mut RequestContext> {
        match self {
            Self::Request(ctx) => Some(ctx),
            _ => None,
        }
    }

    /// Get as response context
    pub fn as_response(&self) -> Option<&ResponseContext> {
        match self {
            Self::Response(ctx) => Some(ctx),
            _ => None,
        }
    }

    /// Get as mutable response context
    pub fn as_response_mut(&mut self) -> Option<&mut ResponseContext> {
        match self {
            Self::Response(ctx) => Some(ctx),
            _ => None,
        }
    }
}

