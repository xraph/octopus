//! Host (authority) matching for routes.
//!
//! A route may be scoped to a request host (the HTTP `Host` header / HTTP/2
//! `:authority`). This mirrors the Kubernetes Gateway API `hostnames` model:
//! a route matches either any host, an exact host, or a wildcard suffix
//! (`*.example.com`). When several routes match the same method+path, the one
//! with the most specific host wins (exact > wildcard > any).
//!
//! Hosts are compared in lowercase. Callers normalize the request host with
//! [`normalize_request_host`]; patterns are lowercased by [`HostMatch::parse`]
//! (lenient, for discovered input) or [`HostMatch::parse_pattern`] (strict, for
//! configuration and API input).

/// How a route matches against the request host.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum HostMatch {
    /// Matches any host (host-agnostic route — the legacy default).
    #[default]
    Any,
    /// Matches any subdomain under a base. Stored with a leading dot, e.g.
    /// `.example.com` (parsed from `*.example.com`). Matches `a.example.com`
    /// and `a.b.example.com`, but not the apex `example.com`.
    Wildcard(String),
    /// Matches exactly one host, e.g. `api.example.com`.
    Exact(String),
}

impl HostMatch {
    /// Parse a Gateway-API-style hostname pattern.
    ///
    /// - `""` or `"*"` → [`HostMatch::Any`]
    /// - `"*.example.com"` → [`HostMatch::Wildcard`] (stored as `.example.com`)
    /// - `"api.example.com"` → [`HostMatch::Exact`]
    pub fn parse(pattern: &str) -> Self {
        let pattern = pattern.trim();
        if pattern.is_empty() || pattern == "*" {
            return HostMatch::Any;
        }
        if let Some(rest) = pattern.strip_prefix("*.") {
            return HostMatch::Wildcard(format!(".{}", rest.to_ascii_lowercase()));
        }
        HostMatch::Exact(pattern.to_ascii_lowercase())
    }

    /// Strictly parse a hostname pattern from configuration or API input.
    ///
    /// Unlike [`HostMatch::parse`], this never yields [`HostMatch::Any`]: an
    /// operator matches any host by giving an empty hosts list instead.
    ///
    /// - The input is trimmed, lowercased and loses one trailing `.`.
    /// - `*.` is allowed only as a prefix (`*.example.com` becomes
    ///   [`HostMatch::Wildcard`] stored as `.example.com`).
    /// - The rest must be 1 to 253 characters of dot-separated labels, each
    ///   matching `[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?`.
    pub fn parse_pattern(pattern: &str) -> Result<HostMatch, String> {
        let lowered = pattern.trim().to_ascii_lowercase();
        let host = lowered.strip_suffix('.').unwrap_or(&lowered);

        if host.is_empty() {
            return Err(format!("host '{pattern}': must not be empty"));
        }
        if host == "*" {
            return Err(format!(
                "host '{pattern}': use an empty hosts list to match any host"
            ));
        }

        let (wildcard, rest) = match host.strip_prefix("*.") {
            Some(rest) => (true, rest),
            None => (false, host),
        };
        if rest.contains('*') {
            return Err(format!(
                "host '{pattern}': '*' is only allowed as the first label ('*.example.com')"
            ));
        }
        if rest.is_empty() || rest.len() > 253 {
            return Err(format!(
                "host '{pattern}': must be 1 to 253 characters long"
            ));
        }
        for label in rest.split('.') {
            if !is_valid_label(label) {
                return Err(format!(
                    "host '{pattern}': label '{label}' must be 1 to 63 characters of a-z, 0-9 \
                     and '-', and must not start or end with '-'"
                ));
            }
        }

        Ok(if wildcard {
            HostMatch::Wildcard(format!(".{rest}"))
        } else {
            HostMatch::Exact(rest.to_string())
        })
    }

    /// The pattern string this matcher was parsed from: `None` for
    /// [`HostMatch::Any`], `"*.example.com"` for a wildcard, the host for an
    /// exact match. The inverse of [`HostMatch::parse_pattern`].
    pub fn pattern(&self) -> Option<String> {
        match self {
            HostMatch::Any => None,
            HostMatch::Exact(h) => Some(h.clone()),
            HostMatch::Wildcard(suffix) => Some(format!("*{suffix}")),
        }
    }

    /// Whether this matcher accepts `host` (which must already be lowercased).
    pub fn matches(&self, host: &str) -> bool {
        match self {
            HostMatch::Any => true,
            HostMatch::Exact(h) => host == h,
            // suffix carries a leading dot (".example.com"), so requiring the
            // host to be strictly longer guarantees a label sits before the dot
            // — matching subdomains but never the apex.
            HostMatch::Wildcard(suffix) => host.len() > suffix.len() && host.ends_with(suffix),
        }
    }

    /// Specificity rank for tie-breaking: exact (2) > wildcard (1) > any (0).
    pub fn specificity(&self) -> u8 {
        match self {
            HostMatch::Exact(_) => 2,
            HostMatch::Wildcard(_) => 1,
            HostMatch::Any => 0,
        }
    }
}

/// One DNS label: `[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?`.
fn is_valid_label(label: &str) -> bool {
    let bytes = label.as_bytes();
    let alnum = |b: &u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    match (bytes.first(), bytes.last()) {
        (Some(first), Some(last)) => {
            bytes.len() <= 63
                && alnum(first)
                && alnum(last)
                && bytes.iter().all(|b| alnum(b) || *b == b'-')
        }
        _ => false,
    }
}

