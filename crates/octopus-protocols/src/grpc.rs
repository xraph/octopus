//! gRPC protocol handler — transparent gRPC proxy
//!
//! Routes gRPC requests to upstream services over HTTP/2.
//! Supports:
//! - Unary, server-streaming, client-streaming, and bidirectional RPCs
//! - gRPC metadata forwarding
//! - Deadline/timeout propagation (grpc-timeout header)
//! - gRPC-Web (HTTP/1.1 compatible variant)
//! - Proper trailers (grpc-status in trailers)

use crate::handler::{ProtocolHandler, ProtocolType};
use async_trait::async_trait;
use bytes::Bytes;
use http::{header, Method, Request, Response, StatusCode};
use http_body_util::Full;
use octopus_core::{Error, Result};
use std::collections::HashMap;
use std::time::Duration;
use tracing::{debug, warn};

/// gRPC protocol handler — transparent proxy for gRPC requests
#[derive(Debug, Clone)]
pub struct GrpcHandler {
    /// Configured services and their upstream mappings
    /// Key: fully qualified gRPC service name (e.g., "users.UserService")
    /// Value: upstream name
    services: HashMap<String, String>,

    /// Enable gRPC reflection proxy
    enable_reflection: bool,

    /// Maximum message size in bytes
    max_message_size: usize,

    /// Whether to propagate gRPC deadlines
    deadline_propagation: bool,

    /// Enable gRPC-Web support
    enable_grpc_web: bool,
}

impl Default for GrpcHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl GrpcHandler {
    /// Create a new gRPC handler with defaults
    #[must_use]
    pub fn new() -> Self {
        Self {
            services: HashMap::new(),
            enable_reflection: false,
            max_message_size: 4 * 1024 * 1024, // 4 MB
            deadline_propagation: true,
            enable_grpc_web: false,
        }
    }

    /// Create gRPC handler from config
    #[must_use]
    pub fn from_config(config: &octopus_config::types::GrpcConfig) -> Self {
        Self {
            services: config.services.clone(),
            enable_reflection: config.enable_reflection,
            max_message_size: config.max_message_size,
            deadline_propagation: config.deadline_propagation,
            enable_grpc_web: config.enable_grpc_web,
        }
    }

