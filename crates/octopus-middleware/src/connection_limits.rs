//! Connection limits middleware
//!
//! Prevents resource exhaustion by limiting concurrent connections.
//! Protects against:
//! - Connection flooding (exhausting file descriptors)
//! - Slowloris attacks (holding connections open)
//! - DDoS attacks (overwhelming the server)

use async_trait::async_trait;
use dashmap::DashMap;
use http::{Request, Response, StatusCode};
use octopus_core::{Body, Middleware, Next, Result};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Connection limits configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionLimitsConfig {
    /// Maximum total concurrent connections
    /// Default: 10,000
    #[serde(default = "default_max_connections")]
    pub max_connections: usize,

    /// Maximum concurrent connections per IP address
    /// Default: 100
    #[serde(default = "default_max_connections_per_ip")]
    pub max_connections_per_ip: usize,

    /// Connection idle timeout
    /// Default: 60 seconds
    #[serde(default = "default_idle_timeout")]
    pub idle_timeout: Duration,

    /// IP addresses to whitelist (unlimited connections)
    #[serde(default)]
    pub whitelist: Vec<IpAddr>,

    /// IP addresses to blacklist (no connections allowed)
    #[serde(default)]
    pub blacklist: Vec<IpAddr>,

    /// Custom error message when limit exceeded
    #[serde(default)]
    pub error_message: Option<String>,
}

fn default_max_connections() -> usize {
    10_000
}

fn default_max_connections_per_ip() -> usize {
    100
}

fn default_idle_timeout() -> Duration {
    Duration::from_secs(60)
}

impl Default for ConnectionLimitsConfig {
    fn default() -> Self {
        Self {
            max_connections: default_max_connections(),
            max_connections_per_ip: default_max_connections_per_ip(),
            idle_timeout: default_idle_timeout(),
            whitelist: Vec::new(),
            blacklist: Vec::new(),
            error_message: None,
        }
    }
}

/// Connection tracking information
#[derive(Debug)]
struct ConnectionInfo {
    count: AtomicUsize,
    last_seen: parking_lot::Mutex<Instant>,
}

impl ConnectionInfo {
    fn new() -> Self {
        Self {
            count: AtomicUsize::new(0),
            last_seen: parking_lot::Mutex::new(Instant::now()),
        }
    }

    fn increment(&self) -> usize {
        *self.last_seen.lock() = Instant::now();
        self.count.fetch_add(1, Ordering::SeqCst) + 1
    }

    fn decrement(&self) -> usize {
        self.count.fetch_sub(1, Ordering::SeqCst).saturating_sub(1)
    }

    fn get_count(&self) -> usize {
        self.count.load(Ordering::SeqCst)
    }

    fn is_idle(&self, timeout: Duration) -> bool {
        self.last_seen.lock().elapsed() > timeout
    }
}

/// Connection limits middleware
///
/// Tracks and limits concurrent connections globally and per-IP.
///
/// # Example
///
/// ```
/// use octopus_middleware::{ConnectionLimits, ConnectionLimitsConfig};
/// use std::time::Duration;
///
/// // Use defaults
/// let limits = ConnectionLimits::default();
///
/// // Custom limits
/// let config = ConnectionLimitsConfig {
///     max_connections: 5000,
///     max_connections_per_ip: 50,
///     idle_timeout: Duration::from_secs(30),
///     ..Default::default()
/// };
/// let limits = ConnectionLimits::with_config(config);
/// ```
#[derive(Debug, Clone)]
pub struct ConnectionLimits {
    config: ConnectionLimitsConfig,
    total_connections: Arc<AtomicUsize>,
    connections_per_ip: Arc<DashMap<IpAddr, Arc<ConnectionInfo>>>,
}

impl ConnectionLimits {
    /// Create a new connection limits middleware with default configuration
    pub fn new() -> Self {
        Self {
            config: ConnectionLimitsConfig::default(),
            total_connections: Arc::new(AtomicUsize::new(0)),
            connections_per_ip: Arc::new(DashMap::new()),
        }
    }

    /// Create a new connection limits middleware with custom configuration
    pub fn with_config(config: ConnectionLimitsConfig) -> Self {
        Self {
            config,
            total_connections: Arc::new(AtomicUsize::new(0)),
            connections_per_ip: Arc::new(DashMap::new()),
        }
    }

    /// Create a strict connection limits configuration
    /// Recommended for public APIs
    pub fn strict() -> Self {
        Self::with_config(ConnectionLimitsConfig {
            max_connections: 5000,
            max_connections_per_ip: 25,
            idle_timeout: Duration::from_secs(30),
            whitelist: Vec::new(),
            blacklist: Vec::new(),
            error_message: Some("Too many concurrent connections".to_string()),
        })
    }

