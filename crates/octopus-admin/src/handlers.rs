//! HTTP handlers for the admin dashboard

use askama::Template;
use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    Json,
};
use std::sync::Arc;

use crate::models::*;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    // Add your application state here
    // For example: router, health checker, metrics collector, etc.
}

impl AppState {
    /// Create a new application state
    #[must_use]
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper struct for rendering Askama templates
pub struct HtmlTemplate<T>(pub T);

impl<T> IntoResponse for HtmlTemplate<T>
where
    T: Template,
{
    fn into_response(self) -> Response {
        match self.0.render() {
            Ok(html) => Html(html).into_response(),
            Err(err) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to render template: {err}"),
            )
                .into_response(),
        }
    }
}

/// Overview page template (Preline UI with ApexCharts)
#[derive(Template)]
#[template(path = "preline_dashboard.html")]
pub struct OverviewTemplate {
    pub total_requests: u64,
    pub active_routes: usize,
    pub avg_latency_ms: f64,
    pub health_status: String,
    pub plugin_cards: Vec<PluginStatsCard>,
}

/// Legacy overview page template (shadcn design with Alpine.js and Chart.js)
#[derive(Template)]
#[template(path = "shadcn_overview_enhanced.html")]
pub struct LegacyOverviewEnhancedTemplate {
    pub total_requests: u64,
    pub active_routes: usize,
    pub avg_latency_ms: f64,
    pub health_status: String,
    pub plugin_cards: Vec<PluginStatsCard>,
}

/// Routes page template (shadcn design with Alpine.js enhancements)
#[derive(Template)]
#[template(path = "shadcn_routes_enhanced.html")]
pub struct RoutesTemplate {
    pub routes: Vec<RouteInfo>,
    pub page_start: usize,
    pub page_end: usize,
    pub total_routes: usize,
}

/// Health page template (shadcn design with Alpine.js enhancements)
#[derive(Template)]
#[template(path = "shadcn_health_enhanced.html")]
pub struct HealthTemplate {
    pub overall_status: String,
    pub last_check_time: String,
    pub health_checks: Vec<HealthCheckInfo>,
    pub uptime_24h: f64,
    pub uptime_7d: f64,
    pub uptime_30d: f64,
}

/// Plugins page template (shadcn design with Alpine.js enhancements)
#[derive(Template)]
#[template(path = "shadcn_plugins_enhanced.html")]
pub struct PluginsTemplate {
    pub plugins: Vec<PluginInfo>,
    pub total_plugins: usize,
    pub active_plugins: usize,
    pub updates_available: usize,
}

/// Legacy routes page template (for backwards compatibility)
#[derive(Template)]
#[template(path = "routes.html")]
pub struct LegacyRoutesTemplate {
    pub routes: Vec<RouteInfo>,
    pub page_start: usize,
    pub page_end: usize,
    pub total_routes: usize,
}

/// Legacy health page template (for backwards compatibility)
#[derive(Template)]
#[template(path = "health.html")]
pub struct LegacyHealthTemplate {
    pub overall_status: String,
    pub last_check_time: String,
    pub health_checks: Vec<HealthCheckInfo>,
    pub uptime_24h: f64,
    pub uptime_7d: f64,
    pub uptime_30d: f64,
}

/// Legacy plugins page template (for backwards compatibility)
#[derive(Template)]
#[template(path = "plugins.html")]
pub struct LegacyPluginsTemplate {
    pub plugins: Vec<PluginInfo>,
    pub total_plugins: usize,
    pub active_plugins: usize,
    pub updates_available: usize,
}

/// Overview page handler
pub async fn overview_handler(State(_state): State<Arc<AppState>>) -> impl IntoResponse {
    // TODO: Fetch real data from state
    let template = OverviewTemplate {
        total_requests: 1_234_567,
        active_routes: 42,
        avg_latency_ms: 45.6,
        health_status: "healthy".to_string(),
        plugin_cards: vec![],
    };

    HtmlTemplate(template)
}

/// Routes page handler
pub async fn routes_handler(State(_state): State<Arc<AppState>>) -> impl IntoResponse {
    // TODO: Fetch real data from state
    let routes = vec![
        RouteInfo {
            id: "route1".to_string(),
            path: "/api/users".to_string(),
            method: "GET".to_string(),
            upstream: "user-service".to_string(),
            request_count: 456,
            is_healthy: true,
            avg_latency_ms: 23.5,
            error_count: 2,
            last_accessed: Some("2024-01-15 10:30:00".to_string()),
        },
        RouteInfo {
            id: "route2".to_string(),
            path: "/api/products".to_string(),
            method: "POST".to_string(),
            upstream: "product-service".to_string(),
            request_count: 123,
            is_healthy: true,
            avg_latency_ms: 34.2,
            error_count: 1,
            last_accessed: Some("2024-01-15 10:25:00".to_string()),
        },
    ];

    let template = RoutesTemplate {
        routes,
        page_start: 1,
        page_end: 2,
        total_routes: 2,
    };

    HtmlTemplate(template)
}

