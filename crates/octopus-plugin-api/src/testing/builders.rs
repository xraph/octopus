//! Builder utilities for constructing test requests and responses

use crate::context::{RequestContext, ResponseContext};
use bytes::Bytes;
use http::{Method, Request, Response, StatusCode};
use http_body_util::Full;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

/// Builder for creating test HTTP requests
pub struct RequestBuilder {
    method: Method,
    uri: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

impl RequestBuilder {
    /// Create a new request builder with GET method
    pub fn new() -> Self {
        Self {
            method: Method::GET,
            uri: "/".to_string(),
            headers: HashMap::new(),
            body: Vec::new(),
        }
    }

    /// Create a GET request builder
    pub fn get(uri: impl Into<String>) -> Self {
        Self::new().method(Method::GET).uri(uri)
    }

    /// Create a POST request builder
    pub fn post(uri: impl Into<String>) -> Self {
        Self::new().method(Method::POST).uri(uri)
    }

    /// Create a PUT request builder
    pub fn put(uri: impl Into<String>) -> Self {
        Self::new().method(Method::PUT).uri(uri)
    }

    /// Create a DELETE request builder
    pub fn delete(uri: impl Into<String>) -> Self {
        Self::new().method(Method::DELETE).uri(uri)
    }

    /// Set the HTTP method
    pub fn method(mut self, method: Method) -> Self {
        self.method = method;
        self
    }

    /// Set the URI
    pub fn uri(mut self, uri: impl Into<String>) -> Self {
        self.uri = uri.into();
        self
    }

    /// Add a header
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    /// Add multiple headers
    pub fn headers(mut self, headers: HashMap<String, String>) -> Self {
        self.headers.extend(headers);
        self
    }

    /// Set the request body
    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = body.into();
        self
    }

    /// Set JSON body
    pub fn json_body(mut self, value: &serde_json::Value) -> Self {
        self.body = value.to_string().into_bytes();
        self.headers
            .insert("content-type".to_string(), "application/json".to_string());
        self
    }

    /// Build the HTTP request
    pub fn build(self) -> Request<Full<Bytes>> {
        let mut builder = Request::builder().method(self.method).uri(self.uri);

        for (name, value) in self.headers {
            builder = builder.header(name, value);
        }

        builder.body(Full::new(Bytes::from(self.body))).unwrap()
    }

    /// Build with a request context
    pub fn build_with_context(self) -> (Request<Full<Bytes>>, RequestContext) {
        let ctx = RequestContext::new(
            uuid::Uuid::new_v4().to_string(),
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080),
        );
        (self.build(), ctx)
    }
}

impl Default for RequestBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for creating test HTTP responses
pub struct ResponseBuilder {
    status: StatusCode,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

impl ResponseBuilder {
    /// Create a new response builder with 200 OK status
    pub fn new() -> Self {
        Self {
            status: StatusCode::OK,
            headers: HashMap::new(),
            body: Vec::new(),
        }
    }

    /// Create a 200 OK response
    pub fn ok() -> Self {
        Self::new()
    }

    /// Create a 201 Created response
    pub fn created() -> Self {
        Self::new().status(StatusCode::CREATED)
    }

    /// Create a 400 Bad Request response
    pub fn bad_request() -> Self {
        Self::new().status(StatusCode::BAD_REQUEST)
    }

    /// Create a 401 Unauthorized response
    pub fn unauthorized() -> Self {
        Self::new().status(StatusCode::UNAUTHORIZED)
    }

    /// Create a 404 Not Found response
    pub fn not_found() -> Self {
        Self::new().status(StatusCode::NOT_FOUND)
    }

    /// Create a 500 Internal Server Error response
    pub fn internal_server_error() -> Self {
        Self::new().status(StatusCode::INTERNAL_SERVER_ERROR)
    }

    /// Set the status code
    pub fn status(mut self, status: StatusCode) -> Self {
        self.status = status;
        self
    }

    /// Add a header
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    /// Set the response body
    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = body.into();
        self
    }

    /// Set JSON body
    pub fn json_body(mut self, value: &serde_json::Value) -> Self {
        self.body = value.to_string().into_bytes();
        self.headers
            .insert("content-type".to_string(), "application/json".to_string());
        self
    }

    /// Build the HTTP response
    pub fn build(self) -> Response<Full<Bytes>> {
        let mut builder = Response::builder().status(self.status);

        for (name, value) in self.headers {
            builder = builder.header(name, value);
        }

        builder.body(Full::new(Bytes::from(self.body))).unwrap()
    }

    /// Build with a response context
    pub fn build_with_context(
        self,
        request_id: String,
    ) -> (Response<Full<Bytes>>, ResponseContext) {
        let ctx =
            ResponseContext::new(request_id, Duration::from_millis(100), self.status.as_u16());
        (self.build(), ctx)
    }
}

impl Default for ResponseBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_builder() {
        let req = RequestBuilder::get("/test")
            .header("X-Test", "value")
            .body(b"test body")
            .build();

        assert_eq!(req.method(), Method::GET);
        assert_eq!(req.uri().path(), "/test");
        assert_eq!(req.headers().get("X-Test").unwrap(), "value");
    }

    #[test]
    fn test_request_builder_json() {
        let json = serde_json::json!({"key": "value"});
        let req = RequestBuilder::post("/api").json_body(&json).build();

        assert_eq!(req.method(), Method::POST);
        assert_eq!(
            req.headers().get("content-type").unwrap(),
            "application/json"
        );
    }

    #[test]
    fn test_response_builder() {
        let res = ResponseBuilder::ok()
            .header("X-Custom", "value")
            .body(b"response body")
            .build();

        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers().get("X-Custom").unwrap(), "value");
    }

    #[test]
    fn test_response_builder_json() {
        let json = serde_json::json!({"status": "success"});
        let res = ResponseBuilder::ok().json_body(&json).build();

        assert_eq!(
            res.headers().get("content-type").unwrap(),
            "application/json"
        );
    }
}
