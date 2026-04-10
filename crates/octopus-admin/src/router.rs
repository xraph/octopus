//! Router configuration for the admin dashboard

use axum::{
    extract::ws::WebSocketUpgrade,
    routing::{delete, get, post, put},
    Router,
};
use std::sync::Arc;
use tower_http::services::{ServeDir, ServeFile};

use crate::api_handlers::{api_analytics_handler, api_realtime_metrics_handler, api_timeseries_handler, api_performance_metrics_handler, api_routes_list_handler, api_route_create_handler, api_route_get_handler, api_route_update_handler, api_route_delete_handler, api_plugins_list_handler, api_plugin_get_handler, api_plugin_toggle_handler, api_plugin_config_handler, api_logs_handler, api_security_events_handler, api_config_list_handler, api_config_update_handler, api_system_info_handler, api_upstreams_list_handler, api_services_list_handler, api_circuits_list_handler, api_health_checks_handler, api_openapi_handler, api_farp_services_handler, api_farp_service_detail_handler, api_farp_federated_openapi_handler};
use crate::handlers::{AppState, overview_handler, routes_handler, health_handler, plugins_handler, analytics_handler, logs_handler, config_handler, api_stats_handler, api_activity_handler, api_health_handler};
use crate::octopus_ui_handlers_pure::{octopus_ui_dashboard_handler, octopus_ui_routes_handler, octopus_ui_health_handler, octopus_ui_plugins_handler};

/// Dashboard router builder
pub struct DashboardRouter;

impl DashboardRouter {
    /// Build the dashboard router
    #[must_use]
    pub fn build(state: Arc<AppState>) -> Router {
        // UI distribution path
        let ui_dist = concat!(env!("CARGO_MANIFEST_DIR"), "/ui/dist");

        Router::new()
            // ===== Server-rendered pages (Askama templates) =====
            .route("/admin", get(overview_handler))
            .route("/admin/routes", get(routes_handler))
            .route("/admin/health", get(health_handler))
            .route("/admin/plugins", get(plugins_handler))
            .route("/admin/analytics", get(analytics_handler))
            .route("/admin/logs", get(logs_handler))
            .route("/admin/config", get(config_handler))
            // ===== Octopus-UI powered pages (modern UI with octopus-ui components) =====
            .route("/admin/octopus-ui", get(octopus_ui_dashboard_handler))
            .route("/admin/octopus-ui/routes", get(octopus_ui_routes_handler))
            .route("/admin/octopus-ui/health", get(octopus_ui_health_handler))
            .route("/admin/octopus-ui/plugins", get(octopus_ui_plugins_handler))
            // ===== Metrics & Analytics API =====
            .route("/admin/api/stats", get(api_stats_handler))
            .route("/admin/api/analytics", get(api_analytics_handler))
            .route(
                "/admin/api/metrics/realtime",
                get(api_realtime_metrics_handler),
            )
            .route("/admin/api/metrics/timeseries", get(api_timeseries_handler))
            .route(
                "/admin/api/metrics/performance",
                get(api_performance_metrics_handler),
            )
            // ===== Routes Management API (CRUD) =====
            .route("/admin/api/routes", get(api_routes_list_handler))
            .route("/admin/api/routes", post(api_route_create_handler))
            .route("/admin/api/routes/:id", get(api_route_get_handler))
            .route("/admin/api/routes/:id", put(api_route_update_handler))
            .route("/admin/api/routes/:id", delete(api_route_delete_handler))
            // ===== Plugin Management API =====
            .route("/admin/api/plugins", get(api_plugins_list_handler))
            .route("/admin/api/plugins/:id", get(api_plugin_get_handler))
            .route(
                "/admin/api/plugins/:id/toggle",
                post(api_plugin_toggle_handler),
            )
            .route(
                "/admin/api/plugins/:id/config",
                put(api_plugin_config_handler),
            )
            // ===== Logs & Monitoring API =====
            .route("/admin/api/logs", get(api_logs_handler))
            .route("/admin/api/activity", get(api_activity_handler))
            .route("/admin/api/health", get(api_health_handler))
            .route(
                "/admin/api/security/events",
                get(api_security_events_handler),
            )
            // ===== Configuration Management API =====
            .route("/admin/api/config", get(api_config_list_handler))
            .route("/admin/api/config/:key", put(api_config_update_handler))
            // ===== Upstreams, Services & Circuits API =====
            .route("/admin/api/upstreams", get(api_upstreams_list_handler))
            .route("/admin/api/services", get(api_services_list_handler))
            .route("/admin/api/circuits", get(api_circuits_list_handler))
            .route("/admin/api/health/checks", get(api_health_checks_handler))
            .route("/admin/api/openapi.json", get(api_openapi_handler))
            // ===== FARP (Federated API Registry Protocol) API =====
            .route("/admin/api/farp/services", get(api_farp_services_handler))
            .route("/admin/api/farp/services/:name", get(api_farp_service_detail_handler))
            .route("/admin/api/farp/schema/openapi", get(api_farp_federated_openapi_handler))
            // ===== System Information API =====
            .route("/admin/api/system/info", get(api_system_info_handler))
            // ===== WebSocket for real-time dashboard events =====
            .route("/admin/ws", get({
                let ws_hub = state.ws_hub.clone();
                move |ws: WebSocketUpgrade| async move {
                    crate::websocket::WsHub::handle_upgrade(ws_hub, ws).await
                }
            }))
            // ===== Static assets =====
            .nest_service(
                "/admin/static",
                ServeDir::new(concat!(env!("CARGO_MANIFEST_DIR"), "/static")),
            )
            // ===== Next.js React UI (optional SPA) =====
            // Structure generated by Next.js:
            //   - /_next/static/chunks/*.js    - JavaScript chunks
            //   - /_next/static/media/*        - Fonts, images
            //   - /overview/index.html         - Pre-rendered pages
            //   - /routes/index.html
            //   - /health/index.html
            //   - /plugins/index.html
            //   - /index.html                  - Landing page
            .nest_service(
                "/admin/ui",
                ServeDir::new(ui_dist)
                    .append_index_html_on_directories(true)
                    .not_found_service(ServeFile::new(format!("{ui_dist}/index.html")))
                    .precompressed_gzip()
                    .precompressed_br(),
            )
            .with_state(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_router() {
        let state = Arc::new(AppState::new());
        let app = DashboardRouter::build(state);

        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/admin")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }
}