    /// Create a permissive connection limits configuration
    /// Use for internal APIs
    pub fn permissive() -> Self {
        Self::with_config(ConnectionLimitsConfig {
            max_connections: 50_000,
            max_connections_per_ip: 1000,
            idle_timeout: Duration::from_secs(300),
            whitelist: Vec::new(),
            blacklist: Vec::new(),
            error_message: None,
        })
    }

    /// Get current total connection count
    pub fn current_connections(&self) -> usize {
        self.total_connections.load(Ordering::SeqCst)
    }

    /// Get current connection count for an IP
    pub fn connections_for_ip(&self, ip: &IpAddr) -> usize {
        self.connections_per_ip
            .get(ip)
            .map(|info| info.get_count())
            .unwrap_or(0)
    }

    /// Extract client IP from request headers
    fn extract_client_ip(&self, req: &Request<Body>) -> Option<IpAddr> {
        // Try X-Forwarded-For header first (behind proxy)
        if let Some(forwarded) = req.headers().get("x-forwarded-for") {
            if let Ok(forwarded_str) = forwarded.to_str() {
                if let Some(first_ip) = forwarded_str.split(',').next() {
                    if let Ok(ip) = first_ip.trim().parse() {
                        return Some(ip);
                    }
                }
            }
        }

        // Try X-Real-IP header
        if let Some(real_ip) = req.headers().get("x-real-ip") {
            if let Ok(ip_str) = real_ip.to_str() {
                if let Ok(ip) = ip_str.parse() {
                    return Some(ip);
                }
            }
        }

        // Could also extract from socket address if available in extensions
        None
    }

    /// Clean up idle connections
    fn cleanup_idle_connections(&self) {
        let timeout = self.config.idle_timeout;
        self.connections_per_ip.retain(|_, info| {
            if info.get_count() == 0 && info.is_idle(timeout) {
                false // Remove
            } else {
                true // Keep
            }
        });
    }

    fn error_response(&self) -> Response<Body> {
        use bytes::Bytes;
        use http_body_util::Full;

        let message = self
            .config
            .error_message
            .as_deref()
            .unwrap_or("Too many concurrent connections - please try again later");

        let body = serde_json::json!({
            "error": "connection_limit_exceeded",
            "message": message,
            "retry_after": 5,
        })
        .to_string();

        Response::builder()
            .status(StatusCode::SERVICE_UNAVAILABLE)
            .header("content-type", "application/json")
            .header("retry-after", "5")
            .body(Full::new(Bytes::from(body)))
            .expect("Failed to build error response")
    }
}

impl Default for ConnectionLimits {
    fn default() -> Self {
        Self::new()
    }
}

/// RAII guard for connection tracking
struct ConnectionGuard {
    total_connections: Arc<AtomicUsize>,
    ip_connection_info: Option<Arc<ConnectionInfo>>,
}

impl ConnectionGuard {
    fn new(
        total_connections: Arc<AtomicUsize>,
        ip_connection_info: Option<Arc<ConnectionInfo>>,
    ) -> Self {
        // Increment counters
        total_connections.fetch_add(1, Ordering::SeqCst);
        if let Some(ref info) = ip_connection_info {
            info.increment();
        }

        Self {
            total_connections,
            ip_connection_info,
        }
    }
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        // Decrement counters when request completes
        self.total_connections.fetch_sub(1, Ordering::SeqCst);
        if let Some(ref info) = self.ip_connection_info {
            info.decrement();
        }
    }
}

