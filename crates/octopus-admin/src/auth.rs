//! Admin dashboard authentication.
//!
//! A self-contained username/password login backed by a signed (HS256) session
//! token, delivered as an `HttpOnly` cookie (and echoed in the login response
//! body for bearer-style clients).
//!
//! When no credentials are configured the entire layer becomes a pass-through,
//! preserving the historical "no auth" behavior of the admin dashboard. Real
//! enforcement happens on `/admin/api/*` (and the websocket) via
//! [`require_admin_session`]; the SPA additionally performs a client-side gate
//! for UX, but the API is the security boundary.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::handlers::AppState;
use crate::models::{LoginRequest, LoginResponse, MeResponse};

/// Name of the session cookie.
pub const SESSION_COOKIE: &str = "octopus_admin_session";

/// Default session lifetime (8 hours).
const DEFAULT_TTL_SECS: u64 = 8 * 60 * 60;

/// JWT claims for an admin session.
#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    /// Subject (username).
    sub: String,
    /// Role (currently always `admin`).
    role: String,
    /// Issued-at (unix seconds).
    iat: usize,
    /// Expiry (unix seconds).
    exp: usize,
}

/// Self-contained admin authentication backend.
#[derive(Clone)]
pub struct AdminAuth {
    username: String,
    password_sha256: [u8; 32],
    secret: Vec<u8>,
    ttl_secs: u64,
}

impl std::fmt::Debug for AdminAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdminAuth")
            .field("username", &self.username)
            .field("ttl_secs", &self.ttl_secs)
            .finish_non_exhaustive()
    }
}

impl AdminAuth {
    /// Construct an [`AdminAuth`] from explicit values.
    pub fn new(
        username: impl Into<String>,
        password: &str,
        secret: impl Into<Vec<u8>>,
        ttl_secs: u64,
    ) -> Self {
        Self {
            username: username.into(),
            password_sha256: sha256(password.as_bytes()),
            secret: secret.into(),
            ttl_secs: if ttl_secs == 0 {
                DEFAULT_TTL_SECS
            } else {
                ttl_secs
            },
        }
    }

    /// Build from environment variables, returning `None` (auth disabled) when
    /// `OCTOPUS_ADMIN_PASSWORD` is unset or empty.
    ///
    /// Recognized variables:
    /// - `OCTOPUS_ADMIN_PASSWORD` (required to enable auth)
    /// - `OCTOPUS_ADMIN_USERNAME` (default: `admin`)
    /// - `OCTOPUS_ADMIN_SESSION_SECRET` (default: derived from the password)
    /// - `OCTOPUS_ADMIN_SESSION_TTL_SECS` (default: 28800)
    pub fn from_env() -> Option<Self> {
        let password = std::env::var("OCTOPUS_ADMIN_PASSWORD").ok()?;
        if password.is_empty() {
            return None;
        }
        let username =
            std::env::var("OCTOPUS_ADMIN_USERNAME").unwrap_or_else(|_| "admin".to_string());
        let secret = std::env::var("OCTOPUS_ADMIN_SESSION_SECRET")
            .unwrap_or_else(|_| format!("octopus-admin-session::{password}"))
            .into_bytes();
        let ttl_secs = std::env::var("OCTOPUS_ADMIN_SESSION_TTL_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_TTL_SECS);
        Some(Self::new(username, &password, secret, ttl_secs))
    }

    /// Verify a username/password pair.
    fn verify_credentials(&self, username: &str, password: &str) -> bool {
        let user_ok = username.as_bytes() == self.username.as_bytes();
        // Fixed-size digest comparison avoids leaking password length.
        let pass_ok = sha256(password.as_bytes()) == self.password_sha256;
        user_ok && pass_ok
    }

    /// Mint a fresh session token. Returns `(token, expiry_unix_secs)`.
    fn mint_token(&self) -> (String, usize) {
        let now = unix_now();
        let exp = now.saturating_add(self.ttl_secs as usize);
        let claims = Claims {
            sub: self.username.clone(),
            role: "admin".to_string(),
            iat: now,
            exp,
        };
        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(&self.secret),
        )
        .unwrap_or_default();
        (token, exp)
    }

    /// Verify a session token, returning the subject (username) when valid.
    fn verify_token(&self, token: &str) -> Option<String> {
        let validation = Validation::default(); // HS256 + exp validation
        decode::<Claims>(token, &DecodingKey::from_secret(&self.secret), &validation)
            .ok()
            .map(|data| data.claims.sub)
    }

    /// Build the `Set-Cookie` header value for a freshly minted token.
    fn session_cookie(&self, token: &str) -> String {
        format!(
            "{SESSION_COOKIE}={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age={}",
            self.ttl_secs
        )
    }
}

/// Compute the SHA-256 digest of `data`.
fn sha256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}

/// Current unix time in seconds.
fn unix_now() -> usize {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as usize)
}

