//! Mock HTTP upstream server for integration testing

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::sleep;
use hyper::{Request, Response, StatusCode, body::Incoming};
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use hyper_util::server::conn::auto::Builder;
use http_body_util::{Full, BodyExt};
use bytes::Bytes;
use tokio::net::TcpListener;

/// Configuration for mock upstream behavior
#[derive(Debug, Clone)]
pub struct MockConfig {
    /// Response delay
    pub delay: Option<Duration>,
    /// Error rate (0.0-1.0)
    pub error_rate: f64,
    /// Default status code
    pub status_code: StatusCode,
    /// Response body
    pub body: Bytes,
    /// Custom headers
    pub headers: HashMap<String, String>,
    /// Whether to echo request headers
    pub echo_headers: bool,
    /// Maximum body size to accept
    pub max_body_size: usize,
}

impl Default for MockConfig {
    fn default() -> Self {
        Self {
            delay: None,
            error_rate: 0.0,
            status_code: StatusCode::OK,
            body: Bytes::from("OK"),
            headers: HashMap::new(),
            echo_headers: false,
            max_body_size: 10 * 1024 * 1024, // 10MB
        }
    }
}

/// Mock response for specific paths
#[derive(Debug, Clone)]
pub struct MockResponse {
    pub status: StatusCode,
    pub body: Bytes,
    pub headers: HashMap<String, String>,
    pub delay: Option<Duration>,
}

impl MockResponse {
    pub fn new(status: StatusCode, body: impl Into<Bytes>) -> Self {
        Self {
            status,
            body: body.into(),
            headers: HashMap::new(),
            delay: None,
        }
    }

    pub fn with_header(mut self, key: String, value: String) -> Self {
        self.headers.insert(key, value);
        self
    }

    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = Some(delay);
        self
    }
}

/// Statistics tracked by mock upstream
#[derive(Debug, Clone, Default)]
pub struct MockStats {
    pub requests_received: usize,
    pub bytes_received: usize,
    pub bytes_sent: usize,
    pub active_connections: usize,
    pub total_connections: usize,
}

/// Mock HTTP upstream server
pub struct MockUpstream {
    config: Arc<RwLock<MockConfig>>,
    routes: Arc<RwLock<HashMap<String, MockResponse>>>,
    stats: Arc<RwLock<MockStats>>,
    addr: SocketAddr,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

impl MockUpstream {
    /// Create a new mock upstream server
    pub async fn new(port: u16) -> anyhow::Result<Self> {
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        
        Ok(Self {
            config: Arc::new(RwLock::new(MockConfig::default())),
            routes: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(RwLock::new(MockStats::default())),
            addr,
            shutdown_tx: None,
        })
    }

    /// Start the mock server
    pub async fn start(&mut self) -> anyhow::Result<()> {
        let listener = TcpListener::bind(self.addr).await?;
        let actual_addr = listener.local_addr()?;
        self.addr = actual_addr;

        let config = Arc::clone(&self.config);
        let routes = Arc::clone(&self.routes);
        let stats = Arc::clone(&self.stats);

        let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel();
        self.shutdown_tx = Some(shutdown_tx);

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => {
                        break;
                    }
                    result = listener.accept() => {
                        match result {
                            Ok((stream, _)) => {
                                let config = Arc::clone(&config);
                                let routes = Arc::clone(&routes);
                                let stats = Arc::clone(&stats);

                                // Track connection
                                {
                                    let mut s = stats.write().await;
                                    s.active_connections += 1;
                                    s.total_connections += 1;
                                }

                                tokio::spawn(async move {
                                    let io = TokioIo::new(stream);
                                    
                                    let service = service_fn(|req: Request<Incoming>| {
                                        let config = Arc::clone(&config);
                                        let routes = Arc::clone(&routes);
                                        let stats = Arc::clone(&stats);
                                        
                                        async move {
                                            handle_request(req, config, routes, stats).await
                                        }
                                    });

                                    if let Err(e) = Builder::new(hyper_util::rt::TokioExecutor::new())
                                        .serve_connection(io, service)
                                        .await
                                    {
                                        eprintln!("Mock server error: {}", e);
                                    }

                                    // Untrack connection
                                    let mut s = stats.write().await;
                                    s.active_connections = s.active_connections.saturating_sub(1);
                                });
                            }
                            Err(e) => {
                                eprintln!("Accept error: {}", e);
                            }
                        }
                    }
                }
            }
        });

        Ok(())
    }

    /// Get the actual bound address
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Update configuration
    pub async fn set_config(&self, config: MockConfig) {
        *self.config.write().await = config;
    }

    /// Add a route-specific response
    pub async fn add_route(&self, path: String, response: MockResponse) {
        self.routes.write().await.insert(path, response);
    }

    /// Remove a route
    pub async fn remove_route(&self, path: &str) {
        self.routes.write().await.remove(path);
    }

    /// Get current statistics
    pub async fn stats(&self) -> MockStats {
        self.stats.read().await.clone()
    }

    /// Reset statistics
    pub async fn reset_stats(&self) {
        *self.stats.write().await = MockStats::default();
    }

    /// Stop the server
    pub async fn stop(self) {
        if let Some(tx) = self.shutdown_tx {
            let _ = tx.send(());
        }
    }
}

