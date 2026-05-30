//! Complete header handling for proxy operations
//!
//! Implements standard proxy headers including:
//! - Forwarded headers (RFC 7239)
//! - Via header (RFC 7230)
//! - X-Forwarded-* headers (de facto standard)
//! - Trace context propagation

use http::{HeaderMap, HeaderName, HeaderValue, Request, Uri};
use std::net::IpAddr;
use tracing::debug;

/// Header names
pub mod header_names {

    /// Standard Forwarded header (RFC 7239)
    pub const FORWARDED: &str = "forwarded";

    /// Via header (RFC 7230)
    pub const VIA: &str = "via";

    /// X-Forwarded-For header
    pub const X_FORWARDED_FOR: &str = "x-forwarded-for";

    /// X-Forwarded-Host header
    pub const X_FORWARDED_HOST: &str = "x-forwarded-host";

    /// X-Forwarded-Proto header
    pub const X_FORWARDED_PROTO: &str = "x-forwarded-proto";

    /// X-Real-IP header
    pub const X_REAL_IP: &str = "x-real-ip";

    /// X-Request-ID header
    pub const X_REQUEST_ID: &str = "x-request-id";

    /// Server header
    pub const SERVER: &str = "server";

    /// Host header
    pub const HOST: &str = "host";
}

/// Configuration for header handling
#[derive(Debug, Clone)]
pub struct HeaderConfig {
    /// Add Forwarded header (RFC 7239)
    pub add_forwarded: bool,

    /// Add Via header
    pub add_via: bool,

    /// Add X-Forwarded-* headers
    pub add_x_forwarded: bool,

    /// Preserve client Host header
    pub preserve_host: bool,

    /// Add X-Request-ID if not present
    pub add_request_id: bool,

    /// Server identifier for Via header
    pub server_id: String,

    /// Remove hop-by-hop headers
    pub remove_hop_by_hop: bool,

    /// Custom headers to add to upstream requests
    pub custom_upstream_headers: Vec<(String, String)>,

    /// Custom headers to add to client responses
    pub custom_response_headers: Vec<(String, String)>,
}

impl Default for HeaderConfig {
    fn default() -> Self {
        Self {
            add_forwarded: true,
            add_via: true,
            add_x_forwarded: true,
            preserve_host: false,
            add_request_id: true,
            server_id: "octopus".to_string(),
            remove_hop_by_hop: true,
            custom_upstream_headers: Vec::new(),
            custom_response_headers: Vec::new(),
        }
    }
}

/// Header processor for request/response transformation
pub struct HeaderProcessor {
    config: HeaderConfig,
}

impl HeaderProcessor {
    /// Create a new header processor
    pub fn new(config: HeaderConfig) -> Self {
        Self { config }
    }

    /// Process incoming request headers before forwarding to upstream
    pub fn process_request_headers<B>(
        &self,
        req: &mut Request<B>,
        client_ip: Option<IpAddr>,
        original_uri: &Uri,
    ) {
        // Get request version before borrowing headers
        let version = req.version();
        let req_uri = req.uri().clone();

        let headers = req.headers_mut();

        // Remove hop-by-hop headers
        if self.config.remove_hop_by_hop {
            self.remove_hop_by_hop_headers(headers);
        }

        // Add Forwarded header (RFC 7239)
        if self.config.add_forwarded {
            self.add_forwarded_header(headers, client_ip, original_uri);
        }

        // Add X-Forwarded-* headers
        if self.config.add_x_forwarded {
            self.add_x_forwarded_headers(headers, client_ip, original_uri);
        }

        // Add Via header
        if self.config.add_via {
            self.add_via_header(headers, version);
        }

        // Add X-Request-ID if not present
        if self.config.add_request_id && !headers.contains_key(header_names::X_REQUEST_ID) {
            self.add_request_id(headers);
        }

        // Handle Host header
        if !self.config.preserve_host {
            self.update_host_header(headers, &req_uri);
        }

        // Add custom upstream headers
        for (name, value) in &self.config.custom_upstream_headers {
            if let (Ok(header_name), Ok(header_value)) = (
                HeaderName::from_bytes(name.as_bytes()),
                HeaderValue::from_str(value),
            ) {
                headers.insert(header_name, header_value);
            }
        }

        debug!(
            forwarded = self.config.add_forwarded,
            via = self.config.add_via,
            "Processed request headers"
        );
    }

