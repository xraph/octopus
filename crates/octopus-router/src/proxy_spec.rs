//! Reverse-proxy behavior for a route (proxy mode).

/// How the request path is forwarded to the upstream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PathMode {
    /// Apply the route's strip/add prefix rewrite (legacy behavior).
    #[default]
    Strip,
    /// Forward the full original request path untouched.
    Passthrough,
}

/// Upstream wire scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    /// Plain HTTP.
    Http,
    /// TLS-wrapped HTTPS.
    Https,
}

impl Scheme {
    /// Returns the scheme as a static string slice.
    pub fn as_str(self) -> &'static str {
        match self {
            Scheme::Http => "http",
            Scheme::Https => "https",
        }
    }
}

/// An external (non-cluster) origin to reverse-proxy to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpstreamOrigin {
    /// Wire scheme to use when connecting to the origin.
    pub scheme: Scheme,
    /// Origin hostname or IP address.
    pub host: String,
    /// TCP port on the origin.
    pub port: u16,
    /// TLS SNI; defaults to `host` when `None`.
    pub sni: Option<String>,
    /// Verify the origin server certificate (default true).
    pub tls_verify: bool,
}

impl UpstreamOrigin {
    /// Returns the base URL of the origin, e.g. `"https://api.example.com:443"`.
    pub fn base_url(&self) -> String {
        format!("{}://{}:{}", self.scheme.as_str(), self.host, self.port)
    }
}

/// Per-route reverse-proxy configuration. Absence (`Route.proxy == None`)
/// preserves the legacy in-cluster, strip-only, no-rewrite behavior.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxySpec {
    /// External origin. `None` keeps the route's in-cluster `upstream_name`.
    pub origin: Option<UpstreamOrigin>,
    /// How the request path is forwarded.
    pub path_mode: PathMode,
    /// Rewrite `Location`/`Content-Location`/`Refresh` on responses.
    pub rewrite_redirects: bool,
    /// Also rewrite `Set-Cookie` `Path=` attribute (opt-in).
    pub rewrite_cookie_path: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_mode_defaults_to_strip() {
        assert_eq!(PathMode::default(), PathMode::Strip);
    }

    #[test]
    fn origin_base_url_is_scheme_aware() {
        let o = UpstreamOrigin {
            scheme: Scheme::Https,
            host: "api.example.com".into(),
            port: 443,
            sni: None,
            tls_verify: true,
        };
        assert_eq!(o.base_url(), "https://api.example.com:443");
    }
}
