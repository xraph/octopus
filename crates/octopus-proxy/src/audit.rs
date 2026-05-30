//! Comprehensive audit logging for security events

use http::{Request, Response, StatusCode};
use octopus_core::UpstreamInstance;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::time::{Duration, SystemTime};
use tracing::{error, info, warn};

/// Audit event types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuditEventType {
    /// Request received
    RequestReceived,
    /// Request forwarded to upstream
    RequestForwarded,
    /// Response received from upstream
    ResponseReceived,
    /// Request completed successfully
    RequestCompleted,
    /// Request failed
    RequestFailed,
    /// Authentication attempt
    AuthenticationAttempt,
    /// Authentication success
    AuthenticationSuccess,
    /// Authentication failure
    AuthenticationFailure,
    /// Authorization check
    AuthorizationCheck,
    /// Authorization granted
    AuthorizationGranted,
    /// Authorization denied
    AuthorizationDenied,
    /// Rate limit exceeded
    RateLimitExceeded,
    /// Circuit breaker opened
    CircuitBreakerOpened,
    /// Circuit breaker closed
    CircuitBreakerClosed,
    /// TLS handshake failed
    TlsHandshakeFailed,
    /// Upstream connection failed
    UpstreamConnectionFailed,
    /// Request timeout
    RequestTimeout,
    /// Invalid request
    InvalidRequest,
    /// Configuration changed
    ConfigurationChanged,
    /// Security violation detected
    SecurityViolation,
}

/// Audit event containing security-relevant information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    /// Event ID (UUID)
    pub event_id: String,

    /// Event type
    pub event_type: AuditEventType,

    /// Timestamp
    pub timestamp: SystemTime,

    /// Client IP address
    pub client_ip: Option<IpAddr>,

    /// Request ID (for correlation)
    pub request_id: Option<String>,

    /// HTTP method
    pub method: Option<String>,

    /// Request URI
    pub uri: Option<String>,

    /// HTTP status code
    pub status_code: Option<u16>,

    /// Upstream instance ID
    pub upstream_id: Option<String>,

    /// User ID (if authenticated)
    pub user_id: Option<String>,

    /// Session ID
    pub session_id: Option<String>,

    /// Duration (for completed requests)
    pub duration: Option<Duration>,

    /// Error message (if applicable)
    pub error: Option<String>,

    /// Additional context
    pub context: serde_json::Value,
}

impl AuditEvent {
    /// Create a new audit event
    pub fn new(event_type: AuditEventType) -> Self {
        Self {
            event_id: uuid::Uuid::new_v4().to_string(),
            event_type,
            timestamp: SystemTime::now(),
            client_ip: None,
            request_id: None,
            method: None,
            uri: None,
            status_code: None,
            upstream_id: None,
            user_id: None,
            session_id: None,
            duration: None,
            error: None,
            context: serde_json::Value::Null,
        }
    }

    /// Set client IP
    pub fn with_client_ip(mut self, ip: IpAddr) -> Self {
        self.client_ip = Some(ip);
        self
    }

    /// Set request ID
    pub fn with_request_id(mut self, id: String) -> Self {
        self.request_id = Some(id);
        self
    }

    /// Set HTTP method
    pub fn with_method(mut self, method: String) -> Self {
        self.method = Some(method);
        self
    }

    /// Set URI
    pub fn with_uri(mut self, uri: String) -> Self {
        self.uri = Some(uri);
        self
    }

    /// Set status code
    pub fn with_status_code(mut self, code: u16) -> Self {
        self.status_code = Some(code);
        self
    }

    /// Set upstream ID
    pub fn with_upstream_id(mut self, id: String) -> Self {
        self.upstream_id = Some(id);
        self
    }

    /// Set user ID
    pub fn with_user_id(mut self, id: String) -> Self {
        self.user_id = Some(id);
        self
    }

    /// Set session ID
    pub fn with_session_id(mut self, id: String) -> Self {
        self.session_id = Some(id);
        self
    }

