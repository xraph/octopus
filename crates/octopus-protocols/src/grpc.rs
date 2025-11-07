//! gRPC protocol handler

use crate::handler::{ProtocolHandler, ProtocolType};
use async_trait::async_trait;
use bytes::Bytes;
use http::{header, Method, Request, Response, StatusCode};
use http_body_util::Full;
use octopus_core::{Error, Result};
use std::collections::HashMap;
use tracing::{debug, warn};

/// gRPC protocol handler
#[derive(Debug, Clone)]
pub struct GrpcHandler {
    /// Configured services and their upstreams
    services: HashMap<String, String>,

    /// Enable gRPC reflection
    #[allow(dead_code)]
    enable_reflection: bool,

    /// Maximum message size in bytes
    #[allow(dead_code)]
    max_message_size: usize,
}

impl Default for GrpcHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl GrpcHandler {
    /// Create a new gRPC handler
    #[must_use] pub fn new() -> Self {
        Self {
            services: HashMap::new(),
            enable_reflection: true,
            max_message_size: 4 * 1024 * 1024, // 4 MB
        }
    }

    /// Create gRPC handler with custom configuration
    #[must_use] pub const fn with_config(
        services: HashMap<String, String>,
        enable_reflection: bool,
        max_message_size: usize,
    ) -> Self {
        Self {
            services,
            enable_reflection,
            max_message_size,
        }
    }

    /// Check if request is a gRPC request
    pub fn is_grpc_request(req: &Request<Full<Bytes>>) -> bool {
        req.headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("application/grpc"))
    }

    /// Extract gRPC service and method from path
    #[must_use] pub fn parse_grpc_path(path: &str) -> Option<(String, String)> {
        // gRPC paths are in format: /{service}/{method}
        let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
        if parts.len() == 2 {
            Some((parts[0].to_string(), parts[1].to_string()))
        } else {
            None
        }
    }

    /// Get gRPC status code from response
    pub fn get_grpc_status(res: &Response<Full<Bytes>>) -> Option<i32> {
        res.headers()
            .get("grpc-status")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
    }

    /// Build gRPC error response
    pub fn error_response(status_code: i32, message: &str) -> Result<Response<Full<Bytes>>> {
        Response::builder()
            .status(StatusCode::OK) // gRPC uses HTTP 200 with grpc-status for errors
            .header(header::CONTENT_TYPE, "application/grpc")
            .header("grpc-status", status_code.to_string())
            .header("grpc-message", message)
            .body(Full::new(Bytes::new()))
            .map_err(|e| Error::Internal(format!("Failed to build gRPC error response: {e}")))
    }
}

#[async_trait]
impl ProtocolHandler for GrpcHandler {
    fn protocol_type(&self) -> ProtocolType {
        ProtocolType::Grpc
    }

    fn can_handle(&self, req: &Request<Full<Bytes>>) -> bool {
        Self::is_grpc_request(req)
    }

    async fn handle(&self, req: Request<Full<Bytes>>) -> Result<Response<Full<Bytes>>> {
        // Verify gRPC request
        if !Self::is_grpc_request(&req) {
            return Self::error_response(12, "Not a valid gRPC request"); // UNIMPLEMENTED
        }

        // Only POST is allowed for gRPC
        if req.method() != Method::POST {
            return Self::error_response(12, "Only POST method is allowed for gRPC");
        }

        let path = req.uri().path();

        // Parse service and method
        let (service, method) = if let Some(parsed) = Self::parse_grpc_path(path) { parsed } else {
            warn!(path = %path, "Invalid gRPC path format");
            return Self::error_response(12, "Invalid gRPC path format");
        };

        debug!(service = %service, method = %method, "Handling gRPC request");

        // Check if service is registered
        if !self.services.is_empty() && !self.services.contains_key(&service) {
            warn!(service = %service, "Service not found");
            return Self::error_response(5, &format!("Service '{service}' not found"));
            // NOT_FOUND
        }

        // In a real implementation, this would:
        // 1. Route to the appropriate upstream gRPC service
        // 2. Handle streaming (unary, client-streaming, server-streaming, bidirectional)
        // 3. Handle gRPC metadata
        // 4. Handle trailers
        // 5. Support gRPC reflection if enabled

        // For now, return success
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/grpc")
            .header("grpc-status", "0") // OK
            .body(Full::new(Bytes::new()))
            .map_err(|e| Error::Internal(format!("Failed to build gRPC response: {e}")))
    }
}