#[async_trait]
impl Middleware for ConnectionLimits {
    async fn call(&self, req: Request<Body>, next: Next) -> Result<Response<Body>> {
        // Periodic cleanup of idle connections
        if rand::random::<u8>() < 10 {
            // ~4% chance
            self.cleanup_idle_connections();
        }

        // Extract client IP
        let client_ip = self.extract_client_ip(&req);

        // Check blacklist
        if let Some(ip) = client_ip {
            if self.config.blacklist.contains(&ip) {
                tracing::warn!(
                    client_ip = %ip,
                    "Connection rejected: IP is blacklisted"
                );
                return Ok(self.error_response());
            }
        }

        // Check whitelist (bypass limits)
        let is_whitelisted = client_ip
            .map(|ip| self.config.whitelist.contains(&ip))
            .unwrap_or(false);

        if !is_whitelisted {
            // Check total connection limit
            let current_total = self.current_connections();
            if current_total >= self.config.max_connections {
                tracing::warn!(
                    current_connections = current_total,
                    max_connections = self.config.max_connections,
                    "Connection rejected: total connection limit exceeded"
                );
                return Ok(self.error_response());
            }

            // Check per-IP connection limit
            if let Some(ip) = client_ip {
                let ip_connections = self.connections_for_ip(&ip);
                if ip_connections >= self.config.max_connections_per_ip {
                    tracing::warn!(
                        client_ip = %ip,
                        ip_connections,
                        max_per_ip = self.config.max_connections_per_ip,
                        "Connection rejected: per-IP connection limit exceeded"
                    );
                    return Ok(self.error_response());
                }
            }
        }

        // Get or create connection info for this IP
        let ip_info = client_ip.map(|ip| {
            self.connections_per_ip
                .entry(ip)
                .or_insert_with(|| Arc::new(ConnectionInfo::new()))
                .clone()
        });

        // Create guard to track connection (will auto-decrement on drop)
        let _guard = ConnectionGuard::new(self.total_connections.clone(), ip_info);

        // Process request
        next.run(req).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use http_body_util::Full;
    use std::sync::Arc;

    type TestBody = Full<Bytes>;

    // Mock handler for testing
    #[derive(Debug, Clone)]
    struct TestHandler;

    #[async_trait]
    impl Middleware for TestHandler {
        async fn call(&self, _req: Request<TestBody>, _next: Next) -> Result<Response<TestBody>> {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(Full::new(Bytes::from("success")))
                .unwrap())
        }
    }

    #[tokio::test]
    async fn test_accept_normal_request() {
        let limits = ConnectionLimits::default();
        let handler = TestHandler;
        let stack: Arc<[Arc<dyn Middleware>]> =
            Arc::new([Arc::new(limits), Arc::new(handler)]);

        let req = Request::builder()
            .uri("/test")
            .body(Full::new(Bytes::from("")))
            .unwrap();

        let next = Next::new(stack);
        let response = next.run(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_reject_when_total_limit_exceeded() {
        let config = ConnectionLimitsConfig {
            max_connections: 2,
            ..Default::default()
        };
        let limits = ConnectionLimits::with_config(config);
        
        // Manually set high connection count
        limits.total_connections.store(10, Ordering::SeqCst);
        
        let handler = TestHandler;
        let stack: Arc<[Arc<dyn Middleware>]> =
            Arc::new([Arc::new(limits), Arc::new(handler)]);

        let req = Request::builder()
            .uri("/test")
            .body(Full::new(Bytes::from("")))
            .unwrap();

        let next = Next::new(stack);
        let response = next.run(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn test_track_connections_per_ip() {
        let config = ConnectionLimitsConfig {
            max_connections_per_ip: 2,
            ..Default::default()
        };
        let limits = ConnectionLimits::with_config(config.clone());

        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        
        // Manually set IP connection count
        let info = Arc::new(ConnectionInfo::new());
        info.count.store(3, Ordering::SeqCst);
        limits.connections_per_ip.insert(ip, info);

        let handler = TestHandler;
        let stack: Arc<[Arc<dyn Middleware>]> =
            Arc::new([Arc::new(limits), Arc::new(handler)]);

        let req = Request::builder()
            .uri("/test")
            .header("x-forwarded-for", "192.168.1.100")
            .body(Full::new(Bytes::from("")))
            .unwrap();

        let next = Next::new(stack);
        let response = next.run(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn test_whitelist_bypasses_limits() {
        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        let config = ConnectionLimitsConfig {
            max_connections: 1,
            whitelist: vec![ip],
            ..Default::default()
        };
        let limits = ConnectionLimits::with_config(config);
        
        // Set connection count above limit
        limits.total_connections.store(10, Ordering::SeqCst);

        let handler = TestHandler;
        let stack: Arc<[Arc<dyn Middleware>]> =
            Arc::new([Arc::new(limits), Arc::new(handler)]);

        let req = Request::builder()
            .uri("/test")
            .header("x-forwarded-for", "192.168.1.100")
            .body(Full::new(Bytes::from("")))
            .unwrap();

        let next = Next::new(stack);
        let response = next.run(req).await.unwrap();

        // Should succeed despite being over limit (whitelisted)
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_blacklist_rejects_immediately() {
        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        let config = ConnectionLimitsConfig {
            blacklist: vec![ip],
            ..Default::default()
        };
        let limits = ConnectionLimits::with_config(config);

        let handler = TestHandler;
        let stack: Arc<[Arc<dyn Middleware>]> =
            Arc::new([Arc::new(limits), Arc::new(handler)]);

        let req = Request::builder()
            .uri("/test")
            .header("x-forwarded-for", "192.168.1.100")
            .body(Full::new(Bytes::from("")))
            .unwrap();

        let next = Next::new(stack);
        let response = next.run(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}

