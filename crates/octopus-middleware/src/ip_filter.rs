//! IP filtering middleware with allowlist/blocklist support

use async_trait::async_trait;
use bytes::Bytes;
use http::{Request, Response, StatusCode};
use http_body_util::Full;
use octopus_core::{Middleware, Next, Result};
use std::fmt;
use std::net::IpAddr;
use std::str::FromStr;

/// Body type alias
pub type Body = Full<Bytes>;

/// IP pattern for matching
#[derive(Debug, Clone)]
pub enum IpPattern {
    /// Exact IP address match
    Exact(IpAddr),
    /// CIDR range match (base address + prefix length)
    Cidr(IpAddr, u8),
    /// IP range match (inclusive start to inclusive end)
    Range(IpAddr, IpAddr),
}

impl IpPattern {
    /// Parse a string into an IpPattern.
    ///
    /// Supports:
    /// - Exact: "192.168.1.1"
    /// - CIDR: "192.168.1.0/24"
    /// - Range: "192.168.1.1-192.168.1.254"
    pub fn parse(s: &str) -> std::result::Result<Self, String> {
        if let Some((base, prefix)) = s.split_once('/') {
            let addr = IpAddr::from_str(base).map_err(|e| format!("invalid IP in CIDR: {e}"))?;
            let prefix_len: u8 = prefix
                .parse()
                .map_err(|e| format!("invalid prefix length: {e}"))?;
            let max_prefix = if addr.is_ipv4() { 32 } else { 128 };
            if prefix_len > max_prefix {
                return Err(format!(
                    "prefix length {prefix_len} exceeds maximum {max_prefix}"
                ));
            }
            Ok(IpPattern::Cidr(addr, prefix_len))
        } else if let Some((start, end)) = s.split_once('-') {
            let start_addr =
                IpAddr::from_str(start).map_err(|e| format!("invalid start IP: {e}"))?;
            let end_addr = IpAddr::from_str(end).map_err(|e| format!("invalid end IP: {e}"))?;
            Ok(IpPattern::Range(start_addr, end_addr))
        } else {
            let addr = IpAddr::from_str(s).map_err(|e| format!("invalid IP address: {e}"))?;
            Ok(IpPattern::Exact(addr))
        }
    }

    /// Check if an IP address matches this pattern
    pub fn matches(&self, ip: &IpAddr) -> bool {
        match self {
            IpPattern::Exact(pattern_ip) => ip == pattern_ip,
            IpPattern::Cidr(base, prefix_len) => cidr_matches(base, *prefix_len, ip),
            IpPattern::Range(start, end) => range_matches(start, end, ip),
        }
    }
}

/// Check if an IP is within a CIDR range
fn cidr_matches(base: &IpAddr, prefix_len: u8, candidate: &IpAddr) -> bool {
    match (base, candidate) {
        (IpAddr::V4(base_v4), IpAddr::V4(cand_v4)) => {
            if prefix_len == 0 {
                return true;
            }
            if prefix_len >= 32 {
                return base_v4 == cand_v4;
            }
            let base_bits = u32::from(*base_v4);
            let cand_bits = u32::from(*cand_v4);
            let mask = !0u32 << (32 - prefix_len);
            (base_bits & mask) == (cand_bits & mask)
        }
        (IpAddr::V6(base_v6), IpAddr::V6(cand_v6)) => {
            if prefix_len == 0 {
                return true;
            }
            if prefix_len >= 128 {
                return base_v6 == cand_v6;
            }
            let base_bits = u128::from(*base_v6);
            let cand_bits = u128::from(*cand_v6);
            let mask = !0u128 << (128 - prefix_len);
            (base_bits & mask) == (cand_bits & mask)
        }
        _ => false, // IPv4 vs IPv6 mismatch
    }
}

