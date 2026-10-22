//! Machine-token admin authentication, driven through the real dashboard
//! router with in-process `oneshot` calls.

use std::sync::Arc;

use axum::body::Body;
use http::{header, Method, Request, StatusCode};
use octopus_admin::auth::{AdminAuth, ADMIN_TOKEN_HEADER};
use octopus_admin::{AppState, DashboardRouter};
use octopus_router::Router;
use regex::Regex;
use serde_json::{json, Value};
use tower::ServiceExt;

const TOKEN: &str = "tok";

/// A dashboard router whose only credential is the API token `tok`.
fn app() -> axum::Router {
    let state = AppState::new()
        .with_router(Arc::new(Router::new()))
        .with_admin_auth(Arc::new(AdminAuth::api_token_only(
            TOKEN,
            b"session-secret".to_vec(),
        )));
    DashboardRouter::build(Arc::new(state))
}

struct Reply {
    status: StatusCode,
    content_type: String,
    location: Option<String>,
    body: String,
}

impl Reply {
    fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap_or(Value::Null)
    }
}

async fn send(method: Method, uri: &str, headers: &[(&str, &str)], body: Option<Value>) -> Reply {
    send_to(app(), method, uri, headers, body).await
}

/// A dashboard router with password login (`admin` / `pw`) and the API token.
fn password_app() -> axum::Router {
    let auth =
        AdminAuth::new("admin", "pw", b"session-secret".to_vec(), 3600).with_api_token(TOKEN);
    let state = AppState::new()
        .with_router(Arc::new(Router::new()))
        .with_admin_auth(Arc::new(auth));
    DashboardRouter::build(Arc::new(state))
}

async fn send_to(
    app: axum::Router,
    method: Method,
    uri: &str,
    headers: &[(&str, &str)],
    body: Option<Value>,
) -> Reply {
    let mut request = Request::builder().method(method).uri(uri);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let request = match body {
        Some(body) => request
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
        None => request.body(Body::empty()).unwrap(),
    };
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let header_str = |name: header::HeaderName| {
        response
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(ToString::to_string)
    };
    let content_type = header_str(header::CONTENT_TYPE).unwrap_or_default();
    let location = header_str(header::LOCATION);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    Reply {
        status,
        content_type,
        location,
        body: String::from_utf8_lossy(&bytes).into_owned(),
    }
}

async fn get(uri: &str, headers: &[(&str, &str)]) -> Reply {
    send(Method::GET, uri, headers, None).await
}

#[tokio::test]
async fn api_routes_require_auth_and_accept_api_token() {
    let anonymous = get("/admin/api/routes", &[]).await;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        anonymous.json(),
        json!({ "error": "Authentication required" })
    );

    let wrong = get("/admin/api/routes", &[("authorization", "Bearer nope")]).await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);

    let bearer = get("/admin/api/routes", &[("authorization", "Bearer tok")]).await;
    assert_eq!(bearer.status, StatusCode::OK, "{}", bearer.body);

    let header = get("/admin/api/routes", &[(ADMIN_TOKEN_HEADER, TOKEN)]).await;
    assert_eq!(header.status, StatusCode::OK, "{}", header.body);

    // The header name is matched case-insensitively, as HTTP requires.
    let mixed_case = get("/admin/api/routes", &[("X-Octopus-Admin-Token", TOKEN)]).await;
    assert_eq!(mixed_case.status, StatusCode::OK);
}

#[tokio::test]
async fn ssr_pages_require_auth_when_enabled() {
    let anonymous = get("/admin", &[]).await;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);
    assert!(
        anonymous.content_type.starts_with("text/html"),
        "page 401 should be HTML, got {:?}",
        anonymous.content_type
    );

    let octopus_ui = get("/admin/octopus-ui/routes", &[]).await;
    assert_eq!(octopus_ui.status, StatusCode::UNAUTHORIZED);
    assert!(octopus_ui.content_type.starts_with("text/html"));

    let authed = get("/admin", &[(ADMIN_TOKEN_HEADER, TOKEN)]).await;
    assert_eq!(authed.status, StatusCode::OK);
}

#[tokio::test]
async fn admin_ui_paths_stay_open() {
    let login = get("/admin/ui/login/", &[]).await;
    assert_ne!(login.status, StatusCode::UNAUTHORIZED);

    let asset = get("/admin/static/output.css", &[]).await;
    assert_ne!(asset.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn me_reports_api_token_principal() {
    let anonymous = get("/admin/api/auth/me", &[]).await;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);
    assert_eq!(anonymous.json()["auth_required"], json!(true));

    let me = get("/admin/api/auth/me", &[(ADMIN_TOKEN_HEADER, TOKEN)]).await;
    assert_eq!(me.status, StatusCode::OK);
    let body = me.json();
    assert_eq!(body["authenticated"], json!(true));
    assert_eq!(body["username"], json!("api-token"));
    assert_eq!(body["role"], json!("admin"));
}

#[tokio::test]
async fn login_is_refused_when_only_a_token_is_set() {
    let reply = send(
        Method::POST,
        "/admin/api/auth/login",
        &[],
        Some(json!({ "username": "admin", "password": "tok" })),
    )
    .await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        reply.json(),
        json!({ "success": false, "message": "Password login is disabled" })
    );
}

