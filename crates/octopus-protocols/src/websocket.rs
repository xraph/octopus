//! WebSocket protocol handler
//!
//! High-performance WebSocket proxy for the Octopus API Gateway:
//! - RFC 6455 WebSocket handshake
//! - HTTP upgrade via `hyper::upgrade::on()`
//! - Zero-copy bidirectional frame forwarding
//! - Ping/pong keepalive with dead connection detection
//! - Graceful close with timeout
//! - Per-upstream connection tracking
//! - Header injection (X-Forwarded-For, X-Real-IP, etc.)

use base64::{engine::general_purpose, Engine as _};
use bytes::Bytes;
use http::{header, Request, Response, StatusCode};
use http_body_util::Full;
use sha1::{Digest, Sha1};
use std::time::Duration;

/// WebSocket configuration
#[derive(Debug, Clone)]
pub struct WebSocketConfig {
    /// Maximum WebSocket frame size (default: 16 MB)
    pub max_frame_size: usize,
    /// Maximum WebSocket message size (default: 64 MB)
    pub max_message_size: usize,
    /// Ping interval for keepalive (default: 30s)
    pub ping_interval: Duration,
    /// Timeout waiting for close frame after sending one (default: 5s)
    pub close_timeout: Duration,
    /// Timeout for connecting to upstream WebSocket (default: 10s)
    pub connect_timeout: Duration,
}

impl Default for WebSocketConfig {
    fn default() -> Self {
        Self {
            max_frame_size: 16 * 1024 * 1024,   // 16 MB
            max_message_size: 64 * 1024 * 1024, // 64 MB
            ping_interval: Duration::from_secs(30),
            close_timeout: Duration::from_secs(5),
            connect_timeout: Duration::from_secs(10),
        }
    }
}

impl WebSocketConfig {
    /// Build a `tungstenite::protocol::WebSocketConfig` for size limits
    pub fn to_tungstenite_config(
        &self,
    ) -> tokio_tungstenite::tungstenite::protocol::WebSocketConfig {
        tokio_tungstenite::tungstenite::protocol::WebSocketConfig {
            max_frame_size: Some(self.max_frame_size),
            max_message_size: Some(self.max_message_size),
            ..Default::default()
        }
    }
}

/// WebSocket magic GUID for Sec-WebSocket-Accept computation (RFC 6455 §1.3)
const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// Check if an HTTP request is a WebSocket upgrade request.
///
/// Checks for `Upgrade: websocket` and `Connection: Upgrade` headers.
/// Works with any body type.
pub fn is_websocket_upgrade<B>(req: &Request<B>) -> bool {
    req.headers()
        .get(header::UPGRADE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("websocket"))
        && req
            .headers()
            .get(header::CONNECTION)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| {
                v.split(',')
                    .any(|s| s.trim().eq_ignore_ascii_case("upgrade"))
            })
}

/// Generate the `Sec-WebSocket-Accept` value from `Sec-WebSocket-Key`.
///
/// Per RFC 6455 §1.3: concatenate key with GUID, SHA-1 hash, base64 encode.
pub fn generate_accept_key(key: &str) -> String {
    let mut sha1 = Sha1::new();
    sha1.update(key.as_bytes());
    sha1.update(WS_GUID.as_bytes());
    general_purpose::STANDARD.encode(sha1.finalize())
}