/// Health page handler
pub async fn health_handler(State(_state): State<Arc<AppState>>) -> impl IntoResponse {
    // TODO: Fetch real data from state
    let health_checks = vec![
        HealthCheckInfo {
            name: "Database".to_string(),
            status: "passing".to_string(),
            response_time_ms: 12,
            message: Some("Connected successfully".to_string()),
            endpoint: Some("postgresql://localhost:5432".to_string()),
            last_check: "2024-01-15 10:30:00".to_string(),
            consecutive_failures: 0,
        },
        HealthCheckInfo {
            name: "Redis".to_string(),
            status: "passing".to_string(),
            response_time_ms: 5,
            message: Some("Cache operational".to_string()),
            endpoint: Some("redis://localhost:6379".to_string()),
            last_check: "2024-01-15 10:30:00".to_string(),
            consecutive_failures: 0,
        },
    ];

    let template = HealthTemplate {
        overall_status: "healthy".to_string(),
        last_check_time: "2024-01-15 10:30:00".to_string(),
        health_checks,
        uptime_24h: 99.9,
        uptime_7d: 99.8,
        uptime_30d: 99.7,
    };

    HtmlTemplate(template)
}

/// Plugins page handler
pub async fn plugins_handler(State(_state): State<Arc<AppState>>) -> impl IntoResponse {
    // TODO: Fetch real data from state
    let plugins = vec![
        PluginInfo {
            id: "auth-jwt".to_string(),
            name: "JWT Authentication".to_string(),
            version: "0.1.0".to_string(),
            description: "JWT token validation and authentication".to_string(),
            author: Some("Octopus Team".to_string()),
            enabled: true,
            has_dashboard: true,
            config: None,
        },
        PluginInfo {
            id: "rate-limiter".to_string(),
            name: "Rate Limiter".to_string(),
            version: "0.1.0".to_string(),
            description: "Request rate limiting with token bucket algorithm".to_string(),
            author: Some("Octopus Team".to_string()),
            enabled: true,
            has_dashboard: false,
            config: None,
        },
    ];

    let active_plugins = plugins.iter().filter(|p| p.enabled).count();

    let template = PluginsTemplate {
        plugins: plugins.clone(),
        total_plugins: plugins.len(),
        active_plugins,
        updates_available: 0,
    };

    HtmlTemplate(template)
}

/// Analytics page template
#[derive(Template)]
#[template(path = "analytics.html")]
pub struct AnalyticsTemplate {
    pub timeframe: String,
}

/// Analytics page handler
pub async fn analytics_handler(State(_state): State<Arc<AppState>>) -> impl IntoResponse {
    let template = AnalyticsTemplate {
        timeframe: "24h".to_string(),
    };

    HtmlTemplate(template)
}

/// Logs page template
#[derive(Template)]
#[template(path = "logs.html")]
pub struct LogsTemplate {
    pub total_logs: usize,
}

/// Logs page handler
pub async fn logs_handler(State(_state): State<Arc<AppState>>) -> impl IntoResponse {
    let template = LogsTemplate { total_logs: 0 };

    HtmlTemplate(template)
}

/// Configuration page template
#[derive(Template)]
#[template(path = "config.html")]
pub struct ConfigTemplate {
    pub total_config: usize,
}

/// Configuration page handler
pub async fn config_handler(State(_state): State<Arc<AppState>>) -> impl IntoResponse {
    let template = ConfigTemplate { total_config: 0 };

    HtmlTemplate(template)
}

// API Endpoints for HTMX

/// API: Get stats (for HTMX auto-refresh)
pub async fn api_stats_handler(State(_state): State<Arc<AppState>>) -> impl IntoResponse {
    // TODO: Fetch real metrics
    let stats = DashboardStats::default();

    Json(stats)
}

/// API: Get recent activity (for HTMX auto-refresh)
pub async fn api_activity_handler(State(_state): State<Arc<AppState>>) -> impl IntoResponse {
    // TODO: Fetch real activity logs
    let activities = vec![ActivityLogEntry {
        timestamp: "2024-01-15 10:30:00".to_string(),
        level: "info".to_string(),
        message: "Route /api/users registered".to_string(),
        details: None,
        source: Some("router".to_string()),
    }];

    Json(activities)
}

/// API: Get health checks (for HTMX auto-refresh)
pub async fn api_health_handler(State(_state): State<Arc<AppState>>) -> impl IntoResponse {
    // TODO: Fetch real health checks
    let health_checks = vec![HealthCheckInfo {
        name: "Database".to_string(),
        status: "passing".to_string(),
        response_time_ms: 12,
        message: Some("Connected successfully".to_string()),
        endpoint: Some("postgresql://localhost:5432".to_string()),
        last_check: "2024-01-15 10:30:00".to_string(),
        consecutive_failures: 0,
    }];

    Json(health_checks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_overview_handler() {
        let state = Arc::new(AppState::new());
        let response = overview_handler(State(state)).await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_routes_handler() {
        let state = Arc::new(AppState::new());
        let response = routes_handler(State(state)).await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
    }
}
