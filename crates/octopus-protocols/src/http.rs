//! HTTP/REST protocol handler

use crate::handler::{ProtocolHandler, ProtocolType};
use async_trait::async_trait;
use bytes::Bytes;
use http::{Method, Request, Response, StatusCode};
use http_body_util::Full;
use octopus_core::{ResponseBuilder, Result};

/// HTTP protocol handler
#[derive(Debug, Clone)]
pub struct HttpHandler {
    /// Allowed HTTP methods
    pub allowed_methods: Vec<Method>,
}

impl Default for HttpHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpHandler {
    /// Create a new HTTP handler
    #[must_use] pub fn new() -> Self {
        Self {
            allowed_methods: vec![
                Method::GET,
                Method::POST,
                Method::PUT,
                Method::DELETE,
                Method::PATCH,
                Method::HEAD,
                Method::OPTIONS,
            ],
        }
    }

    /// Create an HTTP handler with specific allowed methods
    #[must_use] pub const fn with_methods(methods: Vec<Method>) -> Self {
        Self {
            allowed_methods: methods,
        }
    }

    /// Check if method is allowed
    #[must_use] pub fn is_method_allowed(&self, method: &Method) -> bool {
        self.allowed_methods.contains(method)
    }

    /// Handle OPTIONS request (CORS preflight)
    fn handle_options(&self) -> Result<Response<Full<Bytes>>> {
        let methods_str = self
            .allowed_methods
            .iter()
            .map(http::Method::as_str)
            .collect::<Vec<_>>()
            .join(", ");

        ResponseBuilder::new(StatusCode::NO_CONTENT)
            .header(http::header::ALLOW, methods_str.clone())
            .header(http::header::ACCESS_CONTROL_ALLOW_METHODS, methods_str)
            .header(
                http::header::ACCESS_CONTROL_ALLOW_HEADERS,
                "Content-Type, Authorization",
            )
            .build()
    }
}

#[async_trait]
impl ProtocolHandler for HttpHandler {
    fn protocol_type(&self) -> ProtocolType {
        ProtocolType::Http
    }

    fn can_handle(&self, req: &Request<Full<Bytes>>) -> bool {
        // Can handle all HTTP requests, but check if method is allowed
        req.method() == Method::OPTIONS || self.is_method_allowed(req.method())
    }

    async fn handle(&self, req: Request<Full<Bytes>>) -> Result<Response<Full<Bytes>>> {
        // Handle OPTIONS specially
        if req.method() == Method::OPTIONS {
            return self.handle_options();
        }

        // For non-OPTIONS, this is just a pass-through
        // The actual proxying happens elsewhere
        ResponseBuilder::new(StatusCode::OK)
            .json()
            .json_body(&serde_json::json!({"status": "ok"}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_handler_creation() {
        let handler = HttpHandler::new();
        assert_eq!(handler.protocol_type(), ProtocolType::Http);
        assert!(handler.is_method_allowed(&Method::GET));
        assert!(handler.is_method_allowed(&Method::POST));
    }

    #[test]
    fn test_http_handler_with_methods() {
        let handler = HttpHandler::with_methods(vec![Method::GET, Method::POST]);
        assert!(handler.is_method_allowed(&Method::GET));
        assert!(handler.is_method_allowed(&Method::POST));
        assert!(!handler.is_method_allowed(&Method::DELETE));
    }

    #[tokio::test]
    async fn test_http_handler_can_handle() {
        let handler = HttpHandler::new();

        let get_req = Request::builder()
            .method(Method::GET)
            .uri("/test")
            .body(Full::new(Bytes::new()))
            .unwrap();

        assert!(handler.can_handle(&get_req));

        let options_req = Request::builder()
            .method(Method::OPTIONS)
            .uri("/test")
            .body(Full::new(Bytes::new()))
            .unwrap();

        assert!(handler.can_handle(&options_req));
    }

    #[tokio::test]
    async fn test_http_handler_options() {
        let handler = HttpHandler::new();

        let req = Request::builder()
            .method(Method::OPTIONS)
            .uri("/test")
            .body(Full::new(Bytes::new()))
            .unwrap();

        let response = handler.handle(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert!(response.headers().contains_key(http::header::ALLOW));
    }
}
