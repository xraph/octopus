//! Handlers demonstrating octopus-ui integration

use askama::Template;
use axum::{extract::State, response::IntoResponse};
use std::sync::Arc;

use crate::{
    handlers::{AppState, HtmlTemplate},
    models::{HealthCheckInfo, PluginInfo, RouteInfo},
    ui_components,
};

/// Modern dashboard template using octopus-ui components
#[derive(Template)]
#[template(path = "octopus_ui_dashboard.html")]
pub struct OctopusUiDashboardTemplate {
    pub stats_cards: String,
    pub routes_table: String,
    pub health_checks: String,
    pub plugins_grid: String,
}

/// Modern routes page using octopus-ui components
#[derive(Template)]
#[template(path = "octopus_ui_routes.html")]
pub struct OctopusUiRoutesTemplate {
    pub routes_table: String,
    pub routes_grid: String,
    pub total_routes: usize,
}

/// Modern health page using octopus-ui components
#[derive(Template)]
#[template(path = "octopus_ui_health.html")]
pub struct OctopusUiHealthTemplate {
    pub health_checks: String,
    pub overall_status: String,
    pub uptime_24h: f64,
    pub uptime_7d: f64,
    pub uptime_30d: f64,
}

/// Modern plugins page using octopus-ui components
#[derive(Template)]
#[template(path = "octopus_ui_plugins.html")]
pub struct OctopusUiPluginsTemplate {
    pub plugins_grid: String,
    pub total_plugins: usize,
    pub active_plugins: usize,
}

