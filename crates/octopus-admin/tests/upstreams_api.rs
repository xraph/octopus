//! Admin API upstream writes with the exact bodies a control plane sends,
//! driven through the real dashboard router with in-process `oneshot` calls.

use std::sync::Arc;

use axum::body::Body;
use http::{Method, Request, StatusCode};
use octopus_admin::{AppState, DashboardRouter};
use octopus_router::Router;
use serde_json::{json, Value};
use tower::ServiceExt;

/// Send one admin API request and return its status and JSON body
/// (`Value::Null` for an empty body). `raw` bodies go out unparsed.
async fn call(
    router: &Arc<Router>,
    method: Method,
    uri: &str,
    raw: Option<&str>,
) -> (StatusCode, Value) {
    let state = Arc::new(AppState::new().with_router(Arc::clone(router)));
    let app = DashboardRouter::build(state);
    let request = Request::builder().method(method).uri(uri);
    let request = match raw {
        Some(body) => request
            .header("content-type", "application/json")
            .body(Body::from(body.to_owned()))
            .unwrap(),
        None => request.body(Body::empty()).unwrap(),
    };
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()))
    };
    (status, value)
}

/// The upstream body from the cross-team contract, byte for byte.
const CONTRACT_UPSTREAM: &str = r#"{"strategy":"round_robin","instances":[{"id":"lp-billing-0","address":"billing","port":8080}]}"#;

#[tokio::test]
async fn put_upstream_accepts_the_contract_body_without_a_name() {
    let router = Arc::new(Router::new());
    let (status, body) = call(
        &router,
        Method::PUT,
        "/admin/api/upstreams/lp-billing-8080",
        Some(CONTRACT_UPSTREAM),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["name"], "lp-billing-8080");
    assert_eq!(body["instance_count"], 1);
    assert_eq!(body["instances"][0]["id"], "lp-billing-0");
    assert_eq!(body["instances"][0]["address"], "billing");
    assert_eq!(body["instances"][0]["port"], 8080);
    assert!(router.get_upstream("lp-billing-8080").is_some());
}

#[tokio::test]
async fn put_upstream_path_name_wins_over_the_body_name() {
    let router = Arc::new(Router::new());
    let (status, body) = call(
        &router,
        Method::PUT,
        "/admin/api/upstreams/lp-a",
        Some(r#"{"name":"lp-b","instances":[{"address":"a","port":80}]}"#),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["name"], "lp-a");
    assert!(router.get_upstream("lp-a").is_some());
    assert!(router.get_upstream("lp-b").is_none());
}

#[tokio::test]
async fn contract_upstream_then_route_then_delete() {
    let router = Arc::new(Router::new());
    let (status, body) = call(
        &router,
        Method::PUT,
        "/admin/api/upstreams/lp-billing-8080",
        Some(CONTRACT_UPSTREAM),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let route = json!({
        "path": "/billing/*",
        "methods": ["GET", "POST"],
        "hosts": ["api.acme.dev"],
        "upstream": "lp-billing-8080",
        "strip_prefix": "/billing",
        "priority": 0
    })
    .to_string();
    let (status, body) = call(
        &router,
        Method::PUT,
        "/admin/api/routes/lpr-billing",
        Some(&route),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["upstream"], "lp-billing-8080");

    let (status, body) = call(
        &router,
        Method::DELETE,
        "/admin/api/upstreams/lp-billing-8080",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    assert!(router.get_upstream("lp-billing-8080").is_none());

    let (status, _) = call(
        &router,
        Method::DELETE,
        "/admin/api/upstreams/lp-billing-8080",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn bad_upstream_bodies_answer_400_json() {
    let router = Arc::new(Router::new());
    for (method, uri) in [
        (Method::PUT, "/admin/api/upstreams/lp-x"),
        (Method::POST, "/admin/api/upstreams"),
    ] {
        // An instance without a port, then a body that is not JSON at all.
        for raw in [
            r#"{"name":"lp-x","instances":[{"address":"a"}]}"#,
            "{not json",
        ] {
            let (status, body) = call(&router, method.clone(), uri, Some(raw)).await;
            assert_eq!(
                status,
                StatusCode::BAD_REQUEST,
                "{method} {uri} {raw}: {body}"
            );
            assert!(
                body["error"].as_str().is_some_and(|e| !e.is_empty()),
                "{method} {uri} {raw}: {body}"
            );
        }
    }
    assert!(router.get_upstream("lp-x").is_none());
}

#[tokio::test]
async fn post_upstream_still_requires_a_name() {
    let router = Arc::new(Router::new());
    let (status, body) = call(
        &router,
        Method::POST,
        "/admin/api/upstreams",
        Some(CONTRACT_UPSTREAM),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "Upstream name is required");
}