/// Check if an IP is within a range (inclusive)
fn range_matches(start: &IpAddr, end: &IpAddr, candidate: &IpAddr) -> bool {
    match (start, end, candidate) {
        (IpAddr::V4(s), IpAddr::V4(e), IpAddr::V4(c)) => {
            let s = u32::from(*s);
            let e = u32::from(*e);
            let c = u32::from(*c);
            c >= s && c <= e
        }
        (IpAddr::V6(s), IpAddr::V6(e), IpAddr::V6(c)) => {
            let s = u128::from(*s);
            let e = u128::from(*e);
            let c = u128::from(*c);
            c >= s && c <= e
        }
        _ => false,
    }
}

/// IP filter configuration
#[derive(Debug, Clone)]
pub struct IpFilterConfig {
    /// Whether the filter is enabled
    pub enabled: bool,
    /// Allowlist — if non-empty, only these IPs are permitted
    pub allow_ips: Vec<IpPattern>,
    /// Blocklist — checked first, deny takes precedence
    pub deny_ips: Vec<IpPattern>,
    /// Whether to trust X-Forwarded-For header for client IP extraction
    pub trust_forwarded_for: bool,
    /// Custom rejection message
    pub rejection_message: Option<String>,
}

impl Default for IpFilterConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            allow_ips: Vec::new(),
            deny_ips: Vec::new(),
            trust_forwarded_for: true,
            rejection_message: None,
        }
    }
}

/// IP filtering middleware
///
/// Filters requests based on client IP address using allowlists and blocklists.
/// Deny rules take precedence over allow rules.
/// Extracts client IP from `X-Forwarded-For` header or falls back to `X-Real-IP`.
#[derive(Clone)]
pub struct IpFilter {
    config: IpFilterConfig,
}

impl IpFilter {
    /// Create a new IpFilter with default config (disabled, no rules)
    pub fn new() -> Self {
        Self {
            config: IpFilterConfig::default(),
        }
    }

    /// Create a new IpFilter with custom config
    pub fn with_config(config: IpFilterConfig) -> Self {
        Self { config }
    }

    /// Extract client IP from request headers or connection info
    fn extract_client_ip<B>(&self, req: &Request<B>) -> Option<IpAddr> {
        if self.config.trust_forwarded_for {
            // Try X-Forwarded-For first (first entry is the original client)
            if let Some(xff) = req.headers().get("x-forwarded-for") {
                if let Ok(xff_str) = xff.to_str() {
                    if let Some(first_ip) = xff_str.split(',').next() {
                        if let Ok(ip) = IpAddr::from_str(first_ip.trim()) {
                            return Some(ip);
                        }
                    }
                }
            }

            // Try X-Real-IP
            if let Some(real_ip) = req.headers().get("x-real-ip") {
                if let Ok(ip_str) = real_ip.to_str() {
                    if let Ok(ip) = IpAddr::from_str(ip_str.trim()) {
                        return Some(ip);
                    }
                }
            }
        }

        None
    }

    /// Check if an IP is allowed by the filter rules
    fn is_allowed(&self, ip: &IpAddr) -> bool {
        // Deny list is checked first — deny takes precedence
        for pattern in &self.config.deny_ips {
            if pattern.matches(ip) {
                return false;
            }
        }

        // If allow list is non-empty, IP must match at least one entry
        if !self.config.allow_ips.is_empty() {
            return self
                .config
                .allow_ips
                .iter()
                .any(|pattern| pattern.matches(ip));
        }

        // No deny match and no allow list → allow all
        true
    }

    /// Build a 403 Forbidden response
    fn forbidden_response(&self) -> Response<Body> {
        let message = self
            .config
            .rejection_message
            .as_deref()
            .unwrap_or("Forbidden");

        Response::builder()
            .status(StatusCode::FORBIDDEN)
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(
                serde_json::json!({
                    "error": "forbidden",
                    "message": message
                })
                .to_string(),
            )))
            .expect("Failed to build forbidden response")
    }
}

impl Default for IpFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for IpFilter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IpFilter")
            .field("enabled", &self.config.enabled)
            .field("allow_count", &self.config.allow_ips.len())
            .field("deny_count", &self.config.deny_ips.len())
            .finish()
    }
}

