//! `GET /admin/api/system/info` reports a `start_time` that only changes when
//! the process restarts.

use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use http::{Request, StatusCode};
use octopus_admin::{AppState, DashboardRouter};
use serde_json::Value;
use tower::ServiceExt;

async fn system_info(state: &Arc<AppState>) -> Value {
    let app = DashboardRouter::build(Arc::clone(state));
    let request = Request::builder()
        .uri("/admin/api/system/info")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

/// Call twice, far enough apart that `now - floor(uptime)` would cross a
/// second boundary at least once in a few tries.
async fn assert_stable(state: Arc<AppState>) {
    let first = system_info(&state).await["start_time"].clone();
    assert!(first.as_str().is_some_and(|s| s.len() == 19), "{first}");
    for _ in 0..6 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        let again = system_info(&state).await["start_time"].clone();
        assert_eq!(first, again, "start_time moved without a restart");
    }
}

#[tokio::test]
async fn start_time_is_stable_without_metrics() {
    assert_stable(Arc::new(AppState::new())).await;
}

#[tokio::test]
async fn start_time_is_stable_with_metrics() {
    let metrics = Arc::new(octopus_metrics::MetricsCollector::new());
    assert_stable(Arc::new(AppState::new().with_metrics(metrics))).await;
}