    /// Set duration
    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.duration = Some(duration);
        self
    }

    /// Set error message
    pub fn with_error(mut self, error: String) -> Self {
        self.error = Some(error);
        self
    }

    /// Set additional context
    pub fn with_context(mut self, context: serde_json::Value) -> Self {
        self.context = context;
        self
    }

    /// Check if this is a security-critical event
    pub fn is_security_critical(&self) -> bool {
        matches!(
            self.event_type,
            AuditEventType::AuthenticationFailure
                | AuditEventType::AuthorizationDenied
                | AuditEventType::SecurityViolation
                | AuditEventType::TlsHandshakeFailed
                | AuditEventType::InvalidRequest
        )
    }

    /// Check if this is an error event
    pub fn is_error(&self) -> bool {
        self.error.is_some()
            || matches!(
                self.event_type,
                AuditEventType::RequestFailed
                    | AuditEventType::AuthenticationFailure
                    | AuditEventType::AuthorizationDenied
                    | AuditEventType::UpstreamConnectionFailed
                    | AuditEventType::RequestTimeout
                    | AuditEventType::InvalidRequest
                    | AuditEventType::TlsHandshakeFailed
            )
    }
}

/// Audit logger for recording security events
#[derive(Clone)]
pub struct AuditLogger {
    /// Whether audit logging is enabled
    enabled: bool,

    /// Whether to log to structured logging (tracing)
    log_to_tracing: bool,

    /// Whether to log to a file
    log_to_file: bool,

    /// File path for audit log
    file_path: Option<String>,
}

impl AuditLogger {
    /// Create a new audit logger
    pub fn new() -> Self {
        Self {
            enabled: true,
            log_to_tracing: true,
            log_to_file: false,
            file_path: None,
        }
    }

    /// Enable or disable audit logging
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Enable or disable logging to tracing
    pub fn with_tracing(mut self, enabled: bool) -> Self {
        self.log_to_tracing = enabled;
        self
    }

    /// Enable logging to a file
    pub fn with_file(mut self, path: String) -> Self {
        self.log_to_file = true;
        self.file_path = Some(path);
        self
    }

    /// Log an audit event
    pub fn log(&self, event: &AuditEvent) {
        if !self.enabled {
            return;
        }

        // Log to structured logging
        if self.log_to_tracing {
            let event_json = serde_json::to_string(event).unwrap_or_else(|e| {
                format!("{{\"error\": \"Failed to serialize audit event: {e}\"}}")
            });

            if event.is_security_critical() {
                warn!(
                    event_type = ?event.event_type,
                    event_id = %event.event_id,
                    audit_event = %event_json,
                    "Security-critical audit event"
                );
            } else if event.is_error() {
                error!(
                    event_type = ?event.event_type,
                    event_id = %event.event_id,
                    audit_event = %event_json,
                    "Error audit event"
                );
            } else {
                info!(
                    event_type = ?event.event_type,
                    event_id = %event.event_id,
                    audit_event = %event_json,
                    "Audit event"
                );
            }
        }

        // Log to file (if enabled)
        if self.log_to_file {
            if let Some(ref path) = self.file_path {
                if let Err(e) = self.write_to_file(path, event) {
                    error!("Failed to write audit event to file: {}", e);
                }
            }
        }
    }

    /// Write audit event to file
    fn write_to_file(&self, path: &str, event: &AuditEvent) -> std::io::Result<()> {
        use std::fs::OpenOptions;
        use std::io::Write;

        let mut file = OpenOptions::new().create(true).append(true).open(path)?;

        let event_json = serde_json::to_string(event)?;
        writeln!(file, "{event_json}")?;

        Ok(())
    }

    /// Log request received
    pub fn log_request_received<B>(&self, req: &Request<B>, client_ip: Option<IpAddr>) {
        let event = AuditEvent::new(AuditEventType::RequestReceived)
            .with_method(req.method().to_string())
            .with_uri(req.uri().to_string());

        let event = if let Some(ip) = client_ip {
            event.with_client_ip(ip)
        } else {
            event
        };

        self.log(&event);
    }

    /// Log request forwarded
    pub fn log_request_forwarded<B>(
        &self,
        req: &Request<B>,
        upstream: &UpstreamInstance,
        request_id: Option<String>,
    ) {
        let mut event = AuditEvent::new(AuditEventType::RequestForwarded)
            .with_method(req.method().to_string())
            .with_uri(req.uri().to_string())
            .with_upstream_id(upstream.id.clone());

        if let Some(id) = request_id {
            event = event.with_request_id(id);
        }

        self.log(&event);
    }

