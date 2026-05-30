//! Observability integration tests - tracing, metrics, audit logging

use super::*;
use bytes::Bytes;
use http::{HeaderValue, Method};
use octopus_proxy::{AuditEvent, AuditEventType, AuditLogger, TraceContext};
use std::net::IpAddr;

#[tokio::test]
async fn test_trace_context_extraction() {
    // Create a request with W3C traceparent header
    let mut req = TestFixtures::request()
        .method(Method::GET)
        .uri("/api/test")
        .body(Bytes::new())
        .build();

    req.headers_mut().insert(
        "traceparent",
        HeaderValue::from_static("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"),
    );

    // Extract trace context
    let trace_ctx = TraceContext::from_headers(req.headers());

    assert!(trace_ctx.is_some(), "Should extract trace context");
    let ctx = trace_ctx.unwrap();

    // Verify trace ID was extracted
    assert!(!ctx.trace_id.is_empty(), "Trace ID should be extracted");
    assert_eq!(ctx.trace_id.len(), 32, "Trace ID should be 32 hex chars");
}

#[tokio::test]
async fn test_trace_context_generation() {
    // Generate a new trace context
    let trace_ctx = TraceContext::new_root();

    // Verify it has valid IDs
    assert!(!trace_ctx.trace_id.is_empty(), "Should have trace ID");
    assert_eq!(
        trace_ctx.trace_id.len(),
        32,
        "Trace ID should be 32 hex chars"
    );
    assert!(!trace_ctx.parent_span_id.is_empty(), "Should have span ID");
    assert_eq!(
        trace_ctx.parent_span_id.len(),
        16,
        "Span ID should be 16 hex chars"
    );
}

#[tokio::test]
async fn test_trace_context_propagation() {
    let mut req = TestFixtures::request()
        .method(Method::GET)
        .uri("/api/test")
        .body(Bytes::new())
        .build();

    req.headers_mut().insert(
        "traceparent",
        HeaderValue::from_static("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"),
    );

    // Extract existing context
    let parent_ctx = TraceContext::from_headers(req.headers()).unwrap();

    // Verify extracted values
    assert_eq!(parent_ctx.trace_id, "4bf92f3577b34da6a3ce929d0e0e4736");
    assert_eq!(parent_ctx.parent_span_id, "00f067aa0ba902b7");
    assert_eq!(parent_ctx.trace_flags, "01");
}

#[tokio::test]
async fn test_trace_context_to_headers() {
    let trace_ctx = TraceContext::new_root();

    // Inject into headers
    let mut headers = http::HeaderMap::new();
    trace_ctx.inject_into_headers(&mut headers);

    // Should have traceparent header
    assert!(
        headers.contains_key("traceparent"),
        "Should have traceparent header"
    );

    let traceparent = headers.get("traceparent").unwrap();
    assert!(
        traceparent.to_str().unwrap().starts_with("00-"),
        "Should start with version 00"
    );
}

#[tokio::test]
async fn test_audit_logger_request_received() {
    let logger = AuditLogger::new();

    // Create event using builder pattern
    let event = AuditEvent::new(AuditEventType::RequestReceived)
        .with_request_id("req-123".to_string())
        .with_client_ip("192.168.1.100".parse::<IpAddr>().unwrap())
        .with_uri("/api/users".to_string())
        .with_method("GET".to_string());

    logger.log(&event);

    // Audit logging writes to tracing, which we can't easily verify in tests
    // But we can verify it doesn't panic
}

#[tokio::test]
async fn test_audit_logger_auth_success() {
    let logger = AuditLogger::new();

    let event = AuditEvent::new(AuditEventType::AuthenticationSuccess)
        .with_request_id("req-124".to_string())
        .with_client_ip("192.168.1.100".parse().unwrap())
        .with_uri("/api/login".to_string())
        .with_method("POST".to_string())
        .with_status_code(200)
        .with_user_id("user-456".to_string());

    logger.log(&event);
}

#[tokio::test]
async fn test_audit_logger_auth_failure() {
    let logger = AuditLogger::new();

    let event = AuditEvent::new(AuditEventType::AuthenticationFailure)
        .with_request_id("req-125".to_string())
        .with_client_ip("192.168.1.100".parse().unwrap())
        .with_uri("/api/login".to_string())
        .with_method("POST".to_string())
        .with_status_code(401)
        .with_error("Invalid credentials".to_string());

    logger.log(&event);
}

#[tokio::test]
async fn test_audit_logger_rate_limit_exceeded() {
    let logger = AuditLogger::new();

    let event = AuditEvent::new(AuditEventType::RateLimitExceeded)
        .with_request_id("req-126".to_string())
        .with_client_ip("192.168.1.100".parse().unwrap())
        .with_uri("/api/data".to_string())
        .with_method("GET".to_string())
        .with_status_code(429)
        .with_user_id("user-789".to_string());

    logger.log(&event);
}

#[tokio::test]
async fn test_audit_logger_upstream_error() {
    let logger = AuditLogger::new();

    let event = AuditEvent::new(AuditEventType::UpstreamConnectionFailed)
        .with_request_id("req-127".to_string())
        .with_client_ip("192.168.1.100".parse().unwrap())
        .with_uri("/api/service".to_string())
        .with_method("GET".to_string())
        .with_status_code(502)
        .with_error("Connection refused".to_string());

    logger.log(&event);
}