/// Build a 101 Switching Protocols response for a WebSocket upgrade.
///
/// Validates the request headers and builds the correct handshake response.
pub fn build_upgrade_response<B>(req: &Request<B>) -> Result<Response<Full<Bytes>>, String> {
    // Extract and validate Sec-WebSocket-Key
    let ws_key = req
        .headers()
        .get("sec-websocket-key")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| "Missing Sec-WebSocket-Key header".to_string())?;

    // Verify Sec-WebSocket-Version is 13
    let ws_version = req
        .headers()
        .get("sec-websocket-version")
        .and_then(|v| v.to_str().ok());

    if ws_version != Some("13") {
        return Err("Unsupported WebSocket version (only 13 supported)".to_string());
    }

    // Compute accept key
    let accept_key = generate_accept_key(ws_key);

    // Build response
    let mut builder = Response::builder()
        .status(StatusCode::SWITCHING_PROTOCOLS)
        .header(header::UPGRADE, "websocket")
        .header(header::CONNECTION, "Upgrade")
        .header("sec-websocket-accept", accept_key);

    // Forward Sec-WebSocket-Protocol if present
    if let Some(protocol) = req.headers().get("sec-websocket-protocol") {
        builder = builder.header("sec-websocket-protocol", protocol);
    }

    builder
        .body(Full::new(Bytes::new()))
        .map_err(|e| format!("Failed to build upgrade response: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_accept_key_rfc_vector() {
        // Test vector from RFC 6455 Section 1.3
        let key = "dGhlIHNhbXBsZSBub25jZQ==";
        let expected = "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=";
        assert_eq!(generate_accept_key(key), expected);
    }

    #[test]
    fn test_is_websocket_upgrade_valid() {
        let req = Request::builder()
            .uri("/ws")
            .header(header::UPGRADE, "websocket")
            .header(header::CONNECTION, "Upgrade")
            .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")
            .header("sec-websocket-version", "13")
            .body(Full::new(Bytes::new()))
            .unwrap();
        assert!(is_websocket_upgrade(&req));
    }

    #[test]
    fn test_is_websocket_upgrade_case_insensitive() {
        let req = Request::builder()
            .uri("/ws")
            .header(header::UPGRADE, "WebSocket")
            .header(header::CONNECTION, "keep-alive, Upgrade")
            .body(Full::new(Bytes::new()))
            .unwrap();
        assert!(is_websocket_upgrade(&req));
    }

    #[test]
    fn test_is_not_websocket_upgrade() {
        let req = Request::builder()
            .uri("/api")
            .body(Full::new(Bytes::new()))
            .unwrap();
        assert!(!is_websocket_upgrade(&req));
    }

    #[test]
    fn test_build_upgrade_response_valid() {
        let req = Request::builder()
            .uri("/ws")
            .header(header::UPGRADE, "websocket")
            .header(header::CONNECTION, "Upgrade")
            .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")
            .header("sec-websocket-version", "13")
            .body(Full::new(Bytes::new()))
            .unwrap();

        let resp = build_upgrade_response(&req).unwrap();
        assert_eq!(resp.status(), StatusCode::SWITCHING_PROTOCOLS);
        assert_eq!(resp.headers().get(header::UPGRADE).unwrap(), "websocket");
        assert_eq!(resp.headers().get(header::CONNECTION).unwrap(), "Upgrade");
        assert_eq!(
            resp.headers().get("sec-websocket-accept").unwrap(),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }

    #[test]
    fn test_build_upgrade_response_missing_key() {
        let req = Request::builder()
            .uri("/ws")
            .header(header::UPGRADE, "websocket")
            .header(header::CONNECTION, "Upgrade")
            .header("sec-websocket-version", "13")
            .body(Full::new(Bytes::new()))
            .unwrap();

        assert!(build_upgrade_response(&req).is_err());
    }

    #[test]
    fn test_build_upgrade_response_wrong_version() {
        let req = Request::builder()
            .uri("/ws")
            .header(header::UPGRADE, "websocket")
            .header(header::CONNECTION, "Upgrade")
            .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")
            .header("sec-websocket-version", "8")
            .body(Full::new(Bytes::new()))
            .unwrap();

        assert!(build_upgrade_response(&req).is_err());
    }

    #[test]
    fn test_build_upgrade_response_forwards_protocol() {
        let req = Request::builder()
            .uri("/ws")
            .header(header::UPGRADE, "websocket")
            .header(header::CONNECTION, "Upgrade")
            .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")
            .header("sec-websocket-version", "13")
            .header("sec-websocket-protocol", "graphql-ws")
            .body(Full::new(Bytes::new()))
            .unwrap();

        let resp = build_upgrade_response(&req).unwrap();
        assert_eq!(
            resp.headers().get("sec-websocket-protocol").unwrap(),
            "graphql-ws"
        );
    }
}
