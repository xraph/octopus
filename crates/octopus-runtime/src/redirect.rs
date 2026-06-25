//! Pure rewrite of redirect-bearing response headers for proxy-mode routes.
//!
//! Maps an upstream's view of a redirect target back to the external view:
//! re-adds the gateway prefix that was stripped on the request, and swaps an
//! upstream/origin authority for the external gateway authority. Pure string
//! logic so it is exhaustively unit-testable; the handler supplies the context.

/// Context for rewriting a single redirect header value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedirectRewrite {
    /// External prefix that was stripped on the request (e.g. "/example"); empty if none.
    pub external_prefix: String,
    /// Authority (`host[:port]`) the upstream sees itself as — an in-cluster
    /// `ip:port` or an external origin host. Absolute redirects to this authority
    /// are rewritten to the gateway authority. `None` disables authority swapping.
    pub upstream_authority: Option<String>,
    /// External scheme to emit on rewritten absolute URLs (e.g. "https").
    pub gateway_scheme: String,
    /// External authority clients use (e.g. "gw.example.cloud").
    pub gateway_authority: String,
}

impl RedirectRewrite {
    /// Rewrite a `Location`/`Content-Location` value. Returns `None` to leave it
    /// unchanged (cross-origin, or nothing to do).
    pub fn rewrite_location(&self, value: &str) -> Option<String> {
        // Absolute URL: only touch it if it points at our upstream authority.
        if let Some(rest) = value
            .strip_prefix("http://")
            .or_else(|| value.strip_prefix("https://"))
        {
            let upstream = self.upstream_authority.as_deref()?;
            let (authority, path) = match rest.find('/') {
                Some(i) => (&rest[..i], &rest[i..]),
                None => (rest, "/"),
            };
            if authority != upstream {
                return None; // cross-origin — leave untouched
            }
            let new_path = self.prefixed(path);
            return Some(format!(
                "{}://{}{}",
                self.gateway_scheme, self.gateway_authority, new_path
            ));
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

    /// Rewrite the `url=` portion of a `Refresh` header value.
    ///
    /// `Refresh` looks like `5; url=/path` or `0;url=https://host/x`. The `url=`
    /// token is case-insensitive and may have optional whitespace around it.
    /// Returns `None` if there is no `url=` token or the target rewrite returns `None`.
    pub fn rewrite_refresh(&self, value: &str) -> Option<String> {
        // Find the position of "url=" (case-insensitive).
        let lower = value.to_ascii_lowercase();
        let url_pos = lower.find("url=")?;

        let before = &value[..url_pos];
        // The separator text up to and including "url=" (preserving original case).
        let sep = &value[url_pos..url_pos + 4]; // exactly "url=" in original casing
        let target = &value[url_pos + 4..];

        let new_target = self.rewrite_location(target)?;
        Some(format!("{before}{sep}{new_target}"))
    }

    /// Rewrite the `Path=` attribute inside a `Set-Cookie` header value.
    ///
    /// Finds the `Path=<p>` attribute (case-insensitive name), rewrites `<p>` via
    /// [`Self::rewrite_location`], and reassembles the cookie string unchanged
    /// except for the `Path` value. Returns `None` if there is no `Path=` attribute
    /// or the rewrite returns `None`.
    pub fn rewrite_cookie_path(&self, value: &str) -> Option<String> {
        // Split on ';' to find the Path= attribute while preserving the structure.
        let parts: Vec<&str> = value.split(';').collect();
        let mut path_idx = None;
        let mut path_value_start = 0usize; // byte offset of value within the part

        for (i, part) in parts.iter().enumerate() {
            let trimmed = part.trim_start();
            let lower = trimmed.to_ascii_lowercase();
            if lower.starts_with("path=") {
                path_idx = Some(i);
                // Compute where the value starts within `part` (not trimmed version).
                let leading = part.len() - trimmed.len();
                path_value_start = leading + "path=".len();
                break;
            }
        }

        let idx = path_idx?;
        let part = parts[idx];
        let path_value = &part[path_value_start..];

        let new_path = self.rewrite_location(path_value.trim())?;

        // Rebuild the part preserving leading whitespace and the attribute name casing.
        let prefix = &part[..path_value_start];
        let new_part = format!("{prefix}{new_path}");

        // Join all parts, substituting the rewritten part at `idx`.
        let mut result = String::with_capacity(value.len() + new_path.len());
        for (i, p) in parts.iter().enumerate() {
            if i > 0 {
                result.push(';');
            }
            if i == idx {
                result.push_str(&new_part);
            } else {
                result.push_str(p);
            }
        }
        Some(result)
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
            external_prefix: "/example".to_string(),
            upstream_authority: Some("10.0.0.1:7900".to_string()),
            gateway_scheme: "https".to_string(),
            gateway_authority: "gw.example.cloud".to_string(),
        }
    }

    #[test]
    fn root_relative_gets_external_prefix() {
        assert_eq!(
            rw().rewrite_location("/public-config").as_deref(),
            Some("/example/public-config")
        );
    }

    #[test]
    fn absolute_upstream_authority_swapped_and_prefixed() {
        assert_eq!(
            rw().rewrite_location("http://10.0.0.1:7900/public-config")
                .as_deref(),
            Some("https://gw.example.cloud/example/public-config")
        );
    }

    #[test]
    fn external_origin_authority_swapped() {
        let r = RedirectRewrite {
            external_prefix: "/ext".to_string(),
            upstream_authority: Some("api.example.com".to_string()),
            gateway_scheme: "https".to_string(),
            gateway_authority: "gw.example.cloud".to_string(),
        };
        assert_eq!(
            r.rewrite_location("https://api.example.com/whatever")
                .as_deref(),
            Some("https://gw.example.cloud/ext/whatever")
        );
    }

    #[test]
    fn unrelated_absolute_left_untouched() {
        assert_eq!(
            rw().rewrite_location("https://accounts.google.com/o/oauth2"),
            None
        );
    }

    #[test]
    fn empty_prefix_is_identity_for_root_relative() {
        let r = RedirectRewrite {
            external_prefix: String::new(),
            ..rw()
        };
        assert_eq!(r.rewrite_location("/foo"), None);
    }

    #[test]
    fn root_path_gets_prefix_with_single_slash() {
        assert_eq!(rw().rewrite_location("/").as_deref(), Some("/example/"));
    }

    #[test]
    fn internal_double_slash_preserved() {
        assert_eq!(rw().rewrite_location("//x").as_deref(), Some("/example//x"));
    }

    // ── rewrite_refresh ───────────────────────────────────────────────────────

    #[test]
    fn refresh_url_gets_prefix() {
        // Standard form: `5; url=/path`
        assert_eq!(
            rw().rewrite_refresh("5; url=/public-config").as_deref(),
            Some("5; url=/example/public-config")
        );
    }

    #[test]
    fn refresh_url_no_space_gets_prefix() {
        // Compact form: `0;url=/path`
        assert_eq!(
            rw().rewrite_refresh("0;url=/public-config").as_deref(),
            Some("0;url=/example/public-config")
        );
    }

    #[test]
    fn refresh_url_absolute_upstream_swapped() {
        assert_eq!(
            rw().rewrite_refresh("0;url=http://10.0.0.1:7900/public-config")
                .as_deref(),
            Some("0;url=https://gw.example.cloud/example/public-config")
        );
    }

    #[test]
    fn refresh_without_url_untouched() {
        // A bare numeric delay with no `url=` returns None.
        assert_eq!(rw().rewrite_refresh("5"), None);
    }

    #[test]
    fn refresh_url_case_insensitive() {
        // `URL=` in uppercase is found.
        assert_eq!(
            rw().rewrite_refresh("5; URL=/public-config").as_deref(),
            Some("5; URL=/example/public-config")
        );
    }

    // ── rewrite_cookie_path ──────────────────────────────────────────────────

    #[test]
    fn cookie_path_gets_prefix() {
        assert_eq!(
            rw().rewrite_cookie_path("session=abc; Path=/; HttpOnly")
                .as_deref(),
            Some("session=abc; Path=/example/; HttpOnly")
        );
    }

    #[test]
    fn cookie_path_non_root_gets_prefix() {
        assert_eq!(
            rw().rewrite_cookie_path("tok=xyz; Path=/app; Secure")
                .as_deref(),
            Some("tok=xyz; Path=/example/app; Secure")
        );
    }

    #[test]
    fn cookie_path_case_insensitive_attribute_name() {
        // `path=` in lowercase is found (case-insensitive match); original casing preserved.
        assert_eq!(
            rw().rewrite_cookie_path("x=1; path=/; HttpOnly").as_deref(),
            Some("x=1; path=/example/; HttpOnly")
        );
    }

    #[test]
    fn cookie_without_path_untouched() {
        // No `Path=` attribute → None.
        assert_eq!(rw().rewrite_cookie_path("session=abc; HttpOnly"), None);
    }

    #[test]
    fn cookie_path_first_in_value() {
        // Path= immediately after the cookie name=value.
        assert_eq!(
            rw().rewrite_cookie_path("x=1;Path=/").as_deref(),
            Some("x=1;Path=/example/")
        );
    }
}
