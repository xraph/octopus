//! Active health checking for upstream services

use async_trait::async_trait;
use http::{Method, StatusCode, Uri};
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use std::collections::HashMap;
use std::fmt;
use std::net::TcpStream;
use std::time::{Duration, Instant};
use tokio::time::timeout;
use tracing::{debug, warn};

/// Health check status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    /// Service is healthy
    Healthy,
    /// Service is unhealthy
    Unhealthy,
    /// Health status is unknown (not yet checked)
    Unknown,
}

impl fmt::Display for HealthStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HealthStatus::Healthy => write!(f, "healthy"),
            HealthStatus::Unhealthy => write!(f, "unhealthy"),
            HealthStatus::Unknown => write!(f, "unknown"),
        }
    }
}

/// Health check result
#[derive(Debug, Clone)]
pub struct HealthCheckResult {
    /// Health status
    pub status: HealthStatus,
    /// Time taken for the check
    pub duration: Duration,
    /// Timestamp of the check
    pub timestamp: Instant,
    /// Optional error message
    pub message: Option<String>,
}

impl HealthCheckResult {
    /// Create a healthy result
    pub fn healthy(duration: Duration) -> Self {
        Self {
            status: HealthStatus::Healthy,
            duration,
            timestamp: Instant::now(),
            message: None,
        }
    }

    /// Create an unhealthy result with a message
    pub fn unhealthy(duration: Duration, message: impl Into<String>) -> Self {
        Self {
            status: HealthStatus::Unhealthy,
            duration,
            timestamp: Instant::now(),
            message: Some(message.into()),
        }
    }

    /// Create an unknown result
    pub fn unknown() -> Self {
        Self {
            status: HealthStatus::Unknown,
            duration: Duration::from_secs(0),
            timestamp: Instant::now(),
            message: None,
        }
    }
}

/// Health check type
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthCheckType {
    /// HTTP health check
    Http {
        /// Path to check
        path: String,
        /// Expected status codes
        expected_status: Vec<StatusCode>,
        /// HTTP method
        method: Method,
        /// Request headers
        headers: HashMap<String, String>,
    },
    /// TCP health check (just check if port is open)
    Tcp,
    /// gRPC health check
    Grpc {
        /// gRPC service name
        service: String,
    },
}

/// Health check configuration
#[derive(Debug, Clone)]
pub struct HealthCheckConfig {
    /// Type of health check
    pub check_type: HealthCheckType,
    /// Check timeout
    pub timeout: Duration,
    /// Number of consecutive successful checks required to mark healthy
    pub healthy_threshold: u32,
    /// Number of consecutive failed checks required to mark unhealthy
    pub unhealthy_threshold: u32,
}

impl Default for HealthCheckConfig {
    fn default() -> Self {
        Self {
            check_type: HealthCheckType::Http {
                path: "/health".to_string(),
                expected_status: vec![StatusCode::OK],
                method: Method::GET,
                headers: HashMap::new(),
            },
            timeout: Duration::from_secs(5),
            healthy_threshold: 2,
            unhealthy_threshold: 3,
        }
    }
}

/// Trait for performing health checks
#[async_trait]
pub trait HealthCheck: Send + Sync + fmt::Debug {
    /// Perform a health check on the given address
    async fn check(&self, address: &str, port: u16) -> HealthCheckResult;
}

/// HTTP health checker
#[derive(Debug, Clone)]
pub struct HttpHealthCheck {
    path: String,
    expected_status: Vec<StatusCode>,
    method: Method,
    headers: HashMap<String, String>,
    timeout_duration: Duration,
    client: Client<HttpConnector, http_body_util::Empty<bytes::Bytes>>,
}

impl HttpHealthCheck {
    /// Create a new HTTP health checker
    pub fn new(
        path: String,
        expected_status: Vec<StatusCode>,
        method: Method,
        headers: HashMap<String, String>,
        timeout_duration: Duration,
    ) -> Self {
        let client = Client::builder(TokioExecutor::new())
            .http1_title_case_headers(true)
            .http2_adaptive_window(true)
            .build_http();

        Self {
            path,
            expected_status,
            method,
            headers,
            timeout_duration,
            client,
        }
    }
}