/// Extract a named cookie value from request headers.
fn extract_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    let raw = headers.get(header::COOKIE)?.to_str().ok()?;
    let prefix = format!("{name}=");
    raw.split(';')
        .map(str::trim)
        .find_map(|part| part.strip_prefix(&prefix).map(ToString::to_string))
}

/// Expired/cleared cookie used on logout.
fn clear_cookie() -> String {
    format!("{SESSION_COOKIE}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0")
}

// ============================================================================
// Handlers
// ============================================================================

/// `POST /admin/api/auth/login`
pub async fn api_auth_login_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<LoginRequest>,
) -> Response {
    let Some(auth) = state.admin_auth.as_ref() else {
        // Auth disabled: accept and report success without a cookie.
        return Json(LoginResponse {
            success: true,
            token: None,
            expires_at: None,
            message: Some("Authentication is disabled".to_string()),
        })
        .into_response();
    };

    if !auth.verify_credentials(&req.username, &req.password) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(LoginResponse {
                success: false,
                token: None,
                expires_at: None,
                message: Some("Invalid username or password".to_string()),
            }),
        )
            .into_response();
    }

    let (token, exp) = auth.mint_token();
    let cookie = auth.session_cookie(&token);
    tracing::info!("Admin login succeeded for user '{}'", req.username);
    (
        [(header::SET_COOKIE, cookie)],
        Json(LoginResponse {
            success: true,
            token: Some(token),
            expires_at: Some(exp.to_string()),
            message: None,
        }),
    )
        .into_response()
}

/// `POST /admin/api/auth/logout`
pub async fn api_auth_logout_handler() -> Response {
    (
        [(header::SET_COOKIE, clear_cookie())],
        Json(serde_json::json!({ "success": true })),
    )
        .into_response()
}

/// `GET /admin/api/auth/me`
pub async fn api_auth_me_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let Some(auth) = state.admin_auth.as_ref() else {
        return Json(MeResponse {
            authenticated: true,
            auth_required: false,
            username: Some("admin".to_string()),
            role: Some("admin".to_string()),
        })
        .into_response();
    };

    if let Some(sub) = extract_cookie(&headers, SESSION_COOKIE)
        .as_deref()
        .and_then(|t| auth.verify_token(t))
    {
        return Json(MeResponse {
            authenticated: true,
            auth_required: true,
            username: Some(sub),
            role: Some("admin".to_string()),
        })
        .into_response();
    }

    (
        StatusCode::UNAUTHORIZED,
        Json(MeResponse {
            authenticated: false,
            auth_required: true,
            username: None,
            role: None,
        }),
    )
        .into_response()
}

// ============================================================================
// Middleware
// ============================================================================

/// Axum middleware enforcing a valid session on protected admin endpoints.
///
/// Pass-through when auth is disabled (`state.admin_auth` is `None`). Otherwise
/// gates `/admin/api/*` (except `/admin/api/auth/*`) and `/admin/ws`, returning
/// `401` with a JSON body when no valid session cookie is present. UI/static
/// routes are intentionally left open so the login page can load; their data is
/// gated at the API.
pub async fn require_admin_session(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Response {
    let Some(auth) = state.admin_auth.clone() else {
        return next.run(req).await;
    };

    let path = req.uri().path();
    let needs_auth = (path.starts_with("/admin/api/") && !path.starts_with("/admin/api/auth/"))
        || path == "/admin/ws";

    if !needs_auth {
        return next.run(req).await;
    }

    let authorized = extract_cookie(req.headers(), SESSION_COOKIE)
        .as_deref()
        .and_then(|t| auth.verify_token(t))
        .is_some();

    if authorized {
        next.run(req).await
    } else {
        (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "Authentication required" })),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_session_token() {
        let auth = AdminAuth::new("admin", "s3cret", b"signing-secret".to_vec(), 3600);
        assert!(auth.verify_credentials("admin", "s3cret"));
        assert!(!auth.verify_credentials("admin", "wrong"));
        assert!(!auth.verify_credentials("root", "s3cret"));

        let (token, _exp) = auth.mint_token();
        assert_eq!(auth.verify_token(&token).as_deref(), Some("admin"));
        assert!(auth.verify_token("not-a-token").is_none());
    }

    #[test]
    fn rejects_token_from_a_different_secret() {
        let a = AdminAuth::new("admin", "pw", b"secret-a".to_vec(), 3600);
        let b = AdminAuth::new("admin", "pw", b"secret-b".to_vec(), 3600);
        let (token, _) = a.mint_token();
        assert!(b.verify_token(&token).is_none());
    }

    #[test]
    fn extracts_named_cookie() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            "foo=bar; octopus_admin_session=abc123; baz=qux"
                .parse()
                .unwrap(),
        );
        assert_eq!(
            extract_cookie(&headers, SESSION_COOKIE).as_deref(),
            Some("abc123")
        );
        assert_eq!(extract_cookie(&headers, "missing"), None);
    }
}