/// Pages a proxy may serve under a path prefix by rewriting `/admin`.
const SSR_PAGES: &[&str] = &[
    "/admin",
    "/admin/routes",
    "/admin/health",
    "/admin/plugins",
    "/admin/analytics",
    "/admin/logs",
    "/admin/config",
    "/admin/octopus-ui",
    "/admin/octopus-ui/routes",
    "/admin/octopus-ui/health",
    "/admin/octopus-ui/plugins",
];

/// The URL a captured value points at. Alpine bindings (`:href="'/x/' + id"`)
/// and template literals hold a JS expression, so the leading string literal
/// is the part that names the path.
fn url_part(value: &str) -> &str {
    let value = value.trim();
    for quote in ['\'', '`', '"'] {
        if let Some(rest) = value.strip_prefix(quote) {
            return rest.split(quote).next().unwrap_or(rest);
        }
    }
    value
}

/// Whether an origin-relative or absolute URL is safe to serve behind a
/// prefix-rewriting proxy.
fn is_allowed(url: &str) -> bool {
    url == "/admin"
        || url.starts_with("/admin/")
        || url.starts_with("/admin?")
        || url.starts_with("https://")
        || url.starts_with('#')
        || !(url.starts_with('/') || url.starts_with("http:"))
}

/// Every URL-bearing attribute value and `fetch(...)`/`new WebSocket(...)`
/// literal in `body`.
fn referenced_urls(body: &str) -> Vec<String> {
    let attr_double = Regex::new(
        r#"(?i)[\s:@](?:href|src|action|hx-get|hx-post|hx-put|hx-delete|hx-patch)\s*=\s*"([^"]*)""#,
    )
    .unwrap();
    let attr_single = Regex::new(
        r"(?i)[\s:@](?:href|src|action|hx-get|hx-post|hx-put|hx-delete|hx-patch)\s*=\s*'([^']*)'",
    )
    .unwrap();
    let call_literal = Regex::new(r#"(?:fetch|new\s+WebSocket)\(\s*(['"`])([^'"`]*)"#).unwrap();
    attr_double
        .captures_iter(body)
        .chain(attr_single.captures_iter(body))
        .map(|c| c[1].to_string())
        .chain(call_literal.captures_iter(body).map(|c| c[2].to_string()))
        .collect()
}

#[tokio::test]
async fn ssr_pages_only_reference_admin_paths() {
    let mut violations = Vec::new();
    for page in SSR_PAGES {
        let reply = get(page, &[(ADMIN_TOKEN_HEADER, TOKEN)]).await;
        if reply.status.is_redirection() {
            let location = reply.location.clone().unwrap_or_default();
            if !location.starts_with("/admin") {
                violations.push(format!("{page}: redirect to {location:?}"));
            }
            continue;
        }
        assert_eq!(reply.status, StatusCode::OK, "{page} should render");

        let values = referenced_urls(&reply.body);
        // Guard against a scan that silently matches nothing.
        assert!(
            !values.is_empty(),
            "{page}: the scan found no URLs to check"
        );
        for value in values {
            if !is_allowed(url_part(&value)) {
                violations.push(format!("{page}: {value:?}"));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "server-rendered pages reference non-/admin URLs:\n{}",
        violations.join("\n")
    );
}

#[tokio::test]
async fn unauthorized_page_offers_a_login_form_when_password_login_is_on() {
    for page in SSR_PAGES {
        let reply = send_to(password_app(), Method::GET, page, &[], None).await;
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{page}");
        assert!(reply.content_type.starts_with("text/html"), "{page}");
        let body = &reply.body;
        assert!(body.contains("<form"), "{page}: no login form");
        assert!(body.contains(r#"name="username""#), "{page}");
        assert!(body.contains(r#"type="password""#), "{page}");
        assert!(body.contains("/admin/api/auth/login"), "{page}");

        // The 401 page follows the same rule as the pages it stands in for.
        let values = referenced_urls(body);
        assert!(
            !values.is_empty(),
            "{page}: the scan found no URLs to check"
        );
        let bad: Vec<_> = values.iter().filter(|v| !is_allowed(url_part(v))).collect();
        assert!(bad.is_empty(), "{page}: 401 page references {bad:?}");
    }
}

#[tokio::test]
async fn token_only_setups_keep_the_plain_unauthorized_page() {
    let reply = get("/admin", &[]).await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
    assert!(!reply.body.contains("<form"), "{}", reply.body);
    assert!(reply.body.contains("X-Octopus-Admin-Token"));
}

#[tokio::test]
async fn the_login_form_endpoint_sets_a_cookie_that_opens_the_page() {
    let login = Request::builder()
        .method(Method::POST)
        .uri("/admin/api/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({"username": "admin", "password": "pw"}).to_string(),
        ))
        .unwrap();
    let response = password_app().oneshot(login).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .expect("session cookie")
        .to_string();

    let page = send_to(
        password_app(),
        Method::GET,
        "/admin",
        &[("cookie", &cookie)],
        None,
    )
    .await;
    assert_eq!(page.status, StatusCode::OK);
}