async fn handle_request(
    req: Request<Incoming>,
    config: Arc<RwLock<MockConfig>>,
    routes: Arc<RwLock<HashMap<String, MockResponse>>>,
    stats: Arc<RwLock<MockStats>>,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    // Track request
    {
        let mut s = stats.write().await;
        s.requests_received += 1;
    }

    // Read request body
    let path = req.uri().path().to_string();
    let (_parts, body) = req.into_parts();
    
    let body_bytes = match body.collect().await {
        Ok(collected) => {
            let bytes = collected.to_bytes();
            let mut s = stats.write().await;
            s.bytes_received += bytes.len();
            bytes
        }
        Err(_) => Bytes::new(),
    };

    // Check for route-specific response
    let route_response = routes.read().await.get(&path).cloned();
    
    if let Some(mock_resp) = route_response {
        // Apply delay
        if let Some(delay) = mock_resp.delay {
            sleep(delay).await;
        }

        let mut response = Response::builder().status(mock_resp.status);
        
        // Add headers
        for (k, v) in &mock_resp.headers {
            response = response.header(k, v);
        }

        let body_len = mock_resp.body.len();
        let resp = response.body(Full::new(mock_resp.body)).unwrap();

        // Track response
        {
            let mut s = stats.write().await;
            s.bytes_sent += body_len;
        }

        return Ok(resp);
    }

    // Use default config
    let cfg = config.read().await.clone();

    // Apply delay
    if let Some(delay) = cfg.delay {
        sleep(delay).await;
    }

    // Simulate errors
    if cfg.error_rate > 0.0 {
        let rand_val: f64 = rand::random();
        if rand_val < cfg.error_rate {
            let error_resp = Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Full::new(Bytes::from("Simulated error")))
                .unwrap();
            return Ok(error_resp);
        }
    }

    let mut response = Response::builder().status(cfg.status_code);

    // Add custom headers
    for (k, v) in &cfg.headers {
        response = response.header(k, v);
    }

    // Echo request headers if configured
    if cfg.echo_headers {
        response = response.header("X-Request-Body-Size", body_bytes.len().to_string());
    }

    let body_len = cfg.body.len();
    let resp = response.body(Full::new(cfg.body)).unwrap();

    // Track response
    {
        let mut s = stats.write().await;
        s.bytes_sent += body_len;
    }

    Ok(resp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_upstream_basic() {
        let mut mock = MockUpstream::new(0).await.unwrap();
        mock.start().await.unwrap();

        let addr = mock.addr();
        assert!(addr.port() > 0);

        let stats = mock.stats().await;
        assert_eq!(stats.requests_received, 0);
    }

    #[tokio::test]
    async fn test_mock_config() {
        let mut config = MockConfig::default();
        config.status_code = StatusCode::NOT_FOUND;
        config.body = Bytes::from("Not Found");

        assert_eq!(config.status_code, StatusCode::NOT_FOUND);
    }
}