/// gRPC status codes
#[allow(dead_code)]
pub mod status_codes {
    /// OK (0)
    pub const OK: i32 = 0;
    /// CANCELLED (1)
    pub const CANCELLED: i32 = 1;
    /// UNKNOWN (2)
    pub const UNKNOWN: i32 = 2;
    /// `INVALID_ARGUMENT` (3)
    pub const INVALID_ARGUMENT: i32 = 3;
    /// `DEADLINE_EXCEEDED` (4)
    pub const DEADLINE_EXCEEDED: i32 = 4;
    /// `NOT_FOUND` (5)
    pub const NOT_FOUND: i32 = 5;
    /// `ALREADY_EXISTS` (6)
    pub const ALREADY_EXISTS: i32 = 6;
    /// `PERMISSION_DENIED` (7)
    pub const PERMISSION_DENIED: i32 = 7;
    /// `RESOURCE_EXHAUSTED` (8)
    pub const RESOURCE_EXHAUSTED: i32 = 8;
    /// `FAILED_PRECONDITION` (9)
    pub const FAILED_PRECONDITION: i32 = 9;
    /// ABORTED (10)
    pub const ABORTED: i32 = 10;
    /// `OUT_OF_RANGE` (11)
    pub const OUT_OF_RANGE: i32 = 11;
    /// UNIMPLEMENTED (12)
    pub const UNIMPLEMENTED: i32 = 12;
    /// INTERNAL (13)
    pub const INTERNAL: i32 = 13;
    /// UNAVAILABLE (14)
    pub const UNAVAILABLE: i32 = 14;
    /// `DATA_LOSS` (15)
    pub const DATA_LOSS: i32 = 15;
    /// UNAUTHENTICATED (16)
    pub const UNAUTHENTICATED: i32 = 16;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grpc_handler_creation() {
        let handler = GrpcHandler::new();
        assert_eq!(handler.protocol_type(), ProtocolType::Grpc);
    }

    #[test]
    fn test_is_grpc_request() {
        let grpc_req = Request::builder()
            .uri("/service.Name/Method")
            .header(header::CONTENT_TYPE, "application/grpc")
            .body(Full::new(Bytes::new()))
            .unwrap();

        assert!(GrpcHandler::is_grpc_request(&grpc_req));

        let http_req = Request::builder()
            .uri("/api/users")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Full::new(Bytes::new()))
            .unwrap();

        assert!(!GrpcHandler::is_grpc_request(&http_req));
    }

    #[test]
    fn test_parse_grpc_path() {
        assert_eq!(
            GrpcHandler::parse_grpc_path("/users.UserService/GetUser"),
            Some(("users.UserService".to_string(), "GetUser".to_string()))
        );

        assert_eq!(GrpcHandler::parse_grpc_path("/invalid"), None);
        assert_eq!(GrpcHandler::parse_grpc_path("/a/b/c"), None);
    }

    #[tokio::test]
    async fn test_grpc_handler_can_handle() {
        let handler = GrpcHandler::new();

        let grpc_req = Request::builder()
            .uri("/service/Method")
            .header(header::CONTENT_TYPE, "application/grpc")
            .body(Full::new(Bytes::new()))
            .unwrap();

        assert!(handler.can_handle(&grpc_req));

        let http_req = Request::builder()
            .uri("/api")
            .body(Full::new(Bytes::new()))
            .unwrap();

        assert!(!handler.can_handle(&http_req));
    }
}