    /// Process response headers before sending to client
    pub fn process_response_headers(&self, headers: &mut HeaderMap) {
        // Remove hop-by-hop headers
        if self.config.remove_hop_by_hop {
            self.remove_hop_by_hop_headers(headers);
        }

        // Add Via header to response
        if self.config.add_via {
            self.add_via_header_response(headers);
        }

        // Add custom response headers
        for (name, value) in &self.config.custom_response_headers {
            if let (Ok(header_name), Ok(header_value)) = (
                HeaderName::from_bytes(name.as_bytes()),
                HeaderValue::from_str(value),
            ) {
                headers.insert(header_name, header_value);
            }
        }

        debug!("Processed response headers");
    }

    /// Remove hop-by-hop headers as per RFC 7230
    fn remove_hop_by_hop_headers(&self, headers: &mut HeaderMap) {
        // Standard hop-by-hop headers
        const HOP_BY_HOP: &[&str] = &[
            "connection",
            "keep-alive",
            "proxy-authenticate",
            "proxy-authorization",
            "te",
            "trailer",
            "transfer-encoding",
            "upgrade",
        ];

        // Collect headers listed in Connection header before modifying
        let connection_headers: Vec<String> = headers
            .get("connection")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.split(',').map(|name| name.trim().to_string()).collect())
            .unwrap_or_default();

        // Remove standard hop-by-hop headers
        for header in HOP_BY_HOP {
            headers.remove(*header);
        }

