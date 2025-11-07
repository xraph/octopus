//! Audit logging middleware
//!
//! Logs security-relevant events for compliance and forensics.
//! Tracks:
//! - Authentication attempts (success/failure)
//! - Authorization failures
//! - Rate limiting violations
//! - Suspicious activity
//! - Admin actions

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use http::{Request, Response, StatusCode};
use octopus_core::{Body, Middleware, Next, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

/// Audit event type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuditEventType {
    /// Successful authentication
    AuthSuccess,
    /// Failed authentication attempt
    AuthFailure,
    /// Authorization denied
    AuthzFailure,
    /// Rate limit exceeded
    RateLimitExceeded,
    /// TLS handshake failure
    TlsFailure,
    /// Admin API access
    AdminAccess,
    /// Configuration change
    ConfigChange,
    /// Suspicious activity detected
    SuspiciousActivity,
    /// Request blocked by security policy
    RequestBlocked,
}

/// Audit event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    /// Event timestamp
    pub timestamp: DateTime<Utc>,
    /// Event type
    pub event_type: AuditEventType,
    /// Client IP address
    pub client_ip: Option<String>,
    /// User identifier (if authenticated)
    pub user_id: Option<String>,
    /// HTTP method
    pub method: String,
    /// Request path
    pub path: String,
    /// HTTP status code
    pub status_code: u16,
    /// Additional event details
    pub details: Option<String>,
    /// Request ID for correlation
    pub request_id: Option<String>,
}

impl AuditEvent {
    fn new(event_type: AuditEventType) -> Self {
        Self {
            timestamp: Utc::now(),
            event_type,
            client_ip: None,
            user_id: None,
            method: String::new(),
            path: String::new(),
            status_code: 0,
            details: None,
            request_id: None,
        }
    }
}

/// Audit output destination
#[derive(Debug, Clone)]
pub enum AuditOutput {
    /// Write to file
    File(PathBuf),
    /// Write to stdout
    Stdout,
    /// Write to stderr
    Stderr,
    /// Custom handler
    Custom(Arc<dyn AuditHandler>),
}

/// Custom audit handler trait
pub trait AuditHandler: Send + Sync + std::fmt::Debug {
    /// Handle an audit event
    fn log(&self, event: &AuditEvent);
}

/// Audit logger configuration
#[derive(Debug, Clone)]
pub struct AuditLoggerConfig {
    /// Output destination
    pub output: AuditOutput,

    /// Log successful authentication
    pub log_auth_success: bool,

    /// Log failed authentication
    pub log_auth_failure: bool,

    /// Log authorization failures
    pub log_authz_failure: bool,

    /// Log rate limit violations
    pub log_rate_limit: bool,

    /// Log TLS failures
    pub log_tls_failure: bool,

    /// Log admin access
    pub log_admin_access: bool,

    /// Log configuration changes
    pub log_config_change: bool,

    /// Log suspicious activity
    pub log_suspicious: bool,

    /// Pretty print JSON (for readability)
    pub pretty_print: bool,
}

impl Default for AuditLoggerConfig {
    fn default() -> Self {
        Self {
            output: AuditOutput::File(PathBuf::from("audit.log")),
            log_auth_success: false, // Too noisy
            log_auth_failure: true,
            log_authz_failure: true,
            log_rate_limit: true,
            log_tls_failure: true,
            log_admin_access: true,
            log_config_change: true,
            log_suspicious: true,
            pretty_print: false,
        }
    }
}

/// Audit logger middleware
///
/// Logs security-relevant events for compliance and forensics.
///
/// # Example
///
/// ```
/// use octopus_middleware::{AuditLogger, AuditLoggerConfig, AuditOutput};
/// use std::path::PathBuf;
///
/// // Log to file
/// let config = AuditLoggerConfig {
///     output: AuditOutput::File(PathBuf::from("/var/log/octopus/audit.log")),
///     ..Default::default()
/// };
/// let logger = AuditLogger::with_config(config);
///
/// // Log to stdout (for containers)
/// let stdout_logger = AuditLogger::stdout();
/// ```
#[derive(Debug, Clone)]
pub struct AuditLogger {
    config: AuditLoggerConfig,
    file_handle: Option<Arc<Mutex<tokio::fs::File>>>,
}

impl AuditLogger {
    /// Create a new audit logger with default configuration
    pub async fn new() -> std::io::Result<Self> {
        Self::with_config(AuditLoggerConfig::default()).await
    }

