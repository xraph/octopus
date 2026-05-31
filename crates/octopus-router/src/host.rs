//! Host (authority) matching for routes.
//!
//! A route may be scoped to a request host (the HTTP `Host` header / HTTP/2
//! `:authority`). This mirrors the Kubernetes Gateway API `hostnames` model:
//! a route matches either any host, an exact host, or a wildcard suffix
//! (`*.example.com`). When several routes match the same method+path, the one
//! with the most specific host wins (exact > wildcard > any).
//!
//! Hosts are compared in lowercase; callers normalize the request host and
//! parsed patterns are lowercased by [`HostMatch::parse`].

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
}
