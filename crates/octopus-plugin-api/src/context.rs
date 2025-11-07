//! Plugin context types

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

/// Request context provided to plugins
#[derive(Debug, Clone)]
pub struct RequestContext {
    /// Unique request ID
    pub request_id: String,

    /// Remote client address
    pub remote_addr: SocketAddr,

    /// Request start time
    pub start_time: Instant,

    /// Route name (if matched)
    pub route: Option<String>,

    /// Upstream name (if selected)
    pub upstream: Option<String>,

    /// Custom metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

impl RequestContext {
    /// Create a new request context
    pub fn new(request_id: String, remote_addr: SocketAddr) -> Self {
        Self {
            request_id,
            remote_addr,
            start_time: Instant::now(),
            route: None,
            upstream: None,
            metadata: HashMap::new(),
        }
    }

    /// Get elapsed time since request start
    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }

    /// Set metadata value
    pub fn set_metadata(&mut self, key: String, value: serde_json::Value) {
        self.metadata.insert(key, value);
    }

    /// Get metadata value
    pub fn get_metadata(&self, key: &str) -> Option<&serde_json::Value> {
        self.metadata.get(key)
    }
}

/// Response context provided to plugins
#[derive(Debug, Clone)]
pub struct ResponseContext {
    /// Request ID (from request context)
    pub request_id: String,

    /// Total request duration
    pub duration: Duration,

    /// HTTP status code
    pub status_code: u16,

    /// Upstream that handled the request
    pub upstream: Option<String>,

    /// Response size in bytes
    pub response_size: Option<usize>,

    /// Custom metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

impl ResponseContext {
    /// Create a new response context
    pub fn new(request_id: String, duration: Duration, status_code: u16) -> Self {
        Self {
            request_id,
            duration,
            status_code,
            upstream: None,
            response_size: None,
            metadata: HashMap::new(),
        }
    }

    /// Set metadata value
    pub fn set_metadata(&mut self, key: String, value: serde_json::Value) {
        self.metadata.insert(key, value);
    }

    /// Get metadata value
    pub fn get_metadata(&self, key: &str) -> Option<&serde_json::Value> {
        self.metadata.get(key)
    }

    /// Check if response was successful (2xx)
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status_code)
    }

    /// Check if response was a client error (4xx)
    pub fn is_client_error(&self) -> bool {
        (400..500).contains(&self.status_code)
    }

    /// Check if response was a server error (5xx)
    pub fn is_server_error(&self) -> bool {
        (500..600).contains(&self.status_code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn test_request_context() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
        let mut ctx = RequestContext::new("req-123".to_string(), addr);

        assert_eq!(ctx.request_id, "req-123");
        assert_eq!(ctx.remote_addr, addr);
        assert!(ctx.elapsed().as_millis() < 1000); // Should be less than 1 second

        ctx.set_metadata("key".to_string(), serde_json::json!("value"));
        assert_eq!(ctx.get_metadata("key"), Some(&serde_json::json!("value")));
    }

    #[test]
    fn test_response_context() {
        let ctx = ResponseContext::new("req-123".to_string(), Duration::from_millis(100), 200);

        assert_eq!(ctx.request_id, "req-123");
        assert_eq!(ctx.duration, Duration::from_millis(100));
        assert_eq!(ctx.status_code, 200);

        assert!(ctx.is_success());
        assert!(!ctx.is_client_error());
        assert!(!ctx.is_server_error());

        let ctx = ResponseContext::new("req-456".to_string(), Duration::from_millis(50), 404);
        assert!(!ctx.is_success());
        assert!(ctx.is_client_error());

        let ctx = ResponseContext::new("req-789".to_string(), Duration::from_millis(200), 500);
        assert!(!ctx.is_success());
        assert!(ctx.is_server_error());
    }
}