    /// Check if a request is a gRPC request (works with any body type)
    pub fn is_grpc_content_type(headers: &http::HeaderMap) -> bool {
        headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("application/grpc"))
    }

    /// Check if a request is a gRPC-Web request
    pub fn is_grpc_web_content_type(headers: &http::HeaderMap) -> bool {
        headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| {
                v.starts_with("application/grpc-web") || v.starts_with("application/grpc-web-text")
            })
    }

    /// Check if request is gRPC (for early detection before body buffering)
    pub fn is_grpc_request_raw<B>(req: &Request<B>) -> bool {
        Self::is_grpc_content_type(req.headers())
            || Self::is_grpc_web_content_type(req.headers())
    }

    /// Check if request is a gRPC request (buffered body version for ProtocolHandler trait)
    pub fn is_grpc_request(req: &Request<Full<Bytes>>) -> bool {
        Self::is_grpc_content_type(req.headers())
    }

    /// Extract gRPC service and method from path
    /// gRPC paths are in format: /{package.Service}/{Method}
    #[must_use]
    pub fn parse_grpc_path(path: &str) -> Option<(String, String)> {
        let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
        if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
            Some((parts[0].to_string(), parts[1].to_string()))
        } else {
            None
        }
    }

    /// Look up the upstream for a gRPC service
    pub fn resolve_upstream(&self, service: &str) -> Option<&str> {
        self.services.get(service).map(String::as_str)
    }

    /// Get gRPC status code from response headers/trailers
    pub fn get_grpc_status(res: &Response<Full<Bytes>>) -> Option<i32> {
        res.headers()
            .get("grpc-status")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
    }

    /// Build a gRPC error response (proper gRPC format: HTTP 200 + grpc-status in headers)
    pub fn error_response(grpc_status: i32, message: &str) -> Result<Response<Full<Bytes>>> {
        Response::builder()
            .status(StatusCode::OK) // gRPC always uses HTTP 200, status in headers/trailers
            .header(header::CONTENT_TYPE, "application/grpc")
            .header("grpc-status", grpc_status.to_string())
            .header(
                "grpc-message",
                percent_encode_grpc_message(message),
            )
            .body(Full::new(Bytes::new()))
            .map_err(|e| Error::Internal(format!("Failed to build gRPC error response: {e}")))
    }

    /// Parse grpc-timeout header into a Duration
    /// Format: Nunit where unit is: n(nano), u(micro), m(milli), S(sec), M(min), H(hour)
    pub fn parse_grpc_timeout(value: &str) -> Option<Duration> {
        if value.is_empty() {
            return None;
        }
        let (num_str, unit) = value.split_at(value.len() - 1);
        let n: u64 = num_str.parse().ok()?;
        match unit {
            "n" => Some(Duration::from_nanos(n)),
            "u" => Some(Duration::from_micros(n)),
            "m" => Some(Duration::from_millis(n)),
            "S" => Some(Duration::from_secs(n)),
            "M" => Some(Duration::from_secs(n * 60)),
            "H" => Some(Duration::from_secs(n * 3600)),
            _ => None,
        }
    }

    /// Encode Duration as grpc-timeout header value
    pub fn encode_grpc_timeout(d: Duration) -> String {
        let millis = d.as_millis();
        if millis < 1000 {
            format!("{}m", millis)
        } else {
            format!("{}S", d.as_secs())
        }
    }

    /// Get the configured services map
    pub fn services(&self) -> &HashMap<String, String> {
        &self.services
    }

    /// Whether gRPC-Web is enabled
    pub fn grpc_web_enabled(&self) -> bool {
        self.enable_grpc_web
    }

    /// Whether deadline propagation is enabled
    pub fn deadline_propagation_enabled(&self) -> bool {
        self.deadline_propagation
    }

    /// Get max message size
    pub fn max_message_size(&self) -> usize {
        self.max_message_size
    }
}

/// Percent-encode a gRPC message for the grpc-message header (RFC 3986)
fn percent_encode_grpc_message(msg: &str) -> String {
    msg.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "-_.~".contains(c) || c == ' ' {
                c.to_string()
            } else {
                format!("%{:02X}", c as u32)
            }
        })
        .collect()
}

/// Headers to strip when forwarding gRPC requests to upstreams
const GRPC_HOP_BY_HOP_HEADERS: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "transfer-encoding",
    "upgrade",
];

/// Build upstream gRPC headers from the original request
pub fn build_grpc_upstream_headers(
    original_headers: &http::HeaderMap,
) -> http::HeaderMap {
    let mut headers = http::HeaderMap::new();

    for (key, value) in original_headers.iter() {
        let key_str = key.as_str();

        // Skip hop-by-hop headers
        if GRPC_HOP_BY_HOP_HEADERS.contains(&key_str) {
            continue;
        }

        // Forward everything else (grpc-*, custom metadata, content-type, authorization, etc.)
        headers.insert(key.clone(), value.clone());
    }

    // Ensure TE: trailers is set (required for gRPC over HTTP/2)
    headers.insert("te", "trailers".parse().unwrap());

    headers
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
        // This is the buffered-body path (for protocol handler trait compatibility).
        // The real gRPC proxy path uses handle_grpc_proxy in the runtime handler,
        // which operates on streaming Request<Incoming> bodies.
        //
        // If we get here, it means the request was buffered (unary RPC),
        // so we validate and return a proper error since routing happens in the runtime.

        if !Self::is_grpc_request(&req) {
            return Self::error_response(status_codes::UNIMPLEMENTED, "Not a valid gRPC request");
        }

        if req.method() != Method::POST {
            return Self::error_response(
                status_codes::UNIMPLEMENTED,
                "Only POST method is allowed for gRPC",
            );
        }

        let path = req.uri().path();
        let (service, method) = if let Some(parsed) = Self::parse_grpc_path(path) {
            parsed
        } else {
            warn!(path = %path, "Invalid gRPC path format");
            return Self::error_response(status_codes::UNIMPLEMENTED, "Invalid gRPC path format");
        };

        debug!(service = %service, method = %method, "gRPC request (buffered path)");

        // In the buffered path, this handler is a fallback.
        // The primary gRPC proxy path intercepts before body buffering.
        // Return UNIMPLEMENTED to indicate this path shouldn't normally be reached.
        Self::error_response(
            status_codes::INTERNAL,
            "gRPC request reached buffered handler; should be handled by streaming proxy",
        )
    }
}

