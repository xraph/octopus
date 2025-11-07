//! Response builder and utilities

use crate::Result;
use bytes::Bytes;
use http::{header, Response, StatusCode};
use http_body_util::Full;
use serde::Serialize;

/// Body type alias
pub type Body = Full<Bytes>;

/// Response builder for convenient response construction
#[derive(Debug)]
pub struct ResponseBuilder {
    status: StatusCode,
    headers: Vec<(header::HeaderName, String)>,
}

impl ResponseBuilder {
    /// Create a new response builder
    pub fn new(status: StatusCode) -> Self {
        Self {
            status,
            headers: Vec::new(),
        }
    }

    /// Set a header
    pub fn header(mut self, name: header::HeaderName, value: impl Into<String>) -> Self {
        self.headers.push((name, value.into()));
        self
    }

    /// Set content type to JSON
    pub fn json(self) -> Self {
        self.header(header::CONTENT_TYPE, "application/json")
    }

    /// Build response with empty body
    pub fn build(self) -> Result<Response<Body>> {
        let mut response = Response::builder().status(self.status);

        for (name, value) in self.headers {
            response = response.header(name, value);
        }

        Ok(response.body(Full::new(Bytes::new()))?)
    }

    /// Build response with text body
    pub fn text(self, body: impl Into<String>) -> Result<Response<Body>> {
        let mut response = Response::builder().status(self.status);

        response = response.header(header::CONTENT_TYPE, "text/plain; charset=utf-8");

        for (name, value) in self.headers {
            response = response.header(name, value);
        }

        Ok(response.body(Full::new(Bytes::from(body.into())))?)
    }

    /// Build response with JSON body
    pub fn json_body<T: Serialize>(self, body: &T) -> Result<Response<Body>> {
        let json = serde_json::to_string(body)?;

        let mut response = Response::builder().status(self.status);

        response = response.header(header::CONTENT_TYPE, "application/json");

        for (name, value) in self.headers {
            response = response.header(name, value);
        }

        Ok(response.body(Full::new(Bytes::from(json)))?)
    }
}

/// Convenience functions for common responses
pub mod responses {
    use super::*;

    /// 200 OK
    pub fn ok() -> ResponseBuilder {
        ResponseBuilder::new(StatusCode::OK)
    }

    /// 201 Created
    pub fn created() -> ResponseBuilder {
        ResponseBuilder::new(StatusCode::CREATED)
    }

    /// 204 No Content
    pub fn no_content() -> Result<Response<Body>> {
        ResponseBuilder::new(StatusCode::NO_CONTENT).build()
    }

    /// 400 Bad Request
    pub fn bad_request(message: impl Into<String>) -> Result<Response<Body>> {
        ResponseBuilder::new(StatusCode::BAD_REQUEST).text(message)
    }

    /// 401 Unauthorized
    pub fn unauthorized(message: impl Into<String>) -> Result<Response<Body>> {
        ResponseBuilder::new(StatusCode::UNAUTHORIZED).text(message)
    }

    /// 403 Forbidden
    pub fn forbidden(message: impl Into<String>) -> Result<Response<Body>> {
        ResponseBuilder::new(StatusCode::FORBIDDEN).text(message)
    }

    /// 404 Not Found
    pub fn not_found(message: impl Into<String>) -> Result<Response<Body>> {
        ResponseBuilder::new(StatusCode::NOT_FOUND).text(message)
    }

    /// 429 Too Many Requests
    pub fn too_many_requests() -> Result<Response<Body>> {
        ResponseBuilder::new(StatusCode::TOO_MANY_REQUESTS).text("Rate limit exceeded")
    }

    /// 500 Internal Server Error
    pub fn internal_error(message: impl Into<String>) -> Result<Response<Body>> {
        ResponseBuilder::new(StatusCode::INTERNAL_SERVER_ERROR).text(message)
    }

    /// 502 Bad Gateway
    pub fn bad_gateway(message: impl Into<String>) -> Result<Response<Body>> {
        ResponseBuilder::new(StatusCode::BAD_GATEWAY).text(message)
    }

    /// 503 Service Unavailable
    pub fn service_unavailable(message: impl Into<String>) -> Result<Response<Body>> {
        ResponseBuilder::new(StatusCode::SERVICE_UNAVAILABLE).text(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_response_builder() {
        let response = ResponseBuilder::new(StatusCode::OK)
            .header(header::HeaderName::from_static("x-custom"), "value")
            .text("Hello, World!")
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("x-custom").unwrap(), "value");
    }

    #[test]
    fn test_json_response() {
        use serde_json::json;

        let data = json!({
            "message": "success"
        });

        let response = ResponseBuilder::new(StatusCode::OK)
            .json_body(&data)
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/json"
        );
    }
}
