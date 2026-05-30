//! Distributed tracing with W3C Trace Context propagation

use http::{HeaderMap, HeaderValue, Request, Response};
use std::fmt;
use tracing::{debug, warn};

/// W3C Trace Context traceparent header name
pub const TRACEPARENT_HEADER: &str = "traceparent";
/// W3C Trace Context tracestate header name
pub const TRACESTATE_HEADER: &str = "tracestate";

/// W3C Trace Context version
const TRACE_CONTEXT_VERSION: &str = "00";

/// Trace context for distributed tracing
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceContext {
    /// Trace ID (32 hex characters)
    pub trace_id: String,

    /// Parent span ID (16 hex characters)
    pub parent_span_id: String,

    /// Trace flags (2 hex characters)
    pub trace_flags: String,

    /// Trace state (optional)
    pub trace_state: Option<String>,
}

impl TraceContext {
    /// Create a new trace context
    pub fn new(trace_id: String, parent_span_id: String) -> Self {
        Self {
            trace_id,
            parent_span_id,
            trace_flags: "01".to_string(), // Sampled
            trace_state: None,
        }
    }

    /// Create a new root trace context with generated IDs
    pub fn new_root() -> Self {
        Self {
            trace_id: generate_trace_id(),
            parent_span_id: generate_span_id(),
            trace_flags: "01".to_string(),
            trace_state: None,
        }
    }

    /// Parse trace context from HTTP headers
    pub fn from_headers(headers: &HeaderMap) -> Option<Self> {
        let traceparent = headers.get(TRACEPARENT_HEADER)?;
        let traceparent_str = traceparent.to_str().ok()?;

        Self::parse_traceparent(traceparent_str).map(|mut ctx| {
            // Also extract tracestate if present
            if let Some(tracestate) = headers.get(TRACESTATE_HEADER) {
                if let Ok(tracestate_str) = tracestate.to_str() {
                    ctx.trace_state = Some(tracestate_str.to_string());
                }
            }
            ctx
        })
    }

    /// Parse traceparent header value
    /// Format: version-trace_id-parent_span_id-trace_flags
    /// Example: 00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01
    fn parse_traceparent(value: &str) -> Option<Self> {
        let parts: Vec<&str> = value.split('-').collect();

        if parts.len() != 4 {
            warn!(
                "Invalid traceparent format: expected 4 parts, got {}",
                parts.len()
            );
            return None;
        }

        let version = parts[0];
        if version != TRACE_CONTEXT_VERSION {
            warn!("Unsupported trace context version: {}", version);
            return None;
        }

        let trace_id = parts[1];
        let parent_span_id = parts[2];
        let trace_flags = parts[3];

        // Validate trace_id (32 hex chars)
        if trace_id.len() != 32 || !trace_id.chars().all(|c| c.is_ascii_hexdigit()) {
            warn!("Invalid trace_id: {}", trace_id);
            return None;
        }

        // Validate parent_span_id (16 hex chars)
        if parent_span_id.len() != 16 || !parent_span_id.chars().all(|c| c.is_ascii_hexdigit()) {
            warn!("Invalid parent_span_id: {}", parent_span_id);
            return None;
        }

        // Validate trace_flags (2 hex chars)
        if trace_flags.len() != 2 || !trace_flags.chars().all(|c| c.is_ascii_hexdigit()) {
            warn!("Invalid trace_flags: {}", trace_flags);
            return None;
        }

        Some(Self {
            trace_id: trace_id.to_string(),
            parent_span_id: parent_span_id.to_string(),
            trace_flags: trace_flags.to_string(),
            trace_state: None,
        })
    }

    /// Create a child span context
    pub fn create_child(&self) -> Self {
        Self {
            trace_id: self.trace_id.clone(),
            parent_span_id: generate_span_id(),
            trace_flags: self.trace_flags.clone(),
            trace_state: self.trace_state.clone(),
        }
    }

    /// Convert to traceparent header value
    pub fn to_traceparent(&self) -> String {
        format!(
            "{}-{}-{}-{}",
            TRACE_CONTEXT_VERSION, self.trace_id, self.parent_span_id, self.trace_flags
        )
    }

    /// Inject trace context into HTTP headers
    pub fn inject_into_headers(&self, headers: &mut HeaderMap) {
        let traceparent = self.to_traceparent();

        if let Ok(value) = HeaderValue::from_str(&traceparent) {
            headers.insert(TRACEPARENT_HEADER, value);
            debug!(trace_id = %self.trace_id, "Injected traceparent header");
        } else {
            warn!("Failed to create traceparent header value");
        }

        // Inject tracestate if present
        if let Some(ref tracestate) = self.trace_state {
            if let Ok(value) = HeaderValue::from_str(tracestate) {
                headers.insert(TRACESTATE_HEADER, value);
            }
        }
    }

    /// Check if trace is sampled
    pub fn is_sampled(&self) -> bool {
        // Check if the least significant bit is set
        self.trace_flags.ends_with('1')
            || self.trace_flags.ends_with('3')
            || self.trace_flags.ends_with('5')
            || self.trace_flags.ends_with('7')
            || self.trace_flags.ends_with('9')
            || self.trace_flags.ends_with('b')
            || self.trace_flags.ends_with('d')
            || self.trace_flags.ends_with('f')
    }

