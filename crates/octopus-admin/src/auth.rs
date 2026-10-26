//! Admin dashboard authentication.
//!
//! Two credentials are supported, alone or together:
//!
//! - a username/password login backed by a signed (HS256) session token,
//!   delivered as an `HttpOnly` cookie (and echoed in the login response body
//!   for bearer-style clients);
//! - a static API token for machine clients, sent as `Authorization: Bearer
//!   <token>` or as the [`ADMIN_TOKEN_HEADER`] header. The custom header exists
//!   because the Kubernetes API service proxy strips `Authorization` after it
//!   authenticates the caller.
//!
//! When no credentials are configured the entire layer becomes a pass-through,
//! preserving the historical "no auth" behavior of the admin dashboard. Real
//! enforcement happens on `/admin/api/*`, the websocket and the server-rendered
//! pages via [`require_admin_session`]; the SPA additionally performs a
//! client-side gate for UX, but the server is the security boundary.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::Next,
    response::{Html, IntoResponse, Response},
    Json,
};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::handlers::AppState;
use crate::models::{LoginRequest, LoginResponse, MeResponse};

/// Name of the session cookie.
pub const SESSION_COOKIE: &str = "octopus_admin_session";

/// Header carrying the admin API token, for clients whose `Authorization`
/// header does not survive the trip (the Kubernetes API service proxy).
pub const ADMIN_TOKEN_HEADER: &str = "x-octopus-admin-token";

/// Who an authenticated admin request acts as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdminPrincipal {
    /// A user holding a session token (cookie or bearer).
    User(String),
    /// A machine client presenting the static API token.
    ApiToken,
}

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
///
/// Secrets are held only as SHA-256 digests (password, API token) or as the
/// signing key, and the [`Debug`](std::fmt::Debug) output never includes them.
#[derive(Clone)]
pub struct AdminAuth {
    username: String,
    password_sha256: Option<[u8; 32]>,
    api_token_sha256: Option<[u8; 32]>,
    secret: Vec<u8>,
    ttl_secs: u64,
}