    /// Create a new audit logger with custom configuration
    pub async fn with_config(config: AuditLoggerConfig) -> std::io::Result<Self> {
        let file_handle = if let AuditOutput::File(ref path) = config.output {
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .await?;
            Some(Arc::new(Mutex::new(file)))
        } else {
            None
        };

        Ok(Self {
            config,
            file_handle,
        })
    }

    /// Create an audit logger that writes to stdout
    pub fn stdout() -> Self {
        Self {
            config: AuditLoggerConfig {
                output: AuditOutput::Stdout,
                ..Default::default()
            },
            file_handle: None,
        }
    }

    /// Create an audit logger that writes to stderr
    pub fn stderr() -> Self {
        Self {
            config: AuditLoggerConfig {
                output: AuditOutput::Stderr,
                ..Default::default()
            },
            file_handle: None,
        }
    }

    /// Log an audit event
    async fn log_event(&self, event: &AuditEvent) {
        // Check if this event type should be logged
        let should_log = match event.event_type {
            AuditEventType::AuthSuccess => self.config.log_auth_success,
            AuditEventType::AuthFailure => self.config.log_auth_failure,
            AuditEventType::AuthzFailure => self.config.log_authz_failure,
            AuditEventType::RateLimitExceeded => self.config.log_rate_limit,
            AuditEventType::TlsFailure => self.config.log_tls_failure,
            AuditEventType::AdminAccess => self.config.log_admin_access,
            AuditEventType::ConfigChange => self.config.log_config_change,
            AuditEventType::SuspiciousActivity => self.config.log_suspicious,
            AuditEventType::RequestBlocked => true, // Always log blocked requests
        };

        if !should_log {
            return;
        }

        // Serialize event
        let json = if self.config.pretty_print {
            serde_json::to_string_pretty(event)
        } else {
            serde_json::to_string(event)
        };

        let mut line = match json {
            Ok(j) => j,
            Err(e) => {
                tracing::error!(error = %e, "Failed to serialize audit event");
                return;
            }
        };
        line.push('\n');

        // Write to output
        match &self.config.output {
            AuditOutput::File(_) => {
                if let Some(ref handle) = self.file_handle {
                    if let Err(e) = handle.lock().await.write_all(line.as_bytes()).await {
                        tracing::error!(error = %e, "Failed to write audit log to file");
                    }
                }
            }
            AuditOutput::Stdout => {
                print!("{line}");
            }
            AuditOutput::Stderr => {
                eprint!("{line}");
            }
            AuditOutput::Custom(handler) => {
                handler.log(event);
            }
        }
    }

    fn extract_client_ip(&self, req: &Request<Body>) -> Option<String> {
        // Try X-Forwarded-For header first
        if let Some(forwarded) = req.headers().get("x-forwarded-for") {
            if let Ok(forwarded_str) = forwarded.to_str() {
                if let Some(first_ip) = forwarded_str.split(',').next() {
                    return Some(first_ip.trim().to_string());
                }
            }
        }

        // Try X-Real-IP header
        if let Some(real_ip) = req.headers().get("x-real-ip") {
            if let Ok(ip_str) = real_ip.to_str() {
                return Some(ip_str.to_string());
            }
        }

        None
    }

    fn extract_request_id(&self, req: &Request<Body>) -> Option<String> {
        req.headers()
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
    }

    fn extract_user_id(&self, req: &Request<Body>) -> Option<String> {
        // Try to extract from JWT claims in extensions
        // This would be populated by JWT middleware
        req.extensions()
            .get::<crate::jwt::Claims>()
            .map(|claims| claims.sub.clone())
    }

    fn should_audit_response(&self, status: StatusCode) -> Option<AuditEventType> {
        match status.as_u16() {
            401 => Some(AuditEventType::AuthFailure),
            403 => Some(AuditEventType::AuthzFailure),
            429 => Some(AuditEventType::RateLimitExceeded),
            _ => None,
        }
    }
}