/// Normalize a raw request host (`Host` header or `:authority`) for matching.
///
/// Drops the `:port`, lowercases, and strips one trailing `.`, so
/// `API.Acme.Dev.:8443` becomes `api.acme.dev`. A bracketed IPv6 literal is
/// kept intact (`[::1]:8080` becomes `[::1]`), and an unbracketed value with
/// several colons is treated as a bare IPv6 address and keeps them.
pub fn normalize_request_host(raw: &str) -> String {
    let raw = raw.trim();
    let host = if raw.starts_with('[') {
        match raw.find(']') {
            Some(end) => &raw[..=end],
            None => raw,
        }
    } else {
        match raw.split_once(':') {
            Some((host, port)) if !port.contains(':') => host,
            _ => raw,
        }
    };
    let host = host.to_ascii_lowercase();
    match host.strip_suffix('.') {
        Some(stripped) => stripped.to_string(),
        None => host,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn any_matches_everything() {
        assert!(HostMatch::Any.matches("anything.com"));
        assert!(HostMatch::Any.matches(""));
    }

    #[test]
    fn exact_matches_only_that_host() {
        let m = HostMatch::Exact("api.example.com".into());
        assert!(m.matches("api.example.com"));
        assert!(!m.matches("www.example.com"));
        assert!(!m.matches("api.example.com.evil.com"));
    }

    #[test]
    fn wildcard_matches_subdomains_not_apex_or_others() {
        let m = HostMatch::Wildcard(".acme.com".into());
        assert!(m.matches("a.acme.com"));
        // multi-label subdomain — required for <service>.<tenant>.<base> convention
        assert!(m.matches("a.b.acme.com"));
        assert!(!m.matches("acme.com")); // apex is not a subdomain
        assert!(!m.matches("x.evil.com")); // different domain
        assert!(!m.matches("notacme.com")); // dot boundary guard (ends with acme.com but not .acme.com)
    }

    #[test]
    fn parse_maps_gateway_api_forms() {
        assert_eq!(
            HostMatch::parse("api.example.com"),
            HostMatch::Exact("api.example.com".into())
        );
        assert_eq!(
            HostMatch::parse("*.example.com"),
            HostMatch::Wildcard(".example.com".into())
        );
        assert_eq!(HostMatch::parse(""), HostMatch::Any);
        assert_eq!(HostMatch::parse("*"), HostMatch::Any);
        // case-insensitive: patterns are lowercased
        assert_eq!(
            HostMatch::parse("API.Example.COM"),
            HostMatch::Exact("api.example.com".into())
        );
    }

    #[test]
    fn specificity_orders_exact_over_wildcard_over_any() {
        assert!(
            HostMatch::Exact("a.com".into()).specificity()
                > HostMatch::Wildcard(".a.com".into()).specificity()
        );
        assert!(HostMatch::Wildcard(".a.com".into()).specificity() > HostMatch::Any.specificity());
    }

    #[test]
    fn parse_pattern_accepts_exact_and_wildcard() {
        assert_eq!(
            HostMatch::parse_pattern("API.Acme.Dev.").unwrap(),
            HostMatch::Exact("api.acme.dev".into())
        );
        assert_eq!(
            HostMatch::parse_pattern("*.acme.dev").unwrap(),
            HostMatch::Wildcard(".acme.dev".into())
        );
        assert_eq!(
            HostMatch::parse_pattern("10.0.0.5").unwrap(),
            HostMatch::Exact("10.0.0.5".into())
        );
    }

    #[test]
    fn parse_pattern_rejects_bad_patterns() {
        for bad in [
            "",
            "*",
            "api.*.dev",
            "*api.dev",
            "api..dev",
            "-api.dev",
            "api-.dev",
            "api.dev:8080",
            "api/dev",
            "ápi.dev",
            &"a".repeat(64),
            &format!("{}.dev", "a.".repeat(126)),
        ] {
            assert!(
                HostMatch::parse_pattern(bad).is_err(),
                "{bad:?} must be rejected"
            );
        }
    }

    #[test]
    fn parse_pattern_errors_name_the_input_and_rule() {
        let err = HostMatch::parse_pattern("api.*.dev").unwrap_err();
        assert!(err.contains("api.*.dev"), "{err}");
        assert!(err.contains("first label"), "{err}");
        let err = HostMatch::parse_pattern("*").unwrap_err();
        assert!(err.contains("empty hosts list"), "{err}");
    }

    #[test]
    fn pattern_round_trips() {
        for p in ["api.acme.dev", "*.acme.dev"] {
            assert_eq!(
                HostMatch::parse_pattern(p).unwrap().pattern().as_deref(),
                Some(p)
            );
        }
        assert_eq!(HostMatch::Any.pattern(), None);
    }

    #[test]
    fn normalize_request_host_strips_port_case_and_trailing_dot() {
        assert_eq!(normalize_request_host("API.acme.dev.:8443"), "api.acme.dev");
        assert_eq!(normalize_request_host("[::1]:8080"), "[::1]");
        assert_eq!(normalize_request_host("acme.dev"), "acme.dev");
    }
}
