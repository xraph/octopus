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
    let response = app().oneshot(request).await.unwrap();
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

#[tokio::test]
async fn ssr_pages_only_reference_admin_paths() {
    let attr_double = Regex::new(
        r#"(?i)[\s:@](?:href|src|action|hx-get|hx-post|hx-put|hx-delete|hx-patch)\s*=\s*"([^"]*)""#,
    )
    .unwrap();
    let attr_single = Regex::new(
        r"(?i)[\s:@](?:href|src|action|hx-get|hx-post|hx-put|hx-delete|hx-patch)\s*=\s*'([^']*)'",
    )
    .unwrap();
    let call_literal = Regex::new(r#"(?:fetch|new\s+WebSocket)\(\s*(['"`])([^'"`]*)"#).unwrap();

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

        let values = attr_double
            .captures_iter(&reply.body)
            .chain(attr_single.captures_iter(&reply.body))
            .map(|c| c[1].to_string())
            .chain(
                call_literal
                    .captures_iter(&reply.body)
                    .map(|c| c[2].to_string()),
            );
        let mut scanned = 0;
        for value in values {
            scanned += 1;
            if !is_allowed(url_part(&value)) {
                violations.push(format!("{page}: {value:?}"));
            }
        }
        // Guard against a scan that silently matches nothing.
        assert!(scanned > 0, "{page}: the scan found no URLs to check");
    }
    assert!(
        violations.is_empty(),
        "server-rendered pages reference non-/admin URLs:\n{}",
        violations.join("\n")
    );
}