        // Remove headers listed in Connection header
        for name in connection_headers {
            if let Ok(header_name) = HeaderName::from_bytes(name.as_bytes()) {
                headers.remove(header_name);
            }
        }
    }

    /// Add Forwarded header (RFC 7239)
    fn add_forwarded_header(&self, headers: &mut HeaderMap, client_ip: Option<IpAddr>, uri: &Uri) {
        let mut parts = Vec::new();

        // Add "for" parameter (client IP)
        if let Some(ip) = client_ip {
            parts.push(format!("for={ip}"));
        }

        // Add "host" parameter (original host)
        if let Some(host) = uri.host() {
            parts.push(format!("host={host}"));
        }

        // Add "proto" parameter (original protocol)
        let proto = uri.scheme_str().unwrap_or("http");
        parts.push(format!("proto={proto}"));

        if !parts.is_empty() {
            let forwarded_value = parts.join(";");

            // Get existing value before modifying
            let existing = headers
                .get(header_names::FORWARDED)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());

            let final_value = if let Some(existing_str) = existing {
                format!("{existing_str}, {forwarded_value}")
            } else {
                forwarded_value
            };

            if let Ok(value) = HeaderValue::from_str(&final_value) {
                headers.insert(HeaderName::from_static(header_names::FORWARDED), value);
            }
        }
    }

    /// Add X-Forwarded-* headers
    fn add_x_forwarded_headers(
        &self,
        headers: &mut HeaderMap,
        client_ip: Option<IpAddr>,
        uri: &Uri,
    ) {
        // X-Forwarded-For
        if let Some(ip) = client_ip {
            let ip_str = ip.to_string();

            // Get existing value before modifying
            let existing = headers
                .get(header_names::X_FORWARDED_FOR)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());
            let has_real_ip = headers.contains_key(header_names::X_REAL_IP);

            let final_value = if let Some(existing_str) = existing {
                format!("{existing_str}, {ip_str}")
            } else {
                ip_str.clone()
            };

            if let Ok(value) = HeaderValue::from_str(&final_value) {
                headers.insert(
                    HeaderName::from_static(header_names::X_FORWARDED_FOR),
                    value,
                );
            }

            // Also set X-Real-IP if not present
            if !has_real_ip {
                if let Ok(value) = HeaderValue::from_str(&ip_str) {
                    headers.insert(HeaderName::from_static(header_names::X_REAL_IP), value);
                }
            }
        }

        // X-Forwarded-Host
        if let Some(host) = uri.host() {
            if !headers.contains_key(header_names::X_FORWARDED_HOST) {
                if let Ok(value) = HeaderValue::from_str(host) {
                    headers.insert(
                        HeaderName::from_static(header_names::X_FORWARDED_HOST),
                        value,
                    );
                }
            }
        }

        // X-Forwarded-Proto
        let proto = uri.scheme_str().unwrap_or("http");
        if !headers.contains_key(header_names::X_FORWARDED_PROTO) {
            if let Ok(value) = HeaderValue::from_str(proto) {
                headers.insert(
                    HeaderName::from_static(header_names::X_FORWARDED_PROTO),
                    value,
                );
            }
        }
    }

    /// Add Via header for request (RFC 7230)
    fn add_via_header(&self, headers: &mut HeaderMap, version: http::Version) {
        let protocol_version = match version {
            http::Version::HTTP_09 => "0.9",
            http::Version::HTTP_10 => "1.0",
            http::Version::HTTP_11 => "1.1",
            http::Version::HTTP_2 => "2",
            http::Version::HTTP_3 => "3",
            _ => "1.1",
        };

        let via_value = format!("{} {}", protocol_version, self.config.server_id);

        if let Ok(value) = HeaderValue::from_str(&via_value) {
            if let Some(existing) = headers.get(header_names::VIA) {
                if let Ok(existing_str) = existing.to_str() {
                    let combined = format!("{existing_str}, {via_value}");
                    if let Ok(combined_value) = HeaderValue::from_str(&combined) {
                        headers.insert(HeaderName::from_static(header_names::VIA), combined_value);
                    }
                }
            } else {
                headers.insert(HeaderName::from_static(header_names::VIA), value);
            }
        }
    }

    /// Add Via header for response
    fn add_via_header_response(&self, headers: &mut HeaderMap) {
        let via_value = format!("1.1 {}", self.config.server_id);

        if let Ok(value) = HeaderValue::from_str(&via_value) {
            if let Some(existing) = headers.get(header_names::VIA) {
                if let Ok(existing_str) = existing.to_str() {
                    let combined = format!("{existing_str}, {via_value}");
                    if let Ok(combined_value) = HeaderValue::from_str(&combined) {
                        headers.insert(HeaderName::from_static(header_names::VIA), combined_value);
                    }
                }
            } else {
                headers.insert(HeaderName::from_static(header_names::VIA), value);
            }
        }
    }

    /// Add X-Request-ID header
    fn add_request_id(&self, headers: &mut HeaderMap) {
        let request_id = uuid::Uuid::new_v4().to_string();
        if let Ok(value) = HeaderValue::from_str(&request_id) {
            headers.insert(HeaderName::from_static(header_names::X_REQUEST_ID), value);
            debug!(request_id = %request_id, "Added X-Request-ID header");
        }
    }

    /// Update Host header based on upstream URI
    fn update_host_header(&self, headers: &mut HeaderMap, uri: &Uri) {
        if let Some(host) = uri.host() {
            let host_value = if let Some(port) = uri.port_u16() {
                format!("{host}:{port}")
            } else {
                host.to_string()
            };

            if let Ok(value) = HeaderValue::from_str(&host_value) {
                headers.insert(HeaderName::from_static(header_names::HOST), value);
            }
        }
    }

    /// Get configuration
    pub fn config(&self) -> &HeaderConfig {
        &self.config
    }
}

