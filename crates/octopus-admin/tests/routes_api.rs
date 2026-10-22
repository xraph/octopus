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
    let state = Arc::new(AppState::new().with_router(Arc::clone(router)));
    call_state(state, method, uri, body).await
}

/// Send one admin API request against `state`; see [`call`].
async fn call_state(
    state: Arc<AppState>,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let app = DashboardRouter::build(state);
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

    // The replaced route gets a fresh UUID id, never the positional one, so
    // the list never shows two entries under one id.
    let id = body["id"].as_str().unwrap().to_string();
    assert!(uuid::Uuid::parse_str(&id).is_ok(), "new id {id} is a UUID");
    assert_eq!(body["route_id"], id.as_str());
    assert!(router.routes_by_id("route-0").is_empty());
    assert_eq!(router.routes_by_id(&id).len(), 1);

    let (_, listed) = call(&router, Method::GET, "/admin/api/routes", None).await;
    let ids: Vec<&str> = listed
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec![id.as_str()]);

    let (status, got) = call(
        &router,
        Method::GET,
        &format!("/admin/api/routes/{id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(got["path"], "/old");

    let (status, _) = call(
        &router,
        Method::DELETE,
        &format!("/admin/api/routes/{id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(router.total_route_count(), 0);
}

#[tokio::test]
async fn positional_shaped_ids_are_never_created() {
    let router = router();
    let body = json!({"path": "/x", "methods": ["GET"], "upstream": "web"});

    let (status, reply) = call(
        &router,
        Method::PUT,
        "/admin/api/routes/route-7",
        Some(body.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{reply}");
    assert!(reply["error"].is_string());

    let mut post = body.clone();
    post["id"] = json!("route-3");
    let (status, reply) = call(&router, Method::POST, "/admin/api/routes", Some(post)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{reply}");

    assert_eq!(router.total_route_count(), 0);

    // `route-` followed by something other than digits is an ordinary id.
    let (status, _) = call(
        &router,
        Method::PUT,
        "/admin/api/routes/route-web",
        Some(body),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(router.routes_by_id("route-web").len(), 1);
}

#[tokio::test]
async fn conflict_check_compares_trie_nodes_not_path_spelling() {
    for (held, other) in [
        ("/x", "/x/"),
        ("/billing/*", "/billing/*rest"),
        ("/users/:id", "/users/:uid"),
    ] {
        let router = router();
        let (status, _) = call(
            &router,
            Method::PUT,
            "/admin/api/routes/lpr-1",
            Some(json!({"path": held, "methods": ["GET"], "hosts": ["a.dev"], "upstream": "web"})),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let (status, body) = call(
            &router,
            Method::PUT,
            "/admin/api/routes/lpr-2",
            Some(json!({"path": other, "methods": ["GET"], "hosts": ["a.dev"], "upstream": "web"})),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "{held} vs {other}: {body}");
        assert_eq!(body["conflict"], "lpr-1");
        assert!(router.routes_by_id("lpr-2").is_empty());
    }

    // A different host on the same node is not a conflict.
    let router = router();
    let (status, _) = call(
        &router,
        Method::PUT,
        "/admin/api/routes/lpr-1",
        Some(json!({"path": "/x", "methods": ["GET"], "hosts": ["a.dev"], "upstream": "web"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call(
        &router,
        Method::PUT,
        "/admin/api/routes/lpr-2",
        Some(json!({"path": "/x/", "methods": ["GET"], "hosts": ["b.dev"], "upstream": "web"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn malformed_bodies_get_a_json_400() {
    let router = router();
    for body in [
        json!({"path": "/x", "methods": ["GET"]}),
        json!({"upstream": "web"}),
        json!({"path": 5, "upstream": "web"}),
    ] {
        for (method, uri) in [
            (Method::PUT, "/admin/api/routes/lpr-1"),
            (Method::POST, "/admin/api/routes"),
        ] {
            let (status, reply) = call(&router, method.clone(), uri, Some(body.clone())).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{method} {body}: {reply}");
            assert!(reply["error"].is_string(), "{method} {body}: {reply}");
        }
    }
    assert_eq!(router.total_route_count(), 0);
}

#[tokio::test]
async fn put_and_delete_leave_discovered_routes_to_their_discoverer() {
    let router = router();
    router
        .add_route(farp_route(Method::GET, "/x", "a.dev", "svc"))
        .unwrap();

    let (status, body) = call(
        &router,
        Method::PUT,
        "/admin/api/routes/farp:svc",
        Some(json!({"path": "/x", "methods": ["GET"], "hosts": ["a.dev"], "upstream": "web"})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["conflict"], "farp:svc");

    let (status, _) = call(&router, Method::DELETE, "/admin/api/routes/farp:svc", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let entries = router.routes_by_id("farp:svc");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].origin, RouteOrigin::Farp);
    assert_eq!(entries[0].upstream_name, "svc");
}

#[tokio::test]
async fn put_path_id_beats_body_id() {
    let router = router();
    let (status, body) = call(
        &router,
        Method::PUT,
        "/admin/api/routes/lpr-path",
        Some(json!({"id": "lpr-body", "path": "/x", "methods": ["GET"], "upstream": "web"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["id"], "lpr-path");
    assert_eq!(router.routes_by_id("lpr-path").len(), 1);
    assert!(router.routes_by_id("lpr-body").is_empty());
}

#[tokio::test]
async fn manual_route_shadows_a_higher_priority_farp_route_until_deleted() {
    let router = router();
    let mut farp = farp_route(Method::GET, "/x", "a.dev", "svc");
    farp.priority = 1000;
    router.add_route(farp).unwrap();

    let (status, body) = call(
        &router,
        Method::PUT,
        "/admin/api/routes/lpr-1",
        Some(json!({"path": "/x", "methods": ["GET"], "hosts": ["a.dev"], "upstream": "web", "priority": 0})),
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_puts_of_one_id_leave_one_route() {
    let router = router();
    let calls: Vec<_> = (0..32)
        .map(|i| {
            let router = Arc::clone(&router);
            tokio::spawn(async move {
                let host = format!("h{}.dev", i % 4);
                call(
                    &router,
                    Method::PUT,
                    "/admin/api/routes/lpr-1",
                    Some(json!({"path": "/x", "methods": ["GET"], "hosts": [host], "upstream": "web"})),
                )
                .await
                .0
            })
        })
        .collect();
    for task in calls {
        assert_eq!(task.await.unwrap(), StatusCode::OK);
    }
    assert_eq!(router.routes_by_id("lpr-1").len(), 1);
    assert_eq!(router.total_route_count(), 1);
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

/// Admin state over `router` with a FARP binding cell holding `binding`.
fn farp_state(router: &Arc<Router>, binding: octopus_farp::GatewayBinding) -> Arc<AppState> {
    let cell = octopus_farp::new_binding_cell();
    cell.store(Arc::new(Some(binding)));
    let mut state = AppState::new().with_router(Arc::clone(router));
    state.farp_binding = Some(cell);
    Arc::new(state)
}

#[tokio::test]
async fn put_farp_binding_rehosts_farp_routes() {
    let router = router();
    router
        .add_route(farp_route(Method::GET, "/svc/x", "old.dev", "web"))
        .unwrap();
    let binding = octopus_farp::GatewayBinding::for_hostnames(&["old.dev".into()])
        .with_gateway_id(Some("platform-api".into()));
    let state = farp_state(&router, binding);

    let (status, body) = call_state(
        Arc::clone(&state),
        Method::PUT,
        "/admin/api/farp/binding",
        Some(json!({"hostnames": ["new.dev"]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body,
        json!({"hostnames": ["new.dev"], "gateway_id": "platform-api", "farp_routes": 1})
    );
    assert!(router
        .match_route("new.dev", &Method::GET, "/svc/x")
        .is_ok());
    assert!(router
        .match_route("old.dev", &Method::GET, "/svc/x")
        .is_err());

    let (status, body) = call_state(state, Method::GET, "/admin/api/farp/binding", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["hostnames"], json!(["new.dev"]));
    assert_eq!(body["farp_routes"], json!(1));
}

#[tokio::test]
async fn put_farp_binding_dedupes_and_keeps_manual_routes() {
    let router = router();
    router
        .add_route(farp_route(Method::GET, "/svc/x", "old.dev", "web"))
        .unwrap();
    load_config_route(&router);
    let state = farp_state(
        &router,
        octopus_farp::GatewayBinding::for_hostnames(&["old.dev".into()]),
    );

    let (status, body) = call_state(
        state,
        Method::PUT,
        "/admin/api/farp/binding",
        Some(json!({"hostnames": ["A.dev", "a.dev.", "*.b.dev"]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["hostnames"], json!(["a.dev", "*.b.dev"]));
    assert_eq!(body["farp_routes"], json!(2));
    assert!(router
        .match_route("x.b.dev", &Method::GET, "/svc/x")
        .is_ok());
    assert_eq!(router.routes_by_id("lpr-1").len(), 1);
    assert_eq!(
        router.routes_by_id("lpr-1")[0].host,
        HostMatch::Exact("a.dev".into())
    );
}

#[tokio::test]
async fn put_farp_binding_rejects_a_bad_hostname() {
    let router = router();
    router
        .add_route(farp_route(Method::GET, "/svc/x", "old.dev", "web"))
        .unwrap();
    let state = farp_state(
        &router,
        octopus_farp::GatewayBinding::for_hostnames(&["old.dev".into()]),
    );

    let (status, body) = call_state(
        state,
        Method::PUT,
        "/admin/api/farp/binding",
        Some(json!({"hostnames": ["ok.dev", "api.*.dev"]})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"].as_str().unwrap().contains("api.*.dev"),
        "{body}"
    );
    assert!(router
        .match_route("old.dev", &Method::GET, "/svc/x")
        .is_ok());
}

#[tokio::test]
async fn put_farp_binding_without_a_binding_creates_one() {
    let router = router();
    let cell = octopus_farp::new_binding_cell();
    let mut state = AppState::new().with_router(Arc::clone(&router));
    state.farp_binding = Some(Arc::clone(&cell));

    let (status, body) = call_state(
        Arc::new(state),
        Method::PUT,
        "/admin/api/farp/binding",
        Some(json!({"hostnames": ["api.acme.dev"]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body,
        json!({"hostnames": ["api.acme.dev"], "gateway_id": null, "farp_routes": 0})
    );
    let stored = cell.load();
    let binding = (**stored).as_ref().expect("binding stored in the cell");
    assert_eq!(binding.hosts, vec![HostMatch::Exact("api.acme.dev".into())]);
}

#[tokio::test]
async fn farp_binding_without_farp_returns_404() {
    let router = router();
    for (method, body) in [
        (Method::GET, None),
        (Method::PUT, Some(json!({"hostnames": ["a.dev"]}))),
    ] {
        let (status, value) = call(&router, method, "/admin/api/farp/binding", body).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(value, json!({"error": "FARP is not enabled"}));
    }
}