#[async_trait]
impl Middleware for AuditLogger {
    async fn call(&self, req: Request<Body>, next: Next) -> Result<Response<Body>> {
        let method = req.method().to_string();
        let path = req.uri().path().to_string();
        let client_ip = self.extract_client_ip(&req);
        let request_id = self.extract_request_id(&req);
        let user_id = self.extract_user_id(&req);

        // Check if this is an admin path
        let is_admin = path.starts_with("/admin") || path.starts_with("/api/admin");

        // Process request
        let response = next.run(req).await?;
        let status = response.status();

        // Determine if we should log this response
        if let Some(event_type) = self.should_audit_response(status) {
            let mut event = AuditEvent::new(event_type);
            event.client_ip = client_ip.clone();
            event.user_id = user_id.clone();
            event.method = method.clone();
            event.path = path.clone();
            event.status_code = status.as_u16();
            event.request_id = request_id.clone();

            self.log_event(&event).await;
        }

        // Log admin access
        if is_admin && status.is_success() && self.config.log_admin_access {
            let mut event = AuditEvent::new(AuditEventType::AdminAccess);
            event.client_ip = client_ip;
            event.user_id = user_id;
            event.method = method;
            event.path = path;
            event.status_code = status.as_u16();
            event.request_id = request_id;

            self.log_event(&event).await;
        }

        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use http_body_util::Full;
    use std::sync::Arc;
    use tempfile::NamedTempFile;

    type TestBody = Full<Bytes>;

    // Mock handler for testing
    #[derive(Debug, Clone)]
    struct TestHandler {
        status: StatusCode,
    }

    #[async_trait]
    impl Middleware for TestHandler {
        async fn call(&self, _req: Request<TestBody>, _next: Next) -> Result<Response<TestBody>> {
            Ok(Response::builder()
                .status(self.status)
                .body(Full::new(Bytes::from("test")))
                .unwrap())
        }
    }

    #[tokio::test]
    async fn test_log_auth_failure() {
        let temp_file = NamedTempFile::new().unwrap();
        let config = AuditLoggerConfig {
            output: AuditOutput::File(temp_file.path().to_path_buf()),
            ..Default::default()
        };

        let logger = AuditLogger::with_config(config).await.unwrap();
        let handler = TestHandler {
            status: StatusCode::UNAUTHORIZED,
        };
        let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([Arc::new(logger), Arc::new(handler)]);

        let req = Request::builder()
            .uri("/api/test")
            .header("x-forwarded-for", "192.168.1.100")
            .body(Full::new(Bytes::from("")))
            .unwrap();

        let next = Next::new(stack);
        let response = next.run(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        // Check that audit log was written
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        let log_content = tokio::fs::read_to_string(temp_file.path()).await.unwrap();
        assert!(log_content.contains("auth_failure"));
        assert!(log_content.contains("192.168.1.100"));
    }

    #[tokio::test]
    async fn test_log_rate_limit() {
        let temp_file = NamedTempFile::new().unwrap();
        let config = AuditLoggerConfig {
            output: AuditOutput::File(temp_file.path().to_path_buf()),
            ..Default::default()
        };

        let logger = AuditLogger::with_config(config).await.unwrap();
        let handler = TestHandler {
            status: StatusCode::TOO_MANY_REQUESTS,
        };
        let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([Arc::new(logger), Arc::new(handler)]);

        let req = Request::builder()
            .uri("/api/test")
            .body(Full::new(Bytes::from("")))
            .unwrap();

        let next = Next::new(stack);
        let response = next.run(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);

        // Check that audit log was written
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        let log_content = tokio::fs::read_to_string(temp_file.path()).await.unwrap();
        assert!(log_content.contains("rate_limit_exceeded"));
    }

    #[tokio::test]
    async fn test_log_admin_access() {
        let temp_file = NamedTempFile::new().unwrap();
        let config = AuditLoggerConfig {
            output: AuditOutput::File(temp_file.path().to_path_buf()),
            ..Default::default()
        };

        let logger = AuditLogger::with_config(config).await.unwrap();
        let handler = TestHandler {
            status: StatusCode::OK,
        };
        let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([Arc::new(logger), Arc::new(handler)]);

        let req = Request::builder()
            .uri("/admin/config")
            .body(Full::new(Bytes::from("")))
            .unwrap();

        let next = Next::new(stack);
        let response = next.run(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // Check that audit log was written
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        let log_content = tokio::fs::read_to_string(temp_file.path()).await.unwrap();
        assert!(log_content.contains("admin_access"));
        assert!(log_content.contains("/admin/config"));
    }

    #[tokio::test]
    async fn test_stdout_logger() {
        let logger = AuditLogger::stdout();
        let handler = TestHandler {
            status: StatusCode::FORBIDDEN,
        };
        let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([Arc::new(logger), Arc::new(handler)]);

        let req = Request::builder()
            .uri("/api/test")
            .body(Full::new(Bytes::from("")))
            .unwrap();

        let next = Next::new(stack);
        let response = next.run(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}