    /// Set sampled flag
    pub fn set_sampled(&mut self, sampled: bool) {
        if sampled {
            self.trace_flags = "01".to_string();
        } else {
            self.trace_flags = "00".to_string();
        }
    }
}

impl fmt::Display for TraceContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_traceparent())
    }
}

/// Extract or create trace context from a request
pub fn extract_or_create_trace_context<B>(req: &Request<B>) -> TraceContext {
    TraceContext::from_headers(req.headers()).unwrap_or_else(|| {
        debug!("No trace context found, creating new root context");
        TraceContext::new_root()
    })
}

/// Propagate trace context to upstream request
pub fn propagate_trace_context<B>(req: &mut Request<B>, context: &TraceContext) {
    let child_context = context.create_child();
    child_context.inject_into_headers(req.headers_mut());
}

/// Generate a random trace ID (32 hex characters)
fn generate_trace_id() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: [u8; 16] = rng.gen();
    hex::encode(bytes)
}

/// Generate a random span ID (16 hex characters)
fn generate_span_id() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: [u8; 8] = rng.gen();
    hex::encode(bytes)
}

/// Trace context middleware for automatic propagation
#[derive(Debug)]
pub struct TraceContextMiddleware;

impl TraceContextMiddleware {
    /// Process incoming request and extract/create trace context
    pub fn process_request<B>(req: &Request<B>) -> TraceContext {
        extract_or_create_trace_context(req)
    }

    /// Process outgoing request and inject trace context
    pub fn process_upstream_request<B>(req: &mut Request<B>, context: &TraceContext) {
        propagate_trace_context(req, context);
    }

    /// Process response (no-op for now, but can add response headers)
    pub fn process_response<B>(_resp: &mut Response<B>, _context: &TraceContext) {
        // Could add server-timing headers or other trace-related headers
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::Request;

    #[test]
    fn test_parse_valid_traceparent() {
        let traceparent = "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01";
        let context = TraceContext::parse_traceparent(traceparent).unwrap();

        assert_eq!(context.trace_id, "0af7651916cd43dd8448eb211c80319c");
        assert_eq!(context.parent_span_id, "b7ad6b7169203331");
        assert_eq!(context.trace_flags, "01");
        assert!(context.is_sampled());
    }

    #[test]
    fn test_parse_invalid_traceparent() {
        // Wrong number of parts
        assert!(TraceContext::parse_traceparent("00-abc-def").is_none());

        // Invalid version
        assert!(TraceContext::parse_traceparent(
            "99-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01"
        )
        .is_none());

        // Invalid trace_id length
        assert!(TraceContext::parse_traceparent("00-abc-b7ad6b7169203331-01").is_none());
    }

    #[test]
    fn test_create_child_context() {
        let parent = TraceContext::new(
            "0af7651916cd43dd8448eb211c80319c".to_string(),
            "b7ad6b7169203331".to_string(),
        );

        let child = parent.create_child();

        assert_eq!(child.trace_id, parent.trace_id);
        assert_ne!(child.parent_span_id, parent.parent_span_id);
        assert_eq!(child.trace_flags, parent.trace_flags);
    }

    #[test]
    fn test_to_traceparent() {
        let context = TraceContext::new(
            "0af7651916cd43dd8448eb211c80319c".to_string(),
            "b7ad6b7169203331".to_string(),
        );

        let traceparent = context.to_traceparent();
        assert_eq!(
            traceparent,
            "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01"
        );
    }

    #[test]
    fn test_inject_and_extract() {
        let context = TraceContext::new_root();
        let mut headers = HeaderMap::new();

        context.inject_into_headers(&mut headers);

        let extracted = TraceContext::from_headers(&headers).unwrap();
        assert_eq!(extracted.trace_id, context.trace_id);
        assert_eq!(extracted.parent_span_id, context.parent_span_id);
    }

    #[test]
    fn test_extract_or_create() {
        let req = Request::builder()
            .uri("http://example.com")
            .body(())
            .unwrap();

        let context = extract_or_create_trace_context(&req);
        assert!(!context.trace_id.is_empty());
        assert!(!context.parent_span_id.is_empty());
    }

    #[test]
    fn test_sampled_flag() {
        let mut context = TraceContext::new_root();

        context.set_sampled(true);
        assert!(context.is_sampled());

        context.set_sampled(false);
        assert!(!context.is_sampled());
    }

    #[test]
    fn test_generate_ids() {
        let trace_id1 = generate_trace_id();
        let trace_id2 = generate_trace_id();

        assert_eq!(trace_id1.len(), 32);
        assert_eq!(trace_id2.len(), 32);
        assert_ne!(trace_id1, trace_id2);

        let span_id1 = generate_span_id();
        let span_id2 = generate_span_id();

        assert_eq!(span_id1.len(), 16);
        assert_eq!(span_id2.len(), 16);
        assert_ne!(span_id1, span_id2);
    }
}