#[async_trait]
impl HealthCheck for HttpHealthCheck {
    async fn check(&self, address: &str, port: u16) -> HealthCheckResult {
        let start = Instant::now();
        let url = format!("http://{}:{}{}", address, port, self.path);

        debug!(url = %url, "Performing HTTP health check");

        let uri: Uri = match url.parse() {
            Ok(u) => u,
            Err(e) => {
                return HealthCheckResult::unhealthy(start.elapsed(), format!("Invalid URL: {e}"));
            }
        };

        let mut req_builder = http::Request::builder().method(&self.method).uri(uri);

        // Add custom headers
        for (key, value) in &self.headers {
            req_builder = req_builder.header(key, value);
        }

        let req = match req_builder.body(http_body_util::Empty::<bytes::Bytes>::new()) {
            Ok(r) => r,
            Err(e) => {
                return HealthCheckResult::unhealthy(
                    start.elapsed(),
                    format!("Failed to build request: {e}"),
                );
            }
        };

        // Perform the request with timeout
        match timeout(self.timeout_duration, self.client.request(req)).await {
            Ok(Ok(response)) => {
                let status = response.status();
                let duration = start.elapsed();

                if self.expected_status.contains(&status) {
                    debug!(url = %url, status = %status, "Health check passed");
                    HealthCheckResult::healthy(duration)
                } else {
                    warn!(url = %url, status = %status, "Health check failed: unexpected status");
                    HealthCheckResult::unhealthy(
                        duration,
                        format!("Unexpected status code: {status}"),
                    )
                }
            }
            Ok(Err(e)) => {
                let duration = start.elapsed();
                warn!(url = %url, error = %e, "Health check failed: request error");
                HealthCheckResult::unhealthy(duration, format!("Request error: {e}"))
            }
            Err(_) => {
                let duration = start.elapsed();
                warn!(url = %url, "Health check failed: timeout");
                HealthCheckResult::unhealthy(duration, "Timeout".to_string())
            }
        }
    }
}

/// TCP health checker
#[derive(Debug, Clone)]
pub struct TcpHealthCheck {
    timeout_duration: Duration,
}

impl TcpHealthCheck {
    /// Create a new TCP health checker
    pub fn new(timeout_duration: Duration) -> Self {
        Self { timeout_duration }
    }
}

#[async_trait]
impl HealthCheck for TcpHealthCheck {
    async fn check(&self, address: &str, port: u16) -> HealthCheckResult {
        let start = Instant::now();
        let addr = format!("{address}:{port}");

        debug!(addr = %addr, "Performing TCP health check");

        // Try to connect with timeout
        let connect_future = tokio::task::spawn_blocking({
            let addr = addr.clone();
            move || TcpStream::connect(addr)
        });

        match timeout(self.timeout_duration, connect_future).await {
            Ok(Ok(Ok(_))) => {
                debug!(addr = %addr, "TCP health check passed");
                HealthCheckResult::healthy(start.elapsed())
            }
            Ok(Ok(Err(e))) => {
                warn!(addr = %addr, error = %e, "TCP health check failed");
                HealthCheckResult::unhealthy(start.elapsed(), format!("Connection error: {e}"))
            }
            Ok(Err(e)) => {
                warn!(addr = %addr, error = %e, "TCP health check failed");
                HealthCheckResult::unhealthy(start.elapsed(), format!("Task error: {e}"))
            }
            Err(_) => {
                warn!(addr = %addr, "TCP health check failed: timeout");
                HealthCheckResult::unhealthy(start.elapsed(), "Timeout".to_string())
            }
        }
    }
}

/// gRPC health check using the standard grpc.health.v1.Health/Check protocol
///
/// Sends an HTTP/2 POST to /grpc.health.v1.Health/Check with a protobuf-encoded
/// HealthCheckRequest containing the service name. Parses the response to check
/// the serving status.
#[derive(Debug)]
struct GrpcHealthCheck {
    service: String,
    timeout_duration: Duration,
}