impl std::fmt::Debug for AdminAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdminAuth")
            .field("username", &self.username)
            .field("password_login", &self.password_sha256.is_some())
            .field("api_token", &self.api_token_sha256.is_some())
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
            password_sha256: Some(sha256(password.as_bytes())),
            api_token_sha256: None,
            secret: secret.into(),
            ttl_secs: effective_ttl(ttl_secs),
        }
    }

    /// Also accept `token` as a static API token (`Authorization: Bearer` or
    /// [`ADMIN_TOKEN_HEADER`]). An empty token is ignored.
    pub fn with_api_token(mut self, token: &str) -> Self {
        self.api_token_sha256 = (!token.is_empty()).then(|| sha256(token.as_bytes()));
        self
    }

    /// Authentication with an API token only: password login is disabled.
    pub fn api_token_only(token: &str, secret: impl Into<Vec<u8>>) -> Self {
        Self {
            username: "admin".to_string(),
            password_sha256: None,
            api_token_sha256: None,
            secret: secret.into(),
            ttl_secs: DEFAULT_TTL_SECS,
        }
        .with_api_token(token)
    }

    /// Whether `POST /admin/api/auth/login` can succeed (a password is set).
    pub fn password_login_enabled(&self) -> bool {
        self.password_sha256.is_some()
    }

    /// The mechanisms that are on, for the startup log line
    /// (e.g. `["password login", "API token"]`). Never includes secrets.
    pub fn mechanisms(&self) -> Vec<&'static str> {
        let mut mechanisms = Vec::new();
        if self.password_sha256.is_some() {
            mechanisms.push("password login");
        }
        if self.api_token_sha256.is_some() {
            mechanisms.push("API token");
        }
        mechanisms
    }

    /// Build from environment variables, returning `None` (auth disabled) when
    /// neither `OCTOPUS_ADMIN_PASSWORD` nor `OCTOPUS_ADMIN_API_TOKEN` is set to
    /// a non-empty value.
    ///
    /// Recognized variables:
    /// - `OCTOPUS_ADMIN_PASSWORD` (enables password login)
    /// - `OCTOPUS_ADMIN_API_TOKEN` (enables the machine API token)
    /// - `OCTOPUS_ADMIN_USERNAME` (default: `admin`)
    /// - `OCTOPUS_ADMIN_SESSION_SECRET` (default: derived from the password,
    ///   else from the API token)
    /// - `OCTOPUS_ADMIN_SESSION_TTL_SECS` (default: 28800)
    pub fn from_env() -> Option<Self> {
        let non_empty = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
        let password = non_empty("OCTOPUS_ADMIN_PASSWORD");
        let api_token = non_empty("OCTOPUS_ADMIN_API_TOKEN");
        if password.is_none() && api_token.is_none() {
            return None;
        }
        let username =
            std::env::var("OCTOPUS_ADMIN_USERNAME").unwrap_or_else(|_| "admin".to_string());
        let secret = non_empty("OCTOPUS_ADMIN_SESSION_SECRET")
            .or_else(|| {
                password
                    .as_ref()
                    .map(|p| format!("octopus-admin-session::{p}"))
            })
            .or_else(|| {
                api_token
                    .as_ref()
                    .map(|t| format!("octopus-admin-session::token::{t}"))
            })
            .unwrap_or_default()
            .into_bytes();
        let ttl_secs = std::env::var("OCTOPUS_ADMIN_SESSION_TTL_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_TTL_SECS);
        Some(Self {
            username,
            password_sha256: password.as_deref().map(|p| sha256(p.as_bytes())),
            api_token_sha256: api_token.as_deref().map(|t| sha256(t.as_bytes())),
            secret,
            ttl_secs: effective_ttl(ttl_secs),
        })
    }

    /// Identify the caller from request headers, trying in order: the session
    /// cookie, `Authorization: Bearer <session token>`, `Authorization: Bearer
    /// <api token>`, and the [`ADMIN_TOKEN_HEADER`] header carrying the API
    /// token.
    pub fn authenticate(&self, headers: &HeaderMap) -> Option<AdminPrincipal> {
        if let Some(sub) = extract_cookie(headers, SESSION_COOKIE)
            .as_deref()
            .and_then(|t| self.verify_token(t))
        {
            return Some(AdminPrincipal::User(sub));
        }

        let bearer = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(bearer_token);
        if let Some(bearer) = bearer {
            if let Some(sub) = self.verify_token(bearer) {
                return Some(AdminPrincipal::User(sub));
            }
            if self.verify_api_token(bearer) {
                return Some(AdminPrincipal::ApiToken);
            }
        }

        let custom = headers
            .get(ADMIN_TOKEN_HEADER)
            .and_then(|v| v.to_str().ok())
            .map(str::trim);
        if custom.is_some_and(|t| self.verify_api_token(t)) {
            return Some(AdminPrincipal::ApiToken);
        }
        None
    }

    /// Verify a presented API token against the configured digest.
    fn verify_api_token(&self, presented: &str) -> bool {
        match self.api_token_sha256 {
            // Fixed-size digest comparison avoids leaking token length.
            Some(expected) if !presented.is_empty() => sha256(presented.as_bytes()) == expected,
            _ => false,
        }
    }

    /// Verify a username/password pair. Always `false` when password login is
    /// disabled.
    fn verify_credentials(&self, username: &str, password: &str) -> bool {
        let Some(expected) = self.password_sha256 else {
            return false;
        };
        let user_ok = username.as_bytes() == self.username.as_bytes();
        // Fixed-size digest comparison avoids leaking password length.
        let pass_ok = sha256(password.as_bytes()) == expected;
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

/// A zero TTL means "use the default".
fn effective_ttl(ttl_secs: u64) -> u64 {
    if ttl_secs == 0 {
        DEFAULT_TTL_SECS
    } else {
        ttl_secs
    }
}

/// The credential of an `Authorization: Bearer <credential>` value.
fn bearer_token(value: &str) -> Option<&str> {
    let (scheme, credential) = value.trim().split_once(' ')?;
    let credential = credential.trim();
    (scheme.eq_ignore_ascii_case("bearer") && !credential.is_empty()).then_some(credential)
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

    if !auth.password_login_enabled() {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "success": false,
                "message": "Password login is disabled",
            })),
        )
            .into_response();
    }

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

    if let Some(principal) = auth.authenticate(&headers) {
        let username = match principal {
            AdminPrincipal::User(name) => name,
            AdminPrincipal::ApiToken => "api-token".to_string(),
        };
        return Json(MeResponse {
            authenticated: true,
            auth_required: true,
            username: Some(username),
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

/// Server-rendered pages gated by [`require_admin_session`].
const GATED_PAGES: &[&str] = &[
    "/admin",
    "/admin/routes",
    "/admin/health",
    "/admin/plugins",
    "/admin/analytics",
    "/admin/logs",
    "/admin/config",
];

/// What kind of gated surface a path is, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Gate {
    /// JSON API (and the websocket): 401 with a JSON body.
    Api,
    /// Server-rendered page: 401 with an HTML body.
    Page,
}

fn gate_for(path: &str) -> Option<Gate> {
    if (path.starts_with("/admin/api/") && !path.starts_with("/admin/api/auth/"))
        || path == "/admin/ws"
    {
        Some(Gate::Api)
    } else if GATED_PAGES.contains(&path)
        || path == "/admin/octopus-ui"
        || path.starts_with("/admin/octopus-ui/")
    {
        Some(Gate::Page)
    } else {
        None
    }
}

/// Body of the 401 a server-rendered page answers without credentials when
/// only the API token is configured.
const PAGE_UNAUTHORIZED_HTML: &str = "<!DOCTYPE html>\n<html lang=\"en\">\n<head><meta charset=\"utf-8\"><title>Authentication required</title></head>\n<body>\n<h1>Authentication required</h1>\n<p>Sign in to the Octopus admin dashboard, or send the admin API token in the <code>X-Octopus-Admin-Token</code> header.</p>\n</body>\n</html>\n";

/// Body of the 401 a server-rendered page answers without credentials when
/// password login is on: a small sign-in form that posts JSON to
/// `/admin/api/auth/login` and reloads the page once the session cookie is
/// set. Every URL it uses starts with `/admin`, like the pages it stands in
/// for, so a proxy that rewrites the `/admin` prefix keeps it working.
const PAGE_LOGIN_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Sign in to Octopus</title>
<style>
body{font-family:system-ui,sans-serif;background:#f5f5f7;color:#1d1d1f;display:flex;min-height:100vh;align-items:center;justify-content:center;margin:0}
form{background:#fff;padding:2rem;border-radius:12px;box-shadow:0 1px 4px rgba(0,0,0,.12);width:min(320px,90vw)}
h1{font-size:1.25rem;margin:0 0 1rem}
label{display:block;font-size:.875rem;margin:.75rem 0 .25rem}
input{width:100%;box-sizing:border-box;padding:.5rem;border:1px solid #c7c7cc;border-radius:6px;font-size:1rem}
button{margin-top:1.25rem;width:100%;padding:.6rem;border:0;border-radius:6px;background:#0a66c2;color:#fff;font-size:1rem;cursor:pointer}
#login-error{color:#b00020;font-size:.875rem;min-height:1.2em;margin:.75rem 0 0}
</style>
</head>
<body>
<form id="login-form" novalidate>
<h1>Sign in to Octopus</h1>
<label for="login-username">Username</label>
<input id="login-username" name="username" autocomplete="username" required>
<label for="login-password">Password</label>
<input id="login-password" name="password" type="password" autocomplete="current-password" required>
<button type="submit">Sign in</button>
<p id="login-error" role="alert"></p>
</form>
<script>
document.getElementById('login-form').addEventListener('submit', async function (event) {
  event.preventDefault();
  var error = document.getElementById('login-error');
  error.textContent = '';
  try {
    var response = await fetch('/admin/api/auth/login', {
      method: 'POST',
      credentials: 'same-origin',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        username: this.elements.username.value,
        password: this.elements.password.value
      })
    });
    var body = await response.json().catch(function () { return {}; });
    if (response.ok && body.success) {
      window.location.reload();
      return;
    }
    error.textContent = body.message || 'Sign-in failed';
  } catch (e) {
    error.textContent = 'Could not reach the gateway';
  }
});
</script>
</body>
</html>
"#;

/// The 401 a server-rendered page answers: the sign-in form when password
/// login is on, the plain notice otherwise.
fn page_unauthorized(auth: &AdminAuth) -> Response {
    let body = if auth.password_login_enabled() {
        PAGE_LOGIN_HTML
    } else {
        PAGE_UNAUTHORIZED_HTML
    };
    (StatusCode::UNAUTHORIZED, Html(body)).into_response()
}

/// Axum middleware enforcing admin credentials on protected admin endpoints.
///
/// Pass-through when auth is disabled (`state.admin_auth` is `None`). Otherwise
/// gates `/admin/api/*` (except `/admin/api/auth/*`), `/admin/ws` and the
/// server-rendered pages (`/admin`, `/admin/routes`, ..., `/admin/octopus-ui*`).
/// The API answers `401` with a JSON body and pages answer `401` with an HTML
/// body (a sign-in form when password login is on) when
/// [`AdminAuth::authenticate`] finds no credential. `/admin/ui/*`
/// (the SPA, which hosts the login page) and `/admin/static/*` stay open;
/// their data is gated at the API.
pub async fn require_admin_session(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Response {
    let Some(auth) = state.admin_auth.clone() else {
        return next.run(req).await;
    };

    let Some(gate) = gate_for(req.uri().path()) else {
        return next.run(req).await;
    };

    if auth.authenticate(req.headers()).is_some() {
        return next.run(req).await;
    }

    match gate {
        Gate::Api => (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "Authentication required" })),
        )
            .into_response(),
        Gate::Page => page_unauthorized(&auth),
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

    fn headers_with(name: &'static str, value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(name, value.parse().unwrap());
        headers
    }

    #[test]
    fn api_token_authenticates_bearer_and_custom_header() {
        let auth = AdminAuth::new("admin", "pw", b"s".to_vec(), 3600).with_api_token("tok");
        assert_eq!(
            auth.authenticate(&headers_with("authorization", "Bearer tok")),
            Some(AdminPrincipal::ApiToken)
        );
        assert_eq!(
            auth.authenticate(&headers_with(ADMIN_TOKEN_HEADER, "tok")),
            Some(AdminPrincipal::ApiToken)
        );
    }

    #[test]
    fn wrong_api_token_is_rejected() {
        let auth = AdminAuth::new("admin", "pw", b"s".to_vec(), 3600).with_api_token("tok");
        assert_eq!(
            auth.authenticate(&headers_with("authorization", "Bearer nope")),
            None
        );
        assert_eq!(
            auth.authenticate(&headers_with(ADMIN_TOKEN_HEADER, "nope")),
            None
        );
        assert_eq!(
            auth.authenticate(&headers_with(ADMIN_TOKEN_HEADER, "")),
            None
        );
        assert_eq!(auth.authenticate(&HeaderMap::new()), None);
    }

    #[test]
    fn session_token_accepted_as_bearer() {
        let auth = AdminAuth::new("admin", "pw", b"s".to_vec(), 3600).with_api_token("tok");
        let (jwt, _) = auth.mint_token();
        assert_eq!(
            auth.authenticate(&headers_with("authorization", &format!("Bearer {jwt}"))),
            Some(AdminPrincipal::User("admin".to_string()))
        );
        assert_eq!(
            auth.authenticate(&headers_with("cookie", &format!("{SESSION_COOKIE}={jwt}"))),
            Some(AdminPrincipal::User("admin".to_string()))
        );
    }

    #[test]
    fn gates_api_websocket_and_pages_only() {
        assert_eq!(gate_for("/admin/api/routes"), Some(Gate::Api));
        assert_eq!(gate_for("/admin/ws"), Some(Gate::Api));
        assert_eq!(gate_for("/admin/api/auth/login"), None);
        assert_eq!(gate_for("/admin/api/auth/me"), None);
        assert_eq!(gate_for("/admin"), Some(Gate::Page));
        assert_eq!(gate_for("/admin/config"), Some(Gate::Page));
        assert_eq!(gate_for("/admin/octopus-ui"), Some(Gate::Page));
        assert_eq!(gate_for("/admin/octopus-ui/health"), Some(Gate::Page));
        assert_eq!(gate_for("/admin/ui/login/"), None);
        assert_eq!(gate_for("/admin/static/output.css"), None);
    }

    #[test]
    fn token_only_auth_disables_password_login() {
        let auth = AdminAuth::api_token_only("tok", b"s".to_vec());
        assert!(!auth.password_login_enabled());
        assert!(!auth.verify_credentials("admin", ""));
        assert!(!auth.verify_credentials("admin", "tok"));
        assert_eq!(
            auth.authenticate(&headers_with(ADMIN_TOKEN_HEADER, "tok")),
            Some(AdminPrincipal::ApiToken)
        );

        let with_password = AdminAuth::new("admin", "pw", b"s".to_vec(), 3600);
        assert!(with_password.password_login_enabled());
        // No API token configured: a token header never authenticates.
        assert_eq!(
            with_password.authenticate(&headers_with(ADMIN_TOKEN_HEADER, "pw")),
            None
        );
    }
}