/// gRPC status codes
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
    fn test_is_grpc_web() {
        let mut headers = http::HeaderMap::new();
        headers.insert(header::CONTENT_TYPE, "application/grpc-web+proto".parse().unwrap());
        assert!(GrpcHandler::is_grpc_web_content_type(&headers));

        let mut headers2 = http::HeaderMap::new();
        headers2.insert(header::CONTENT_TYPE, "application/grpc-web-text".parse().unwrap());
        assert!(GrpcHandler::is_grpc_web_content_type(&headers2));
    }

    #[test]
    fn test_parse_grpc_path() {
        assert_eq!(
            GrpcHandler::parse_grpc_path("/users.UserService/GetUser"),
            Some(("users.UserService".to_string(), "GetUser".to_string()))
        );

        assert_eq!(GrpcHandler::parse_grpc_path("/invalid"), None);
        assert_eq!(GrpcHandler::parse_grpc_path("/a/b/c"), None);
        assert_eq!(GrpcHandler::parse_grpc_path("//"), None);
    }

    #[test]
    fn test_parse_grpc_timeout() {
        assert_eq!(
            GrpcHandler::parse_grpc_timeout("100m"),
            Some(Duration::from_millis(100))
        );
        assert_eq!(
            GrpcHandler::parse_grpc_timeout("5S"),
            Some(Duration::from_secs(5))
        );
        assert_eq!(
            GrpcHandler::parse_grpc_timeout("1M"),
            Some(Duration::from_secs(60))
        );
        assert_eq!(
            GrpcHandler::parse_grpc_timeout("2H"),
            Some(Duration::from_secs(7200))
        );
        assert_eq!(
            GrpcHandler::parse_grpc_timeout("500u"),
            Some(Duration::from_micros(500))
        );
        assert_eq!(GrpcHandler::parse_grpc_timeout(""), None);
        assert_eq!(GrpcHandler::parse_grpc_timeout("abc"), None);
    }

    #[test]
    fn test_encode_grpc_timeout() {
        assert_eq!(GrpcHandler::encode_grpc_timeout(Duration::from_millis(100)), "100m");
        assert_eq!(GrpcHandler::encode_grpc_timeout(Duration::from_secs(5)), "5S");
    }

    #[test]
    fn test_service_resolution() {
        let mut services = HashMap::new();
        services.insert("users.UserService".to_string(), "user-upstream".to_string());
        let handler = GrpcHandler {
            services,
            ..GrpcHandler::new()
        };

        assert_eq!(handler.resolve_upstream("users.UserService"), Some("user-upstream"));
        assert_eq!(handler.resolve_upstream("unknown.Service"), None);
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

    #[test]
    fn test_build_grpc_upstream_headers() {
        let mut headers = http::HeaderMap::new();
        headers.insert(header::CONTENT_TYPE, "application/grpc".parse().unwrap());
        headers.insert("grpc-timeout", "5S".parse().unwrap());
        headers.insert("authorization", "Bearer token".parse().unwrap());
        headers.insert("x-custom-meta", "value".parse().unwrap());
        headers.insert("connection", "keep-alive".parse().unwrap()); // Should be stripped

        let upstream = build_grpc_upstream_headers(&headers);

        assert!(upstream.contains_key("content-type"));
        assert!(upstream.contains_key("grpc-timeout"));
        assert!(upstream.contains_key("authorization"));
        assert!(upstream.contains_key("x-custom-meta"));
        assert!(!upstream.contains_key("connection")); // Stripped
        assert_eq!(upstream.get("te").unwrap(), "trailers");
    }
}