/// Handler for modern dashboard using octopus-ui
pub async fn octopus_ui_dashboard_handler(
    State(_state): State<Arc<AppState>>,
) -> impl IntoResponse {
    // Generate stats cards
    let stats_cards = format!(
        r#"<div class="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
            {}
            {}
            {}
            {}
        </div>"#,
        ui_components::stats_card(
            "Total Requests",
            "1,234,567",
            "+20.1% from last month",
            Some(
                r#"<svg class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13 7h8m0 0v8m0-8l-8 8-4-4-6 6" />
                </svg>"#
            )
        ),
        ui_components::stats_card(
            "Active Routes",
            "42",
            "+3 since last week",
            Some(
                r#"<svg class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2" />
                </svg>"#
            )
        ),
        ui_components::stats_card(
            "Avg Latency",
            "45.6ms",
            "-5.2ms from yesterday",
            Some(
                r#"<svg class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z" />
                </svg>"#
            )
        ),
        ui_components::stats_card(
            "Health Status",
            "Healthy",
            "All systems operational",
            Some(
                r#"<svg class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
                </svg>"#
            )
        ),
    );

    // Sample data
    let routes = vec![
        RouteInfo {
            id: "route1".to_string(),
            path: "/api/users".to_string(),
            method: "GET".to_string(),
            upstream: "user-service:8080".to_string(),
            request_count: 45678,
            is_healthy: true,
            avg_latency_ms: 23.5,
            error_count: 12,
            last_accessed: Some("2024-01-15 10:30:00".to_string()),
        },
        RouteInfo {
            id: "route2".to_string(),
            path: "/api/products".to_string(),
            method: "POST".to_string(),
            upstream: "product-service:8080".to_string(),
            request_count: 23456,
            is_healthy: true,
            avg_latency_ms: 34.2,
            error_count: 8,
            last_accessed: Some("2024-01-15 10:25:00".to_string()),
        },
        RouteInfo {
            id: "route3".to_string(),
            path: "/api/orders".to_string(),
            method: "GET".to_string(),
            upstream: "order-service:8080".to_string(),
            request_count: 34567,
            is_healthy: true,
            avg_latency_ms: 28.7,
            error_count: 5,
            last_accessed: Some("2024-01-15 10:28:00".to_string()),
        },
    ];

    let health_checks = vec![
        HealthCheckInfo {
            name: "PostgreSQL Database".to_string(),
            status: "passing".to_string(),
            response_time_ms: 12,
            message: Some("Connected successfully".to_string()),
            endpoint: Some("postgresql://localhost:5432/octopus".to_string()),
            last_check: "2024-01-15 10:30:00".to_string(),
            consecutive_failures: 0,
        },
        HealthCheckInfo {
            name: "Redis Cache".to_string(),
            status: "passing".to_string(),
            response_time_ms: 5,
            message: Some("Cache operational".to_string()),
            endpoint: Some("redis://localhost:6379".to_string()),
            last_check: "2024-01-15 10:30:00".to_string(),
            consecutive_failures: 0,
        },
    ];

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

    // Generate routes table
    let routes_table = ui_components::routes_table(&routes);

    // Generate health checks
    let health_checks_html = health_checks
        .iter()
        .map(ui_components::health_check_card)
        .collect::<Vec<_>>()
        .join("\n");

    let health_checks_grid =
        format!(r#"<div class="grid gap-4 md:grid-cols-2">{health_checks_html}</div>"#);

    // Generate plugins grid
    let plugins_html = plugins
        .iter()
        .map(ui_components::plugin_card)
        .collect::<Vec<_>>()
        .join("\n");

    let plugins_grid =
        format!(r#"<div class="grid gap-4 md:grid-cols-2 lg:grid-cols-3">{plugins_html}</div>"#);

    let template = OctopusUiDashboardTemplate {
        stats_cards,
        routes_table,
        health_checks: health_checks_grid,
        plugins_grid,
    };

    HtmlTemplate(template)
}

/// Handler for routes page using octopus-ui
pub async fn octopus_ui_routes_handler(State(_state): State<Arc<AppState>>) -> impl IntoResponse {
    let routes = vec![
        RouteInfo {
            id: "route1".to_string(),
            path: "/api/users".to_string(),
            method: "GET".to_string(),
            upstream: "user-service:8080".to_string(),
            request_count: 45678,
            is_healthy: true,
            avg_latency_ms: 23.5,
            error_count: 12,
            last_accessed: Some("2024-01-15 10:30:00".to_string()),
        },
        RouteInfo {
            id: "route2".to_string(),
            path: "/api/products".to_string(),
            method: "POST".to_string(),
            upstream: "product-service:8080".to_string(),
            request_count: 23456,
            is_healthy: true,
            avg_latency_ms: 34.2,
            error_count: 8,
            last_accessed: Some("2024-01-15 10:25:00".to_string()),
        },
    ];

    let routes_table = ui_components::routes_table(&routes);

    let routes_grid = routes
        .iter()
        .map(ui_components::route_card)
        .collect::<Vec<_>>()
        .join("\n");

    let routes_grid = format!(r#"<div class="grid gap-4 md:grid-cols-2">{routes_grid}</div>"#);

    let template = OctopusUiRoutesTemplate {
        routes_table,
        routes_grid,
        total_routes: routes.len(),
    };

    HtmlTemplate(template)
}

/// Handler for health page using octopus-ui
pub async fn octopus_ui_health_handler(State(_state): State<Arc<AppState>>) -> impl IntoResponse {
    let health_checks = vec![
        HealthCheckInfo {
            name: "PostgreSQL Database".to_string(),
            status: "passing".to_string(),
            response_time_ms: 12,
            message: Some("Connected successfully".to_string()),
            endpoint: Some("postgresql://localhost:5432/octopus".to_string()),
            last_check: "2024-01-15 10:30:00".to_string(),
            consecutive_failures: 0,
        },
        HealthCheckInfo {
            name: "Redis Cache".to_string(),
            status: "passing".to_string(),
            response_time_ms: 5,
            message: Some("Cache operational".to_string()),
            endpoint: Some("redis://localhost:6379".to_string()),
            last_check: "2024-01-15 10:30:00".to_string(),
            consecutive_failures: 0,
        },
    ];

    let health_checks_html = health_checks
        .iter()
        .map(ui_components::health_check_card)
        .collect::<Vec<_>>()
        .join("\n");

    let health_checks_grid =
        format!(r#"<div class="grid gap-4 md:grid-cols-2">{health_checks_html}</div>"#);

    let template = OctopusUiHealthTemplate {
        health_checks: health_checks_grid,
        overall_status: ui_components::health_status_badge("passing"),
        uptime_24h: 99.9,
        uptime_7d: 99.8,
        uptime_30d: 99.7,
    };

    HtmlTemplate(template)
}

/// Handler for plugins page using octopus-ui
pub async fn octopus_ui_plugins_handler(State(_state): State<Arc<AppState>>) -> impl IntoResponse {
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
        PluginInfo {
            id: "cache-redis".to_string(),
            name: "Redis Cache".to_string(),
            version: "0.1.0".to_string(),
            description: "Response caching with Redis backend".to_string(),
            author: Some("Octopus Team".to_string()),
            enabled: false,
            has_dashboard: false,
            config: None,
        },
    ];

    let plugins_html = plugins
        .iter()
        .map(ui_components::plugin_card)
        .collect::<Vec<_>>()
        .join("\n");

    let plugins_grid =
        format!(r#"<div class="grid gap-4 md:grid-cols-2 lg:grid-cols-3">{plugins_html}</div>"#);

    let active_plugins = plugins.iter().filter(|p| p.enabled).count();

    let template = OctopusUiPluginsTemplate {
        plugins_grid,
        total_plugins: plugins.len(),
        active_plugins,
    };

    HtmlTemplate(template)
}
