//! Router configuration for the admin dashboard

use axum::{
    extract::ws::WebSocketUpgrade,
    routing::{delete, get, post, put},
    Router,
};
use std::sync::Arc;
use tower_http::services::{ServeDir, ServeFile};

use crate::api_handlers::{
    api_analytics_handler, api_circuits_list_handler, api_config_list_handler,
    api_config_update_handler, api_farp_federated_openapi_handler, api_farp_service_detail_handler,
    api_farp_services_handler, api_health_checks_handler, api_logs_handler, api_openapi_handler,
    api_performance_metrics_handler, api_plugin_config_handler, api_plugin_get_handler,
    api_plugin_toggle_handler, api_plugins_list_handler, api_realtime_metrics_handler,
    api_route_create_handler, api_route_delete_handler, api_route_get_handler,
    api_route_update_handler, api_routes_list_handler, api_security_events_handler,
    api_services_list_handler, api_system_info_handler, api_timeseries_handler,
    api_upstreams_list_handler,
};
use crate::handlers::{
    analytics_handler, api_activity_handler, api_health_handler, api_stats_handler, config_handler,
    health_handler, logs_handler, overview_handler, plugins_handler, routes_handler, AppState,
};
use crate::octopus_ui_handlers_pure::{
    octopus_ui_dashboard_handler, octopus_ui_health_handler, octopus_ui_plugins_handler,
    octopus_ui_routes_handler,
};
use crate::auth::{api_auth_login_handler, api_auth_logout_handler, api_auth_me_handler};
use crate::k8s_handlers::{
    api_k8s_gateways_handler, api_k8s_policies_handler, api_k8s_routes_handler,
    api_k8s_status_handler, api_k8s_upstreams_handler,
};
use crate::tls_handlers::{
    api_tls_cert_detail_handler, api_tls_cert_upload_handler, api_tls_certs_list_handler,
    api_tls_reload_handler,
};
use crate::upstream_handlers::{
    api_upstream_create_handler, api_upstream_delete_handler, api_upstream_get_handler,
    api_upstream_update_handler,
};

/// Dashboard router builder
pub struct DashboardRouter;

impl DashboardRouter {
    /// Build the dashboard router
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
            .route("/admin/api/upstreams", post(api_upstream_create_handler))
            .route("/admin/api/upstreams/:name", get(api_upstream_get_handler))
            .route("/admin/api/upstreams/:name", put(api_upstream_update_handler))
            .route("/admin/api/upstreams/:name", delete(api_upstream_delete_handler))
            .route("/admin/api/services", get(api_services_list_handler))
            .route("/admin/api/circuits", get(api_circuits_list_handler))
            .route("/admin/api/health/checks", get(api_health_checks_handler))
            .route("/admin/api/openapi.json", get(api_openapi_handler))
            // ===== TLS / Certificates API =====
            .route("/admin/api/tls/certs", get(api_tls_certs_list_handler))
            .route("/admin/api/tls/certs", post(api_tls_cert_upload_handler))
            .route("/admin/api/tls/certs/:name", get(api_tls_cert_detail_handler))
            .route("/admin/api/tls/reload", post(api_tls_reload_handler))
            // ===== Kubernetes CRD views API =====
            .route("/admin/api/k8s/gateways", get(api_k8s_gateways_handler))
            .route("/admin/api/k8s/routes", get(api_k8s_routes_handler))
            .route("/admin/api/k8s/policies", get(api_k8s_policies_handler))
            .route("/admin/api/k8s/upstreams", get(api_k8s_upstreams_handler))
            .route("/admin/api/k8s/status", get(api_k8s_status_handler))
            // ===== Admin authentication / session API =====
            .route("/admin/api/auth/login", post(api_auth_login_handler))
            .route("/admin/api/auth/logout", post(api_auth_logout_handler))
            .route("/admin/api/auth/me", get(api_auth_me_handler))
            // ===== FARP (Federated API Registry Protocol) API =====
            .route("/admin/api/farp/services", get(api_farp_services_handler))
            .route(
                "/admin/api/farp/services/:name",
                get(api_farp_service_detail_handler),
            )
            .route(
                "/admin/api/farp/schema/openapi",
                get(api_farp_federated_openapi_handler),
            )
            // ===== System Information API =====
            .route("/admin/api/system/info", get(api_system_info_handler))
            // ===== Auth Configuration API =====
            .route(
                "/admin/api/auth/providers",
                get(crate::api_handlers::api_auth_providers_handler),
            )
            .route(
                "/admin/api/auth/config",
                get(crate::api_handlers::api_auth_config_handler),
            )
            // ===== gRPC Configuration API =====
            .route(
                "/admin/api/grpc/services",
                get(crate::api_handlers::api_grpc_services_handler),
            )
            // ===== WebSocket for real-time dashboard events =====
            .route(
                "/admin/ws",
                get({
                    let ws_hub = state.ws_hub.clone();
                    move |ws: WebSocketUpgrade| async move {
                        crate::websocket::WsHub::handle_upgrade(ws_hub, ws).await
                    }
                }),
            )
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
            // Enforce the admin session on protected endpoints. A pass-through
            // when `state.admin_auth` is `None` (auth disabled).
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::auth::require_admin_session,
            ))
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