impl GrpcHealthCheck {
    fn new(service: String, timeout_duration: Duration) -> Self {
        Self {
            service,
            timeout_duration,
        }
    }

    /// Hand-craft a protobuf-encoded HealthCheckRequest
    /// Proto: message HealthCheckRequest { string service = 1; }
    /// Protobuf wire format: field 1, type 2 (length-delimited) = tag 0x0a
    fn encode_health_request(&self) -> Vec<u8> {
        let service_bytes = self.service.as_bytes();
        let mut buf = Vec::new();
        if !service_bytes.is_empty() {
            buf.push(0x0a); // field 1, wire type 2 (length-delimited)
            buf.push(service_bytes.len() as u8);
            buf.extend_from_slice(service_bytes);
        }
        buf
    }

    /// Wrap protobuf message in gRPC frame: [compressed(1b)][length(4b)][message]
    fn grpc_frame(msg: &[u8]) -> Vec<u8> {
        let len = msg.len() as u32;
        let mut frame = Vec::with_capacity(5 + msg.len());
        frame.push(0); // not compressed
        frame.extend_from_slice(&len.to_be_bytes());
        frame.extend_from_slice(msg);
        frame
    }

    /// Parse serving status from protobuf response
    /// Proto: message HealthCheckResponse { ServingStatus status = 1; }
    /// ServingStatus: UNKNOWN=0, SERVING=1, NOT_SERVING=2
    fn parse_serving_status(body: &[u8]) -> Option<i32> {
        // Skip gRPC 5-byte frame header
        if body.len() < 5 {
            return None;
        }
        let msg = &body[5..];
        // Parse protobuf: field 1, wire type 0 (varint) = tag 0x08
        if msg.len() >= 2 && msg[0] == 0x08 {
            Some(msg[1] as i32)
        } else if msg.is_empty() {
            // Empty response body means SERVING (default value)
            Some(1)
        } else {
            None
        }
    }
}

#[async_trait]
impl HealthCheck for GrpcHealthCheck {
    async fn check(&self, address: &str, port: u16) -> HealthCheckResult {
        let start = Instant::now();
        let addr = format!("{address}:{port}");

        debug!(addr = %addr, service = %self.service, "Performing gRPC health check");

        // Build gRPC health check request
        let proto_msg = self.encode_health_request();
        let body = Self::grpc_frame(&proto_msg);

        let uri = format!("http://{addr}/grpc.health.v1.Health/Check");

        // Connect via HTTP/2 and send request
        let result = timeout(self.timeout_duration, async {
            let stream = tokio::net::TcpStream::connect(&addr).await?;
            stream.set_nodelay(true)?;

            let io = hyper_util::rt::TokioIo::new(stream);
            let (mut sender, conn) =
                hyper::client::conn::http2::Builder::new(hyper_util::rt::TokioExecutor::new())
                    .handshake(io)
                    .await
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::ConnectionRefused, e))?;

            tokio::spawn(async move {
                let _ = conn.await;
            });

            let req = http::Request::builder()
                .method(Method::POST)
                .uri(uri)
                .header("content-type", "application/grpc")
                .header("te", "trailers")
                .body(http_body_util::Full::new(bytes::Bytes::from(body)))
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;

            let resp = sender
                .send_request(req)
                .await
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::ConnectionAborted, e))?;

            // Check grpc-status header (may be in headers for health check)
            let grpc_status = resp
                .headers()
                .get("grpc-status")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<i32>().ok());

            if let Some(status) = grpc_status {
                if status != 0 {
                    return Err(std::io::Error::other(format!("gRPC status: {status}")));
                }
            }

            // Collect response body
            use http_body_util::BodyExt;
            let body_bytes = resp
                .into_body()
                .collect()
                .await
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?
                .to_bytes();

            // Parse serving status
            let status = Self::parse_serving_status(&body_bytes).unwrap_or(0);

            Ok::<i32, std::io::Error>(status)
        })
        .await;

        match result {
            Ok(Ok(status)) => {
                if status == 1 {
                    // SERVING
                    debug!(addr = %addr, "gRPC health check passed (SERVING)");
                    HealthCheckResult::healthy(start.elapsed())
                } else {
                    let msg = match status {
                        0 => "UNKNOWN",
                        2 => "NOT_SERVING",
                        3 => "SERVICE_UNKNOWN",
                        _ => "UNRECOGNIZED",
                    };
                    warn!(addr = %addr, status = %msg, "gRPC health check: not serving");
                    HealthCheckResult::unhealthy(start.elapsed(), format!("Status: {msg}"))
                }
            }
            Ok(Err(e)) => {
                warn!(addr = %addr, error = %e, "gRPC health check failed");
                HealthCheckResult::unhealthy(start.elapsed(), format!("Error: {e}"))
            }
            Err(_) => {
                warn!(addr = %addr, "gRPC health check timeout");
                HealthCheckResult::unhealthy(start.elapsed(), "Timeout".to_string())
            }
        }
    }
}

