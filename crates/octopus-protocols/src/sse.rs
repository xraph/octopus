//! Server-Sent Events (SSE) protocol utilities
//!
//! Detection and formatting helpers for SSE streams.
//! The actual SSE proxying is handled in `octopus-runtime/src/handler.rs`
//! via `handle_sse_proxy()` which streams the upstream `Incoming` body
//! directly to the client without buffering.

use http::{header, Request};

/// Check if a request is for an SSE stream.
///
/// Returns true if the `Accept` header contains `text/event-stream`.
pub fn is_sse_request<B>(req: &Request<B>) -> bool {
    req.headers()
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains("text/event-stream"))
}

/// Format an SSE event message.
///
/// Returns `event: {event}\ndata: {data}\n\n`
#[must_use]
pub fn format_event(event: &str, data: &str) -> String {
    format!("event: {event}\ndata: {data}\n\n")
}

/// Format an SSE data-only message (no event name).
///
/// Returns `data: {data}\n\n`
#[must_use]
pub fn format_data(data: &str) -> String {
    format!("data: {data}\n\n")
}

/// Format an SSE comment (used for keepalive).
///
/// Returns `: {comment}\n\n`
#[must_use]
pub fn format_comment(comment: &str) -> String {
    format!(": {comment}\n\n")
}

/// Format an SSE retry directive.
///
/// Returns `retry: {ms}\n\n`
#[must_use]
pub fn format_retry(milliseconds: u64) -> String {
    format!("retry: {milliseconds}\n\n")
}

/// Format an SSE event with ID.
///
/// Returns `id: {id}\nevent: {event}\ndata: {data}\n\n`
#[must_use]
pub fn format_event_with_id(id: &str, event: &str, data: &str) -> String {
    format!("id: {id}\nevent: {event}\ndata: {data}\n\n")
}

/// SSE-specific headers that should be set on streaming responses
pub const SSE_CONTENT_TYPE: &str = "text/event-stream";
/// Cache-Control value for SSE responses
pub const SSE_CACHE_CONTROL: &str = "no-cache";

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::Full;

    #[test]
    fn test_is_sse_request_true() {
        let req = Request::builder()
            .uri("/events")
            .header(header::ACCEPT, "text/event-stream")
            .body(Full::new(Bytes::new()))
            .unwrap();
        assert!(is_sse_request(&req));
    }

    #[test]
    fn test_is_sse_request_mixed_accept() {
        let req = Request::builder()
            .uri("/events")
            .header(header::ACCEPT, "text/event-stream, application/json")
            .body(Full::new(Bytes::new()))
            .unwrap();
        assert!(is_sse_request(&req));
    }

    #[test]
    fn test_is_sse_request_false() {
        let req = Request::builder()
            .uri("/api")
            .header(header::ACCEPT, "application/json")
            .body(Full::new(Bytes::new()))
            .unwrap();
        assert!(!is_sse_request(&req));
    }

    #[test]
    fn test_is_sse_request_no_accept() {
        let req = Request::builder()
            .uri("/api")
            .body(Full::new(Bytes::new()))
            .unwrap();
        assert!(!is_sse_request(&req));
    }

    #[test]
    fn test_format_event() {
        assert_eq!(
            format_event("update", r#"{"id":1}"#),
            "event: update\ndata: {\"id\":1}\n\n"
        );
    }

    #[test]
    fn test_format_data() {
        assert_eq!(format_data("hello"), "data: hello\n\n");
    }

    #[test]
    fn test_format_comment() {
        assert_eq!(format_comment("keepalive"), ": keepalive\n\n");
    }

    #[test]
    fn test_format_retry() {
        assert_eq!(format_retry(3000), "retry: 3000\n\n");
    }

    #[test]
    fn test_format_event_with_id() {
        assert_eq!(
            format_event_with_id("42", "message", "hello"),
            "id: 42\nevent: message\ndata: hello\n\n"
        );
    }
}