#[tokio::test]
async fn test_audit_event_serialization() {
    let event = AuditEvent::new(AuditEventType::RequestReceived)
        .with_request_id("req-128".to_string())
        .with_client_ip("10.0.0.1".parse().unwrap())
        .with_uri("/api/test".to_string())
        .with_method("POST".to_string())
        .with_status_code(200)
        .with_user_id("test-user".to_string());

    // Serialize to JSON
    let json = serde_json::to_string(&event);
    assert!(json.is_ok(), "Should serialize to JSON");

    let json_str = json.unwrap();
    assert!(json_str.contains("req-128"), "Should contain request ID");
    assert!(json_str.contains("10.0.0.1"), "Should contain client IP");
}

#[tokio::test]
async fn test_multiple_audit_events() {
    let logger = AuditLogger::new();

    // Log multiple events
    for i in 0..10 {
        let event = AuditEvent::new(AuditEventType::RequestReceived)
            .with_request_id(format!("req-{i}"))
            .with_client_ip(format!("192.168.1.{}", i + 1).parse().unwrap())
            .with_uri("/api/test".to_string())
            .with_method("GET".to_string());

        logger.log(&event);
    }

    // Should not panic with multiple rapid log calls
}

#[tokio::test]
async fn test_trace_context_without_header() {
    let req = TestFixtures::request()
        .method(Method::GET)
        .uri("/api/test")
        .body(Bytes::new())
        .build();

    // Try to extract trace context (should be None or generate new)
    let trace_ctx = TraceContext::from_headers(req.headers());

    // Should return None when no header exists
    assert!(
        trace_ctx.is_none(),
        "Should return None without traceparent header"
    );
}

#[tokio::test]
async fn test_trace_context_invalid_header() {
    let mut req = TestFixtures::request()
        .method(Method::GET)
        .uri("/api/test")
        .body(Bytes::new())
        .build();

    // Add invalid traceparent header
    req.headers_mut()
        .insert("traceparent", HeaderValue::from_static("invalid-format"));

    // Should handle invalid header gracefully (return None)
    let trace_ctx = TraceContext::from_headers(req.headers());

    // Should return None for invalid format
    assert!(
        trace_ctx.is_none(),
        "Should return None for invalid traceparent"
    );
}

#[tokio::test]
async fn test_audit_event_with_upstream() {
    let event = AuditEvent::new(AuditEventType::RequestForwarded)
        .with_request_id("req-129".to_string())
        .with_upstream_id("upstream-1".to_string())
        .with_uri("/api/service".to_string())
        .with_method("POST".to_string());

    // Verify upstream_id is set
    assert_eq!(event.upstream_id, Some("upstream-1".to_string()));
}

#[tokio::test]
async fn test_audit_event_with_session() {
    let event = AuditEvent::new(AuditEventType::AuthenticationSuccess)
        .with_request_id("req-130".to_string())
        .with_session_id("session-abc123".to_string())
        .with_user_id("user-456".to_string());

    // Verify session_id is set
    assert_eq!(event.session_id, Some("session-abc123".to_string()));
}

#[tokio::test]
async fn test_audit_event_circuit_breaker() {
    let logger = AuditLogger::new();

    let event = AuditEvent::new(AuditEventType::CircuitBreakerOpened)
        .with_upstream_id("upstream-2".to_string())
        .with_error("Too many failures".to_string());

    logger.log(&event);
}

#[tokio::test]
async fn test_audit_event_tls_failure() {
    let logger = AuditLogger::new();

    let event = AuditEvent::new(AuditEventType::TlsHandshakeFailed)
        .with_upstream_id("upstream-3".to_string())
        .with_client_ip("192.168.1.50".parse().unwrap())
        .with_error("Certificate verification failed".to_string());

    logger.log(&event);
}

#[tokio::test]
async fn test_audit_event_timeout() {
    let logger = AuditLogger::new();

    let event = AuditEvent::new(AuditEventType::RequestTimeout)
        .with_request_id("req-131".to_string())
        .with_uri("/api/slow".to_string())
        .with_upstream_id("upstream-4".to_string())
        .with_error("Request timeout after 30s".to_string());

    logger.log(&event);
}

#[tokio::test]
async fn test_audit_event_invalid_request() {
    let logger = AuditLogger::new();

    let event = AuditEvent::new(AuditEventType::InvalidRequest)
        .with_request_id("req-132".to_string())
        .with_client_ip("192.168.1.100".parse().unwrap())
        .with_error("Body size exceeds limit".to_string());

    logger.log(&event);
}

#[tokio::test]
async fn test_audit_event_minimal() {
    let logger = AuditLogger::new();

    // Create minimal event with just type
    let event = AuditEvent::new(AuditEventType::RequestReceived);

    // Should work with minimal fields
    logger.log(&event);

    // Verify auto-generated fields exist
    assert!(
        !event.event_id.is_empty(),
        "Should have auto-generated event ID"
    );
}

#[tokio::test]
async fn test_audit_event_security_violation() {
    let logger = AuditLogger::new();

    let event = AuditEvent::new(AuditEventType::SecurityViolation)
        .with_request_id("req-133".to_string())
        .with_client_ip("192.168.1.200".parse().unwrap())
        .with_uri("/admin/sensitive".to_string())
        .with_error("Unauthorized access attempt".to_string());

    logger.log(&event);
}

#[tokio::test]
async fn test_audit_event_authorization() {
    let logger = AuditLogger::new();

    let denied = AuditEvent::new(AuditEventType::AuthorizationDenied)
        .with_request_id("req-134".to_string())
        .with_user_id("user-123".to_string())
        .with_uri("/api/admin".to_string());

    logger.log(&denied);

    let granted = AuditEvent::new(AuditEventType::AuthorizationGranted)
        .with_request_id("req-135".to_string())
        .with_user_id("admin-456".to_string())
        .with_uri("/api/admin".to_string());

    logger.log(&granted);
}