/// Health checker that manages health checks for multiple instances
#[derive(Debug)]
pub struct HealthChecker {
    config: HealthCheckConfig,
    checker: Box<dyn HealthCheck>,
}

impl HealthChecker {
    /// Create a new health checker
    pub fn new(config: HealthCheckConfig) -> Self {
        let checker: Box<dyn HealthCheck> = match &config.check_type {
            HealthCheckType::Http {
                path,
                expected_status,
                method,
                headers,
            } => Box::new(HttpHealthCheck::new(
                path.clone(),
                expected_status.clone(),
                method.clone(),
                headers.clone(),
                config.timeout,
            )),
            HealthCheckType::Tcp => Box::new(TcpHealthCheck::new(config.timeout)),
            HealthCheckType::Grpc { service } => {
                Box::new(GrpcHealthCheck::new(service.clone(), config.timeout))
            }
        };

        Self { config, checker }
    }

    /// Perform a health check
    pub async fn check(&self, address: &str, port: u16) -> HealthCheckResult {
        self.checker.check(address, port).await
    }

    /// Get the health check configuration
    pub fn config(&self) -> &HealthCheckConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_health_check_result() {
        let healthy = HealthCheckResult::healthy(Duration::from_millis(10));
        assert_eq!(healthy.status, HealthStatus::Healthy);
        assert!(healthy.message.is_none());

        let unhealthy = HealthCheckResult::unhealthy(Duration::from_millis(10), "error");
        assert_eq!(unhealthy.status, HealthStatus::Unhealthy);
        assert_eq!(unhealthy.message, Some("error".to_string()));

        let unknown = HealthCheckResult::unknown();
        assert_eq!(unknown.status, HealthStatus::Unknown);
    }

    #[tokio::test]
    async fn test_tcp_health_check_invalid_address() {
        let checker = TcpHealthCheck::new(Duration::from_secs(1));
        let result = checker.check("invalid.nonexistent.address", 12345).await;
        assert_eq!(result.status, HealthStatus::Unhealthy);
        assert!(result.message.is_some());
    }

    #[tokio::test]
    async fn test_http_health_check_invalid_url() {
        let checker = HttpHealthCheck::new(
            "/health".to_string(),
            vec![StatusCode::OK],
            Method::GET,
            HashMap::new(),
            Duration::from_secs(1),
        );

        let result = checker.check("invalid.nonexistent.address", 80).await;
        assert_eq!(result.status, HealthStatus::Unhealthy);
    }

    #[tokio::test]
    async fn test_health_status_display() {
        assert_eq!(format!("{}", HealthStatus::Healthy), "healthy");
        assert_eq!(format!("{}", HealthStatus::Unhealthy), "unhealthy");
        assert_eq!(format!("{}", HealthStatus::Unknown), "unknown");
    }
}
