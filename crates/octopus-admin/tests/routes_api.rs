//! Admin API route CRUD over host-scoped routes, driven through the real
//! dashboard router with in-process `oneshot` calls.

use std::sync::Arc;

use axum::body::Body;
use http::{Method, Request, StatusCode};
use octopus_admin::{AppState, DashboardRouter};
use octopus_core::UpstreamCluster;
use octopus_router::{HostMatch, Route, RouteBuilder, RouteOrigin, Router};
use serde_json::{json, Value};
use tower::ServiceExt;

/// A router that knows the `web` upstream.
fn router() -> Arc<Router> {
    let router = Router::new();
    router.register_upstream(UpstreamCluster::new("web"));
    Arc::new(router)
}

/// Send one admin API request and return its status and JSON body
/// (`Value::Null` for an empty body, a JSON string for a non-JSON body).
async fn call(
    router: &Arc<Router>,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let app = DashboardRouter::build(Arc::new(AppState::new().with_router(Arc::clone(router))));
    let request = Request::builder().method(method).uri(uri);
    let request = match body {
        Some(body) => request
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
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

/// A FARP-discovered route, tagged the way the FARP crate will tag it.
fn farp_route(method: Method, path: &str, host: &str, upstream: &str) -> Route {
    RouteBuilder::new()
        .id(Some("farp:svc"))
        .origin(RouteOrigin::Farp)
        .method(method)
        .path(path)
        .host(HostMatch::parse_pattern(host).unwrap())
        .upstream_name(upstream)
        .build()
        .unwrap()
}

/// Load a static-config route with id `lpr-1` at `/a` on `a.dev`.
fn load_config_route(router: &Router) {
    let config: octopus_config::types::RouteConfig = serde_json::from_value(json!({
        "id": "lpr-1",
        "path": "/a",
        "methods": ["GET"],
        "hosts": ["a.dev"],
        "upstream": "web"
    }))
    .unwrap();
    for route in config.to_routes(RouteOrigin::Config).unwrap() {
        router.add_route(route).unwrap();
    }
}

#[tokio::test]
async fn put_route_with_hosts_creates_one_entry_per_method_and_host() {
    let router = router();
    let (status, body) = call(
        &router,
        Method::PUT,
        "/admin/api/routes/lpr-1",
        Some(json!({
            "path": "/api/*",
            "methods": ["GET", "POST"],
            "hosts": ["api.acme.dev", "*.acme.dev"],
            "upstream": "web"
        })),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["id"], "lpr-1");
    let entries = router.routes_by_id("lpr-1");
    assert_eq!(entries.len(), 4);
    assert!(entries.iter().all(|r| r.origin == RouteOrigin::Admin));
}

#[tokio::test]
async fn put_same_id_twice_replaces_entries() {
    let router = router();
    let (status, _) = call(
        &router,
        Method::PUT,
        "/admin/api/routes/lpr-1",
        Some(json!({
            "path": "/api/*",
            "methods": ["GET", "POST"],
            "hosts": ["a.dev", "*.a.dev"],
            "upstream": "web"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = call(
        &router,
        Method::PUT,
        "/admin/api/routes/lpr-1",
        Some(json!({
            "path": "/api/*",
            "methods": ["GET"],
            "hosts": ["b.dev"],
            "upstream": "web"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let entries = router.routes_by_id("lpr-1");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].host, HostMatch::Exact("b.dev".to_string()));
}

#[tokio::test]
async fn put_conflicting_declared_route_returns_409() {
    let router = router();
    let route = json!({"path": "/x", "methods": ["GET"], "hosts": ["a.dev"], "upstream": "web"});
    let (status, _) = call(
        &router,
        Method::PUT,
        "/admin/api/routes/lpr-1",
        Some(route.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = call(&router, Method::PUT, "/admin/api/routes/lpr-2", Some(route)).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["conflict"], "lpr-1");
    assert!(body["error"].is_string());
    assert!(router.routes_by_id("lpr-2").is_empty());
    assert_eq!(router.routes_by_id("lpr-1").len(), 1);
}

#[tokio::test]
async fn put_route_with_unknown_upstream_returns_400() {
    let router = router();
    let (status, body) = call(
        &router,
        Method::PUT,
        "/admin/api/routes/lpr-1",
        Some(json!({"path": "/x", "methods": ["GET"], "upstream": "nope"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body["error"].is_string());
    assert!(router.routes_by_id("lpr-1").is_empty());
}

#[tokio::test]
async fn put_route_with_wildcard_in_middle_returns_400() {
    let router = router();
    let (status, body) = call(
        &router,
        Method::PUT,
        "/admin/api/routes/lpr-1",
        Some(json!({"path": "/x", "methods": ["GET"], "hosts": ["api.*.dev"], "upstream": "web"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(router.routes_by_id("lpr-1").is_empty());
}

#[tokio::test]
async fn delete_by_id_keeps_farp_route_at_the_same_path() {
    let router = router();
    router
        .add_route(farp_route(Method::GET, "/x", "a.dev", "svc"))
        .unwrap();

    let (status, body) = call(
        &router,
        Method::PUT,
        "/admin/api/routes/lpr-1",
        Some(json!({"path": "/x", "methods": ["GET"], "hosts": ["a.dev"], "upstream": "web"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let matched = router.match_route("a.dev", &Method::GET, "/x").unwrap();
    assert_eq!(matched.route.upstream_name, "web");

    let (status, _) = call(&router, Method::DELETE, "/admin/api/routes/lpr-1", None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let matched = router.match_route("a.dev", &Method::GET, "/x").unwrap();
    assert_eq!(matched.route.upstream_name, "svc");
}

#[tokio::test]
async fn list_reports_origin_host_and_route_id() {
    let router = router();
    router
        .add_route(farp_route(Method::GET, "/farp", "a.dev", "svc"))
        .unwrap();
    let (status, _) = call(
        &router,
        Method::PUT,
        "/admin/api/routes/lpr-1",
        Some(json!({"path": "/api", "methods": ["GET"], "hosts": ["api.acme.dev"], "upstream": "web"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = call(&router, Method::GET, "/admin/api/routes", None).await;
    assert_eq!(status, StatusCode::OK);
    let entries = body.as_array().unwrap();

    let admin = entries
        .iter()
        .find(|e| e["route_id"] == "lpr-1")
        .expect("admin entry listed");
    assert_eq!(admin["id"], "lpr-1");
    assert_eq!(admin["origin"], "admin");
    assert_eq!(admin["host"], "api.acme.dev");

    let farp = entries
        .iter()
        .find(|e| e["path"] == "/farp")
        .expect("farp entry listed");
    assert_eq!(farp["origin"], "farp");
    assert_eq!(farp["route_id"], "farp:svc");
}

#[tokio::test]
async fn post_with_legacy_single_method_still_works() {
    let router = router();
    let (status, body) = call(
        &router,
        Method::POST,
        "/admin/api/routes",
        Some(json!({"path": "/legacy", "method": "GET", "upstream": "web"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["method"], "GET");
    assert_eq!(body["host"], Value::Null);
    assert_eq!(body["origin"], "admin");

    let id = body["id"].as_str().unwrap();
    assert!(
        uuid::Uuid::parse_str(id).is_ok(),
        "generated id {id} is a UUID"
    );
    let entries = router.routes_by_id(id);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].method, Method::GET);
    assert_eq!(entries[0].host, HostMatch::Any);

    // A second POST with the same explicit id is a conflict.
    let (status, _) = call(
        &router,
        Method::POST,
        "/admin/api/routes",
        Some(json!({"id": id, "path": "/other", "method": "GET", "upstream": "web"})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn get_and_delete_missing_route_return_404() {
    let router = router();
    let (status, _) = call(&router, Method::GET, "/admin/api/routes/nope", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(&router, Method::DELETE, "/admin/api/routes/nope", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = call(
        &router,
        Method::PUT,
        "/admin/api/routes/lpr-1",
        Some(json!({"path": "/x", "methods": ["GET"], "upstream": "web"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = call(&router, Method::GET, "/admin/api/routes/lpr-1", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["route_id"], "lpr-1");
}

#[tokio::test]
async fn put_replaces_config_route_with_the_same_id_in_place() {
    let router = router();
    load_config_route(&router);

    let (status, body) = call(&router, Method::GET, "/admin/api/routes", None).await;
    assert_eq!(status, StatusCode::OK);
    let listed = body
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["route_id"] == "lpr-1")
        .cloned()
        .expect("config route listed");
    assert_eq!(listed["origin"], "config");

    let (status, body) = call(
        &router,
        Method::PUT,
        "/admin/api/routes/lpr-1",
        Some(json!({"path": "/b", "methods": ["GET"], "hosts": ["a.dev"], "upstream": "web"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let entries = router.routes_by_id("lpr-1");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].path, "/b");
    assert_eq!(entries[0].origin, RouteOrigin::Admin);
    assert!(router.match_route("a.dev", &Method::GET, "/a").is_err());
}

#[tokio::test]
async fn delete_removes_a_config_route_by_id() {
    let router = router();
    load_config_route(&router);

    let (status, _) = call(&router, Method::DELETE, "/admin/api/routes/lpr-1", None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(router.routes_by_id("lpr-1").is_empty());
}

#[tokio::test]
async fn put_and_delete_accept_a_legacy_positional_id() {
    let router = router();
    router
        .add_route(
            RouteBuilder::new()
                .origin(RouteOrigin::Config)
                .method(Method::GET)
                .path("/old")
                .upstream_name("web")
                .build()
                .unwrap(),
        )
        .unwrap();

    // The SPA edits an id-less route through the id the list showed it.
    let (status, body) = call(
        &router,
        Method::PUT,
        "/admin/api/routes/route-0",
        Some(json!({"path": "/old", "method": "GET", "upstream": "web"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(router.total_route_count(), 1);
    assert_eq!(router.routes_by_id("route-0").len(), 1);

    let (status, _) = call(&router, Method::DELETE, "/admin/api/routes/route-0", None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(router.total_route_count(), 0);
}

#[tokio::test]
async fn delete_by_legacy_positional_id_removes_only_that_entry() {
    let router = router();
    router
        .add_route(
            RouteBuilder::new()
                .method(Method::GET)
                .path("/old")
                .upstream_name("web")
                .build()
                .unwrap(),
        )
        .unwrap();

    let (status, _) = call(&router, Method::DELETE, "/admin/api/routes/route-0", None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(router.total_route_count(), 0);

    let (status, _) = call(&router, Method::DELETE, "/admin/api/routes/route-0", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn routes_api_without_router_returns_503() {
    let app = DashboardRouter::build(Arc::new(AppState::new()));
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri("/admin/api/routes/lpr-1")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"path": "/x", "upstream": "web"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}
