//! Pure rewrite of redirect-bearing response headers for proxy-mode routes.
//!
//! Maps an upstream's view of a redirect target back to the external view:
//! re-adds the gateway prefix that was stripped on the request, and swaps an
//! upstream/origin authority for the external gateway authority. Pure string
//! logic so it is exhaustively unit-testable; the handler supplies the context.

/// Context for rewriting a single redirect header value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedirectRewrite {
    /// External prefix that was stripped on the request (e.g. "/twinos"); empty if none.
    pub external_prefix: String,
    /// Authority (`host[:port]`) the upstream sees itself as — an in-cluster
    /// `ip:port` or an external origin host. Absolute redirects to this authority
    /// are rewritten to the gateway authority. `None` disables authority swapping.
    pub upstream_authority: Option<String>,
    /// External scheme to emit on rewritten absolute URLs (e.g. "https").
    pub gateway_scheme: String,
    /// External authority clients use (e.g. "twin.api.muono.cloud").
    pub gateway_authority: String,
}

impl RedirectRewrite {
    /// Rewrite a `Location`/`Content-Location` value. Returns `None` to leave it
    /// unchanged (cross-origin, or nothing to do).
    pub fn rewrite_location(&self, value: &str) -> Option<String> {
        // Absolute URL: only touch it if it points at our upstream authority.
        if let Some(rest) = value.strip_prefix("http://").or_else(|| value.strip_prefix("https://")) {
            let upstream = self.upstream_authority.as_deref()?;
            let (authority, path) = match rest.find('/') {
                Some(i) => (&rest[..i], &rest[i..]),
                None => (rest, "/"),
            };
            if authority != upstream {
                return None; // cross-origin — leave untouched
            }
            let new_path = self.prefixed(path);
            return Some(format!("{}://{}{}", self.gateway_scheme, self.gateway_authority, new_path));
        }

        // Root-relative path: re-add the external prefix.
        if value.starts_with('/') {
            if self.external_prefix.is_empty() {
                return None;
            }
            return Some(self.prefixed(value));
        }

        // Relative (no leading slash) or non-URL (e.g. "0;url=...") — caller handles.
        None
    }

    /// Join the external prefix onto an upstream path without doubling the seam slash.
    fn prefixed(&self, path: &str) -> String {
        if self.external_prefix.is_empty() {
            return path.to_string();
        }
        let base = self.external_prefix.trim_end_matches('/');
        match path.strip_prefix('/') {
            Some(rest) => format!("{base}/{rest}"),
            None => format!("{base}/{path}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rw() -> RedirectRewrite {
        RedirectRewrite {
            external_prefix: "/twinos".to_string(),
            upstream_authority: Some("10.0.0.1:7900".to_string()),
            gateway_scheme: "https".to_string(),
            gateway_authority: "twin.api.muono.cloud".to_string(),
        }
    }

    #[test]
    fn root_relative_gets_external_prefix() {
        assert_eq!(rw().rewrite_location("/public-config").as_deref(), Some("/twinos/public-config"));
    }

    #[test]
    fn absolute_upstream_authority_swapped_and_prefixed() {
        assert_eq!(
            rw().rewrite_location("http://10.0.0.1:7900/public-config").as_deref(),
            Some("https://twin.api.muono.cloud/twinos/public-config")
        );
    }

    #[test]
    fn external_origin_authority_swapped() {
        let r = RedirectRewrite {
            external_prefix: "/ext".to_string(),
            upstream_authority: Some("api.example.com".to_string()),
            gateway_scheme: "https".to_string(),
            gateway_authority: "twin.api.muono.cloud".to_string(),
        };
        assert_eq!(
            r.rewrite_location("https://api.example.com/whatever").as_deref(),
            Some("https://twin.api.muono.cloud/ext/whatever")
        );
    }

    #[test]
    fn unrelated_absolute_left_untouched() {
        assert_eq!(rw().rewrite_location("https://accounts.google.com/o/oauth2"), None);
    }

    #[test]
    fn empty_prefix_is_identity_for_root_relative() {
        let r = RedirectRewrite { external_prefix: String::new(), ..rw() };
        assert_eq!(r.rewrite_location("/foo"), None);
    }

    #[test]
    fn root_path_gets_prefix_with_single_slash() {
        assert_eq!(rw().rewrite_location("/").as_deref(), Some("/twinos/"));
    }

    #[test]
    fn internal_double_slash_preserved() {
        assert_eq!(rw().rewrite_location("//x").as_deref(), Some("/twinos//x"));
    }
}
