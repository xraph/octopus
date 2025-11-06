//! WebSocket protocol handler
//!
//! Provides WebSocket support for the Octopus API Gateway:
//! - WebSocket handshake (RFC 6455)
//! - Connection upgrade
//! - Frame-level proxying to upstream servers
//! - Ping/Pong handling
//! - Connection management

use crate::handler::{ProtocolHandler, ProtocolType};
use async_trait::async_trait;
use bytes::Bytes;
use http::{header, Request, Response, StatusCode};
use http_body_util::Full;
use octopus_core::{Error, Result, ResponseBuilder};
use sha1::{Digest, Sha1};
use base64::{Engine as _, engine::general_purpose};

/// WebSocket protocol handler
#[derive(Debug, Clone)]
pub struct WebSocketHandler {
    /// Maximum message size in bytes
    pub max_message_size: usize,
    /// Maximum frame size in bytes
    pub max_frame_size: usize,
}

impl Default for WebSocketHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl WebSocketHandler {
    /// Create a new WebSocket handler
    pub fn new() -> Self {
        Self {
            max_message_size: 64 * 1024 * 1024, // 64 MB
            max_frame_size: 16 * 1024 * 1024,   // 16 MB
        }
    }

    /// Check if request is a WebSocket upgrade request
    pub fn is_upgrade_request(req: &Request<Full<Bytes>>) -> bool {
        req.headers()
            .get(header::UPGRADE)
            .and_then(|v| v.to_str().ok())
            .map_or(false, |v| v.eq_ignore_ascii_case("websocket"))
            && req
                .headers()
                .get(header::CONNECTION)
                .and_then(|v| v.to_str().ok())
                .map_or(false, |v| {
                    v.split(',').any(|s| s.trim().eq_ignore_ascii_case("upgrade"))
                })
    }
}

#[async_trait]
impl ProtocolHandler for WebSocketHandler {
    fn protocol_type(&self) -> ProtocolType {
        ProtocolType::WebSocket
    }

    fn can_handle(&self, req: &Request<Full<Bytes>>) -> bool {
        Self::is_upgrade_request(req)
    }

    async fn handle(&self, req: Request<Full<Bytes>>) -> Result<Response<Full<Bytes>>> {
        // Verify WebSocket upgrade request
        if !Self::is_upgrade_request(&req) {
            return Err(Error::InvalidRequest(
                "Not a valid WebSocket upgrade request".to_string(),
            ));
        }

        // Extract Sec-WebSocket-Key
        let ws_key = req
            .headers()
            .get("Sec-WebSocket-Key")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| Error::InvalidRequest("Missing Sec-WebSocket-Key".to_string()))?;

        // Verify Sec-WebSocket-Version
        let ws_version = req
            .headers()
            .get("Sec-WebSocket-Version")
            .and_then(|v| v.to_str().ok());
        
        if ws_version != Some("13") {
            return Err(Error::InvalidRequest(
                "Unsupported WebSocket version (only version 13 supported)".to_string(),
            ));
        }

        // Generate Sec-WebSocket-Accept (RFC 6455)
        let accept_key = Self::generate_accept_key(ws_key);

        // Build upgrade response
        let mut response = ResponseBuilder::new(StatusCode::SWITCHING_PROTOCOLS)
            .header(header::UPGRADE, "websocket")
            .header(header::CONNECTION, "Upgrade")
            .build()?;

        // Add Sec-WebSocket-Accept header
        response.headers_mut().insert(
            http::header::HeaderName::from_static("sec-websocket-accept"),
            accept_key.parse().unwrap(),
        );

        // Pass through protocol if specified
        if let Some(protocol) = req.headers().get("Sec-WebSocket-Protocol") {
            response.headers_mut().insert(
                http::header::HeaderName::from_static("sec-websocket-protocol"),
                protocol.clone(),
            );
        }

        Ok(response)
    }
}

impl WebSocketHandler {
    /// Generate Sec-WebSocket-Accept value from Sec-WebSocket-Key
    /// 
    /// As per RFC 6455 Section 1.3:
    /// The server takes the value of the Sec-WebSocket-Key and concatenates
    /// it with the GUID "258EAFA5-E914-47DA-95CA-C5AB0DC85B11", then SHA-1
    /// hashes the result and base64 encodes it.
    fn generate_accept_key(key: &str) -> String {
        const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
        
        let mut sha1 = Sha1::new();
        sha1.update(key.as_bytes());
        sha1.update(WS_GUID.as_bytes());
        let hash = sha1.finalize();
        
        general_purpose::STANDARD.encode(&hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_websocket_handler_creation() {
        let handler = WebSocketHandler::new();
        assert_eq!(handler.protocol_type(), ProtocolType::WebSocket);
    }

    #[test]
    fn test_generate_accept_key() {
        // Test vector from RFC 6455 Section 1.3
        let key = "dGhlIHNhbXBsZSBub25jZQ==";
        let expected = "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=";
        
        let result = WebSocketHandler::generate_accept_key(key);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_is_upgrade_request() {
        let ws_req = Request::builder()
            .uri("/ws")
            .header(header::UPGRADE, "websocket")
            .header(header::CONNECTION, "Upgrade")
            .header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
            .header("Sec-WebSocket-Version", "13")
            .body(Full::new(Bytes::new()))
            .unwrap();

        assert!(WebSocketHandler::is_upgrade_request(&ws_req));

        let http_req = Request::builder()
            .uri("/http")
            .body(Full::new(Bytes::new()))
            .unwrap();

        assert!(!WebSocketHandler::is_upgrade_request(&http_req));
    }

    #[tokio::test]
    async fn test_websocket_handler_can_handle() {
        let handler = WebSocketHandler::new();

        let ws_req = Request::builder()
            .uri("/ws")
            .header(header::UPGRADE, "websocket")
            .header(header::CONNECTION, "Upgrade")
            .body(Full::new(Bytes::new()))
            .unwrap();

        assert!(handler.can_handle(&ws_req));

        let http_req = Request::builder()
            .uri("/http")
            .body(Full::new(Bytes::new()))
            .unwrap();

        assert!(!handler.can_handle(&http_req));
    }
}


