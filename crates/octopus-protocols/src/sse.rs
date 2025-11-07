//! Server-Sent Events (SSE) protocol handler

use crate::handler::{ProtocolHandler, ProtocolType};
use async_trait::async_trait;
use bytes::Bytes;
use http::{header, Request, Response, StatusCode};
use http_body_util::Full;
use octopus_core::Result;

/// SSE protocol handler
#[derive(Debug, Clone)]
pub struct SseHandler {
    /// Heartbeat interval in seconds
    pub heartbeat_interval: u64,
}

impl Default for SseHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl SseHandler {
    /// Create a new SSE handler
    #[must_use] pub const fn new() -> Self {
        Self {
            heartbeat_interval: 30, // 30 seconds default
        }
    }

    /// Check if request accepts SSE
    pub fn accepts_sse(req: &Request<Full<Bytes>>) -> bool {
        req.headers()
            .get(header::ACCEPT)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.contains("text/event-stream"))
    }

    /// Format SSE message
    #[must_use] pub fn format_message(event: &str, data: &str) -> String {
        format!("event: {event}\ndata: {data}\n\n")
    }

    /// Format SSE comment (keepalive)
    #[must_use] pub fn format_comment(comment: &str) -> String {
        format!(": {comment}\n\n")
    }
}

#[async_trait]
impl ProtocolHandler for SseHandler {
    fn protocol_type(&self) -> ProtocolType {
        ProtocolType::Sse
    }

    fn can_handle(&self, req: &Request<Full<Bytes>>) -> bool {
        Self::accepts_sse(req)
    }

    async fn handle(&self, _req: Request<Full<Bytes>>) -> Result<Response<Full<Bytes>>> {
        // In a real implementation, this would:
        // 1. Set up streaming response
        // 2. Send periodic heartbeats
        // 3. Forward events from upstream

        let body = Self::format_comment("connected");

        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .header(header::CACHE_CONTROL, "no-cache")
            .header(header::CONNECTION, "keep-alive")
            .body(Full::new(Bytes::from(body)))
            .map_err(|e| octopus_core::Error::Internal(format!("Failed to build response: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sse_handler_creation() {
        let handler = SseHandler::new();
        assert_eq!(handler.protocol_type(), ProtocolType::Sse);
        assert_eq!(handler.heartbeat_interval, 30);
    }

    #[test]
    fn test_accepts_sse() {
        let sse_req = Request::builder()
            .uri("/events")
            .header(header::ACCEPT, "text/event-stream")
            .body(Full::new(Bytes::new()))
            .unwrap();

        assert!(SseHandler::accepts_sse(&sse_req));

        let http_req = Request::builder()
            .uri("/api")
            .header(header::ACCEPT, "application/json")
            .body(Full::new(Bytes::new()))
            .unwrap();

        assert!(!SseHandler::accepts_sse(&http_req));
    }

    #[test]
    fn test_format_message() {
        let msg = SseHandler::format_message("update", r#"{"id": 123}"#);
        assert_eq!(msg, "event: update\ndata: {\"id\": 123}\n\n");
    }

    #[test]
    fn test_format_comment() {
        let comment = SseHandler::format_comment("keepalive");
        assert_eq!(comment, ": keepalive\n\n");
    }

    #[tokio::test]
    async fn test_sse_handler_can_handle() {
        let handler = SseHandler::new();

        let sse_req = Request::builder()
            .uri("/events")
            .header(header::ACCEPT, "text/event-stream")
            .body(Full::new(Bytes::new()))
            .unwrap();

        assert!(handler.can_handle(&sse_req));
    }

    #[tokio::test]
    async fn test_sse_handler_response() {
        let handler = SseHandler::new();

        let req = Request::builder()
            .uri("/events")
            .header(header::ACCEPT, "text/event-stream")
            .body(Full::new(Bytes::new()))
            .unwrap();

        let response = handler.handle(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/event-stream"
        );
    }
}