impl std::fmt::Debug for HeaderProcessor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeaderProcessor")
            .field("config", &self.config)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::Request;
    use std::str::FromStr;

    #[test]
    fn test_remove_hop_by_hop_headers() {
        let config = HeaderConfig::default();
        let processor = HeaderProcessor::new(config);

        let mut headers = HeaderMap::new();
        headers.insert("connection", HeaderValue::from_static("keep-alive"));
        headers.insert("keep-alive", HeaderValue::from_static("timeout=5"));
        headers.insert("content-type", HeaderValue::from_static("application/json"));

        processor.remove_hop_by_hop_headers(&mut headers);

        assert!(!headers.contains_key("connection"));
        assert!(!headers.contains_key("keep-alive"));
        assert!(headers.contains_key("content-type"));
    }

    #[test]
    fn test_add_forwarded_header() {
        let config = HeaderConfig::default();
        let processor = HeaderProcessor::new(config);

        let mut headers = HeaderMap::new();
        let client_ip = IpAddr::from_str("192.168.1.1").unwrap();
        let uri = Uri::from_static("http://example.com/path");

        processor.add_forwarded_header(&mut headers, Some(client_ip), &uri);

        let forwarded = headers.get("forwarded").unwrap();
        let forwarded_str = forwarded.to_str().unwrap();

        assert!(forwarded_str.contains("for=192.168.1.1"));
        assert!(forwarded_str.contains("host=example.com"));
        assert!(forwarded_str.contains("proto=http"));
    }

    #[test]
    fn test_add_x_forwarded_headers() {
        let config = HeaderConfig::default();
        let processor = HeaderProcessor::new(config);

        let mut headers = HeaderMap::new();
        let client_ip = IpAddr::from_str("192.168.1.1").unwrap();
        let uri = Uri::from_static("https://example.com/path");

        processor.add_x_forwarded_headers(&mut headers, Some(client_ip), &uri);

        assert_eq!(
            headers.get("x-forwarded-for").unwrap(),
            &HeaderValue::from_static("192.168.1.1")
        );
        assert_eq!(
            headers.get("x-real-ip").unwrap(),
            &HeaderValue::from_static("192.168.1.1")
        );
        assert_eq!(
            headers.get("x-forwarded-host").unwrap(),
            &HeaderValue::from_static("example.com")
        );
        assert_eq!(
            headers.get("x-forwarded-proto").unwrap(),
            &HeaderValue::from_static("https")
        );
    }

    #[test]
    fn test_add_via_header() {
        let config = HeaderConfig {
            server_id: "test-proxy".to_string(),
            ..Default::default()
        };
        let processor = HeaderProcessor::new(config);

        let mut headers = HeaderMap::new();
        processor.add_via_header(&mut headers, http::Version::HTTP_11);

        let via = headers.get("via").unwrap();
        assert_eq!(via.to_str().unwrap(), "1.1 test-proxy");
    }

    #[test]
    fn test_append_via_header() {
        let config = HeaderConfig {
            server_id: "proxy2".to_string(),
            ..Default::default()
        };
        let processor = HeaderProcessor::new(config);

        let mut headers = HeaderMap::new();
        headers.insert("via", HeaderValue::from_static("1.1 proxy1"));

        processor.add_via_header(&mut headers, http::Version::HTTP_11);

        let via = headers.get("via").unwrap();
        assert_eq!(via.to_str().unwrap(), "1.1 proxy1, 1.1 proxy2");
    }

    #[test]
    fn test_add_request_id() {
        let config = HeaderConfig::default();
        let processor = HeaderProcessor::new(config);

        let mut headers = HeaderMap::new();
        processor.add_request_id(&mut headers);

        assert!(headers.contains_key("x-request-id"));
        let request_id = headers.get("x-request-id").unwrap().to_str().unwrap();
        assert_eq!(request_id.len(), 36); // UUID format
    }

    #[test]
    fn test_update_host_header() {
        let config = HeaderConfig::default();
        let processor = HeaderProcessor::new(config);

        let mut headers = HeaderMap::new();
        let uri = Uri::from_static("http://upstream.example.com:8080/path");

        processor.update_host_header(&mut headers, &uri);

        assert_eq!(
            headers.get("host").unwrap(),
            &HeaderValue::from_static("upstream.example.com:8080")
        );
    }

    #[test]
    fn test_process_request_headers() {
        let config = HeaderConfig::default();
        let processor = HeaderProcessor::new(config);

        let mut req = Request::builder()
            .uri("http://example.com/path")
            .header("content-type", "application/json")
            .header("connection", "keep-alive")
            .body(())
            .unwrap();

        let client_ip = IpAddr::from_str("192.168.1.1").ok();
        let original_uri = Uri::from_static("http://example.com/path");

        processor.process_request_headers(&mut req, client_ip, &original_uri);

        let headers = req.headers();
        assert!(!headers.contains_key("connection")); // Removed
        assert!(headers.contains_key("forwarded"));
        assert!(headers.contains_key("x-forwarded-for"));
        assert!(headers.contains_key("via"));
        assert!(headers.contains_key("x-request-id"));
    }
}