    /// Log response received
    pub fn log_response_received<B>(
        &self,
        resp: &Response<B>,
        upstream: &UpstreamInstance,
        request_id: Option<String>,
    ) {
        let mut event = AuditEvent::new(AuditEventType::ResponseReceived)
            .with_status_code(resp.status().as_u16())
            .with_upstream_id(upstream.id.clone());

        if let Some(id) = request_id {
            event = event.with_request_id(id);
        }

        self.log(&event);
    }

    /// Log request completed
    pub fn log_request_completed(
        &self,
        status: StatusCode,
        duration: Duration,
        request_id: Option<String>,
    ) {
        let mut event = AuditEvent::new(AuditEventType::RequestCompleted)
            .with_status_code(status.as_u16())
            .with_duration(duration);

        if let Some(id) = request_id {
            event = event.with_request_id(id);
        }

        self.log(&event);
    }

    /// Log request failed
    pub fn log_request_failed(&self, error: String, request_id: Option<String>) {
        let mut event = AuditEvent::new(AuditEventType::RequestFailed).with_error(error);

        if let Some(id) = request_id {
            event = event.with_request_id(id);
        }

        self.log(&event);
    }

    /// Log authentication failure
    pub fn log_authentication_failure(&self, reason: String, client_ip: Option<IpAddr>) {
        let mut event = AuditEvent::new(AuditEventType::AuthenticationFailure).with_error(reason);

        if let Some(ip) = client_ip {
            event = event.with_client_ip(ip);
        }

        self.log(&event);
    }

    /// Log authorization denied
    pub fn log_authorization_denied(
        &self,
        user_id: String,
        resource: String,
        client_ip: Option<IpAddr>,
    ) {
        let mut event = AuditEvent::new(AuditEventType::AuthorizationDenied)
            .with_user_id(user_id)
            .with_context(serde_json::json!({ "resource": resource }));

        if let Some(ip) = client_ip {
            event = event.with_client_ip(ip);
        }

        self.log(&event);
    }

    /// Log rate limit exceeded
    pub fn log_rate_limit_exceeded(&self, client_ip: Option<IpAddr>) {
        let mut event = AuditEvent::new(AuditEventType::RateLimitExceeded);

        if let Some(ip) = client_ip {
            event = event.with_client_ip(ip);
        }

        self.log(&event);
    }

    /// Log circuit breaker opened
    pub fn log_circuit_breaker_opened(&self, upstream_id: String) {
        let event =
            AuditEvent::new(AuditEventType::CircuitBreakerOpened).with_upstream_id(upstream_id);

        self.log(&event);
    }

    /// Log security violation
    pub fn log_security_violation(&self, violation: String, client_ip: Option<IpAddr>) {
        let mut event = AuditEvent::new(AuditEventType::SecurityViolation).with_error(violation);

        if let Some(ip) = client_ip {
            event = event.with_client_ip(ip);
        }

        self.log(&event);
    }
}

impl Default for AuditLogger {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for AuditLogger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuditLogger")
            .field("enabled", &self.enabled)
            .field("log_to_tracing", &self.log_to_tracing)
            .field("log_to_file", &self.log_to_file)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_event_creation() {
        let event = AuditEvent::new(AuditEventType::RequestReceived)
            .with_method("GET".to_string())
            .with_uri("/api/test".to_string())
            .with_status_code(200);

        assert_eq!(event.event_type, AuditEventType::RequestReceived);
        assert_eq!(event.method, Some("GET".to_string()));
        assert_eq!(event.uri, Some("/api/test".to_string()));
        assert_eq!(event.status_code, Some(200));
    }

    #[test]
    fn test_security_critical_events() {
        let auth_failure = AuditEvent::new(AuditEventType::AuthenticationFailure);
        assert!(auth_failure.is_security_critical());
        assert!(auth_failure.is_error());

        let request_received = AuditEvent::new(AuditEventType::RequestReceived);
        assert!(!request_received.is_security_critical());
        assert!(!request_received.is_error());
    }

    #[test]
    fn test_audit_logger_creation() {
        let logger = AuditLogger::new().with_enabled(true).with_tracing(true);

        assert!(logger.enabled);
        assert!(logger.log_to_tracing);
    }

    #[test]
    fn test_audit_logger_disabled() {
        let logger = AuditLogger::new().with_enabled(false);
        let event = AuditEvent::new(AuditEventType::RequestReceived);

        // Should not panic when disabled
        logger.log(&event);
    }
}