#[async_trait]
impl Middleware for IpFilter {
    async fn call(&self, req: Request<Body>, next: Next) -> Result<Response<Body>> {
        if !self.config.enabled {
            return next.run(req).await;
        }

        let client_ip = self.extract_client_ip(&req);

        if let Some(ip) = client_ip {
            if !self.is_allowed(&ip) {
                tracing::warn!(
                    client_ip = %ip,
                    uri = %req.uri(),
                    "IP address denied by filter"
                );
                return Ok(self.forbidden_response());
            }
        } else if !self.config.allow_ips.is_empty() {
            // Can't determine IP but allowlist is active → deny
            tracing::warn!(
                uri = %req.uri(),
                "Could not determine client IP with active allowlist, denying"
            );
            return Ok(self.forbidden_response());
        }

        next.run(req).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use octopus_core::Error;
    use std::net::{Ipv4Addr, Ipv6Addr};
    use std::sync::Arc;

    #[derive(Debug)]
    struct TestHandler;

    #[async_trait]
    impl Middleware for TestHandler {
        async fn call(&self, _req: Request<Body>, _next: Next) -> Result<Response<Body>> {
            Response::builder()
                .status(StatusCode::OK)
                .body(Full::new(Bytes::from("success")))
                .map_err(|e| Error::Internal(e.to_string()))
        }
    }

    fn make_stack(filter: IpFilter) -> Arc<[Arc<dyn Middleware>]> {
        Arc::new([
            Arc::new(filter) as Arc<dyn Middleware>,
            Arc::new(TestHandler) as Arc<dyn Middleware>,
        ])
    }

    fn req_with_ip(ip: &str) -> Request<Body> {
        Request::builder()
            .uri("/test")
            .header("X-Forwarded-For", ip)
            .body(Body::from(""))
            .unwrap()
    }

    #[tokio::test]
    async fn test_disabled_allows_all() {
        let config = IpFilterConfig {
            enabled: false,
            deny_ips: vec![IpPattern::parse("192.168.1.1").unwrap()],
            ..Default::default()
        };
        let stack = make_stack(IpFilter::with_config(config));
        let next = Next::new(stack);
        let resp = next.run(req_with_ip("192.168.1.1")).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_allow_all_when_no_rules() {
        let config = IpFilterConfig::default();
        let stack = make_stack(IpFilter::with_config(config));
        let next = Next::new(stack);
        let resp = next.run(req_with_ip("1.2.3.4")).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_deny_exact_ip() {
        let config = IpFilterConfig {
            deny_ips: vec![IpPattern::Exact(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)))],
            ..Default::default()
        };
        let stack = make_stack(IpFilter::with_config(config));
        let next = Next::new(stack);
        let resp = next.run(req_with_ip("10.0.0.1")).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_deny_cidr_range() {
        let config = IpFilterConfig {
            deny_ips: vec![IpPattern::parse("192.168.1.0/24").unwrap()],
            ..Default::default()
        };
        let stack = make_stack(IpFilter::with_config(config));

        // IP within CIDR → denied
        let next = Next::new(stack.clone());
        let resp = next.run(req_with_ip("192.168.1.100")).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // IP outside CIDR → allowed
        let next = Next::new(stack);
        let resp = next.run(req_with_ip("192.168.2.1")).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_allow_exact_ip() {
        let config = IpFilterConfig {
            allow_ips: vec![IpPattern::Exact(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5)))],
            ..Default::default()
        };
        let stack = make_stack(IpFilter::with_config(config));

        // Allowed IP → OK
        let next = Next::new(stack.clone());
        let resp = next.run(req_with_ip("10.0.0.5")).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Non-allowed IP → denied
        let next = Next::new(stack);
        let resp = next.run(req_with_ip("10.0.0.6")).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_allow_cidr_range() {
        let config = IpFilterConfig {
            allow_ips: vec![IpPattern::parse("10.0.0.0/8").unwrap()],
            ..Default::default()
        };
        let stack = make_stack(IpFilter::with_config(config));

        let next = Next::new(stack.clone());
        let resp = next.run(req_with_ip("10.255.255.1")).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let next = Next::new(stack);
        let resp = next.run(req_with_ip("11.0.0.1")).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_deny_takes_precedence_over_allow() {
        let config = IpFilterConfig {
            allow_ips: vec![IpPattern::parse("10.0.0.0/8").unwrap()],
            deny_ips: vec![IpPattern::Exact(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 99)))],
            ..Default::default()
        };
        let stack = make_stack(IpFilter::with_config(config));

        // 10.0.0.99 is in allow CIDR but also in deny → denied
        let next = Next::new(stack.clone());
        let resp = next.run(req_with_ip("10.0.0.99")).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // 10.0.0.1 is in allow CIDR and not in deny → allowed
        let next = Next::new(stack);
        let resp = next.run(req_with_ip("10.0.0.1")).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_extracts_ip_from_x_forwarded_for() {
        let config = IpFilterConfig {
            deny_ips: vec![IpPattern::Exact(IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4)))],
            ..Default::default()
        };
        let stack = make_stack(IpFilter::with_config(config));
        let next = Next::new(stack);

        // X-Forwarded-For with multiple entries: first one is client
        let req = Request::builder()
            .uri("/test")
            .header("X-Forwarded-For", "1.2.3.4, 5.6.7.8")
            .body(Body::from(""))
            .unwrap();
        let resp = next.run(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_extracts_ip_from_x_real_ip() {
        let config = IpFilterConfig {
            deny_ips: vec![IpPattern::Exact(IpAddr::V4(Ipv4Addr::new(9, 9, 9, 9)))],
            ..Default::default()
        };
        let stack = make_stack(IpFilter::with_config(config));
        let next = Next::new(stack);

        let req = Request::builder()
            .uri("/test")
            .header("X-Real-IP", "9.9.9.9")
            .body(Body::from(""))
            .unwrap();
        let resp = next.run(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_ipv6_exact_match() {
        let ip = IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1));
        let config = IpFilterConfig {
            deny_ips: vec![IpPattern::Exact(ip)],
            ..Default::default()
        };
        let stack = make_stack(IpFilter::with_config(config));
        let next = Next::new(stack);
        let resp = next.run(req_with_ip("2001:db8::1")).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_ipv6_cidr_match() {
        let config = IpFilterConfig {
            deny_ips: vec![IpPattern::parse("2001:db8::/32").unwrap()],
            ..Default::default()
        };
        let stack = make_stack(IpFilter::with_config(config));

        let next = Next::new(stack.clone());
        let resp = next.run(req_with_ip("2001:db8::ffff")).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        let next = Next::new(stack);
        let resp = next.run(req_with_ip("2001:db9::1")).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_ip_range_match() {
        let config = IpFilterConfig {
            deny_ips: vec![IpPattern::parse("192.168.1.10-192.168.1.20").unwrap()],
            ..Default::default()
        };
        let stack = make_stack(IpFilter::with_config(config));

        let next = Next::new(stack.clone());
        let resp = next.run(req_with_ip("192.168.1.15")).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        let next = Next::new(stack.clone());
        let resp = next.run(req_with_ip("192.168.1.10")).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        let next = Next::new(stack);
        let resp = next.run(req_with_ip("192.168.1.21")).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_empty_allow_list_allows_all() {
        let config = IpFilterConfig {
            allow_ips: vec![],
            deny_ips: vec![],
            ..Default::default()
        };
        let stack = make_stack(IpFilter::with_config(config));
        let next = Next::new(stack);
        let resp = next.run(req_with_ip("99.99.99.99")).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[test]
    fn test_ip_pattern_parse() {
        assert!(IpPattern::parse("192.168.1.1").is_ok());
        assert!(IpPattern::parse("192.168.1.0/24").is_ok());
        assert!(IpPattern::parse("10.0.0.1-10.0.0.255").is_ok());
        assert!(IpPattern::parse("2001:db8::1").is_ok());
        assert!(IpPattern::parse("2001:db8::/32").is_ok());
        assert!(IpPattern::parse("invalid").is_err());
        assert!(IpPattern::parse("192.168.1.0/33").is_err());
    }
}
