//! HTTP handlers for the admin dashboard

use askama::Template;
use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    Json,
};
use std::sync::Arc;

use crate::models::{PluginStatsCard, RouteInfo, HealthCheckInfo, PluginInfo, DashboardStats, ActivityLogEntry};

/// Shared application state holding references to all real gateway data sources
#[derive(Clone)]
pub struct AppState {
    /// WebSocket hub for real-time dashboard events
    pub ws_hub: Arc<crate::websocket::WsHub>,
    /// Gateway router (routes, upstreams)
    pub router: Option<Arc<octopus_router::Router>>,
    /// Metrics collector (request counts, latency, errors)
    pub metrics: Option<Arc<octopus_metrics::MetricsCollector>>,
    /// Activity log (recent requests)
    pub activity_log: Option<Arc<octopus_metrics::ActivityLog>>,
    /// Health tracker (per-instance health)
    pub health_tracker: Option<Arc<octopus_health::HealthTracker>>,
    /// Circuit breaker (per-instance circuit state)
    pub circuit_breaker: Option<Arc<octopus_health::CircuitBreaker>>,
    /// Plugin manager
    pub plugin_manager: Option<Arc<octopus_plugins::PluginManager>>,
    /// Gateway configuration
    pub config: Option<Arc<octopus_config::Config>>,
    /// FARP schema registry for federated API discovery
    pub farp_registry: Option<Arc<octopus_farp::SchemaRegistry>>,
    /// FARP schema federation for merged OpenAPI output
    pub farp_federation: Option<Arc<octopus_farp::SchemaFederation>>,
    /// Server start time for uptime calculation
    pub start_time: std::time::Instant,
}

impl AppState {
    /// Create a new application state (minimal, for standalone use)
    #[must_use]
    pub fn new() -> Self {
        Self {
            ws_hub: Arc::new(crate::websocket::WsHub::new()),
            router: None,
            metrics: None,
            activity_log: None,
            health_tracker: None,
            circuit_breaker: None,
            plugin_manager: None,
            config: None,
            farp_registry: None,
            farp_federation: None,
            start_time: std::time::Instant::now(),
        }
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

/// Overview page template (Preline UI with `ApexCharts`)
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
pub async fn overview_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let total_requests = state.metrics.as_ref().map_or(0, |m| m.total_requests());
    let active_routes = state.router.as_ref().map_or(0, |r| r.total_route_count());
    let avg_latency_ms = state.metrics.as_ref().map_or(0.0, |m| m.global_avg_latency_ms());

    let health_status = if let Some(ref ht) = state.health_tracker {
        let snapshots = ht.get_all_snapshots();
        if snapshots.is_empty() {
            "healthy".to_string()
        } else {
            let any_unhealthy = snapshots.iter().any(|(_, s)| s.error_rate > 0.5);
            if any_unhealthy { "degraded".to_string() } else { "healthy".to_string() }
        }
    } else {
        "unknown".to_string()
    };

    let template = OverviewTemplate {
        total_requests,
        active_routes,
        avg_latency_ms,
        health_status,
        plugin_cards: vec![],
    };

    HtmlTemplate(template)
}

/// Routes page handler
pub async fn routes_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let routes = build_routes_from_state(&state);
    let total = routes.len();

    let template = RoutesTemplate {
        routes,
        page_start: if total > 0 { 1 } else { 0 },
        page_end: total,
        total_routes: total,
    };

    HtmlTemplate(template)
}

/// Health page handler
pub async fn health_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let health_checks = build_health_from_state(&state);

    let overall_status = if health_checks.is_empty() {
        "unknown".to_string()
    } else {
        let critical = health_checks.iter().any(|h| h.status == "critical");
        let warning = health_checks.iter().any(|h| h.status == "warning");
        if critical { "critical".to_string() }
        else if warning { "warning".to_string() }
        else { "healthy".to_string() }
    };

    let uptime_secs = state.start_time.elapsed().as_secs();
    let uptime_pct = if uptime_secs > 0 {
        let error_rate = state.metrics.as_ref().map_or(0.0, |m| m.global_error_rate());
        (1.0 - error_rate) * 100.0
    } else {
        100.0
    };

    let template = HealthTemplate {
        overall_status,
        last_check_time: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        health_checks,
        uptime_24h: uptime_pct,
        uptime_7d: uptime_pct,
        uptime_30d: uptime_pct,
    };

    HtmlTemplate(template)
}

/// Plugins page handler
pub async fn plugins_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let plugins = build_plugins_from_state(&state).await;
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
pub async fn api_stats_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let stats = build_dashboard_stats(&state);
    Json(stats)
}

/// API: Get recent activity (for HTMX auto-refresh)
pub async fn api_activity_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let activities = if let Some(ref log) = state.activity_log {
        log.recent_entries(50)
            .into_iter()
            .map(|e| ActivityLogEntry {
                timestamp: e.formatted_time(),
                level: if e.is_error() { "error".to_string() } else { "info".to_string() },
                message: format!("{} {} → {}", e.method, e.path, e.status),
                details: Some(format!("{:.1}ms via {}", e.latency_ms, e.upstream)),
                source: Some("proxy".to_string()),
            })
            .collect()
    } else {
        vec![]
    };

    Json(activities)
}

/// API: Get health checks (for HTMX auto-refresh)
pub async fn api_health_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let health_checks = build_health_from_state(&state);
    Json(health_checks)
}

// ============================================================================
// Helper functions to extract real data from AppState
// ============================================================================

/// Build DashboardStats from real metrics
pub(crate) fn build_dashboard_stats(state: &AppState) -> DashboardStats {
    let (total_requests, total_errors, active_connections, avg_latency, uptime) =
        if let Some(ref m) = state.metrics {
            (
                m.total_requests(),
                m.total_errors(),
                m.active_connections() as u64,
                m.global_avg_latency_ms(),
                m.uptime_seconds(),
            )
        } else {
            (0, 0, 0, 0.0, state.start_time.elapsed().as_secs())
        };

    let active_routes = state.router.as_ref().map_or(0, |r| r.total_route_count());
    let error_rate = if total_requests > 0 {
        total_errors as f64 / total_requests as f64
    } else {
        0.0
    };
    let rps = if uptime > 0 {
        total_requests as f64 / uptime as f64
    } else {
        0.0
    };

    let health_status = if let Some(ref ht) = state.health_tracker {
        let snapshots = ht.get_all_snapshots();
        if snapshots.is_empty() {
            "healthy".to_string()
        } else {
            let any_critical = snapshots.iter().any(|(_, s)| s.error_rate > 0.5);
            if any_critical { "degraded".to_string() } else { "healthy".to_string() }
        }
    } else {
        "unknown".to_string()
    };

    DashboardStats {
        total_requests,
        active_routes,
        avg_latency_ms: avg_latency,
        health_status,
        requests_per_second: rps,
        error_rate,
        active_connections,
        cpu_usage: 0.0,    // System metrics not yet collected
        memory_usage: 0.0,  // System metrics not yet collected
    }
}

/// Build route list from router + metrics
pub(crate) fn build_routes_from_state(state: &AppState) -> Vec<RouteInfo> {
    let Some(ref router) = state.router else { return vec![] };

    router
        .get_all_routes()
        .into_iter()
        .enumerate()
        .map(|(i, route)| {
            let route_key = format!("{} {}", route.method.as_str(), route.path);

            // Look up per-route metrics
            let (request_count, error_count, avg_latency_ms) =
                if let Some(ref m) = state.metrics {
                    if let Some(stats) = m.route_stats(&route_key) {
                        (
                            stats.request_count.load(std::sync::atomic::Ordering::Relaxed),
                            stats.error_count.load(std::sync::atomic::Ordering::Relaxed),
                            stats.avg_latency_ms(),
                        )
                    } else if let Some(stats) = m.route_stats(&route.path) {
                        (
                            stats.request_count.load(std::sync::atomic::Ordering::Relaxed),
                            stats.error_count.load(std::sync::atomic::Ordering::Relaxed),
                            stats.avg_latency_ms(),
                        )
                    } else {
                        (0, 0, 0.0)
                    }
                } else {
                    (0, 0, 0.0)
                };

            // Look up health from health tracker using upstream name
            let is_healthy = state
                .health_tracker
                .as_ref()
                .map_or(true, |ht| ht.is_healthy(&route.upstream_name, 0.5));

            RouteInfo {
                id: format!("route-{i}"),
                path: route.path.clone(),
                method: route.method.to_string(),
                upstream: route.upstream_name.clone(),
                request_count,
                is_healthy,
                avg_latency_ms,
                error_count,
                last_accessed: None,
            }
        })
        .collect()
}

/// Build health checks from health tracker + upstreams
pub(crate) fn build_health_from_state(state: &AppState) -> Vec<HealthCheckInfo> {
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

    if let Some(ref ht) = state.health_tracker {
        let snapshots = ht.get_all_snapshots();
        if !snapshots.is_empty() {
            return snapshots
                .into_iter()
                .map(|(id, snap)| {
                    let status = if snap.error_rate > 0.5 {
                        "critical"
                    } else if snap.error_rate > 0.1 {
                        "warning"
                    } else {
                        "passing"
                    };

                    let consecutive_failures = snap.failed_requests as u32;

                    HealthCheckInfo {
                        name: id.clone(),
                        status: status.to_string(),
                        response_time_ms: snap.avg_latency.as_millis() as u64,
                        message: if status == "critical" {
                            Some(format!("Error rate: {:.1}%", snap.error_rate * 100.0))
                        } else {
                            None
                        },
                        endpoint: Some(id),
                        last_check: now.clone(),
                        consecutive_failures,
                    }
                })
                .collect();
        }
    }

    // Fall back to upstream instances if no health tracker data
    if let Some(ref router) = state.router {
        let mut checks = Vec::new();
        for cluster in router.get_all_upstreams() {
            for inst in &cluster.instances {
                checks.push(HealthCheckInfo {
                    name: format!("{}/{}", cluster.name, inst.id),
                    status: if inst.is_healthy() { "passing" } else { "critical" }.to_string(),
                    response_time_ms: 0,
                    message: if !inst.is_healthy() {
                        Some("Instance marked unhealthy".to_string())
                    } else {
                        None
                    },
                    endpoint: Some(inst.base_url()),
                    last_check: now.clone(),
                    consecutive_failures: if inst.is_healthy() { 0 } else { 1 },
                });
            }
        }
        return checks;
    }

    vec![]
}

/// Build plugin list from plugin manager
pub(crate) async fn build_plugins_from_state(state: &AppState) -> Vec<PluginInfo> {
    let Some(ref pm) = state.plugin_manager else { return vec![] };

    let names = pm.list_plugins().await;
    let mut plugins = Vec::with_capacity(names.len());

    for name in &names {
        if let Some(handle) = pm.registry().get(name).await {
            let guard = handle.read().await;
            let meta = guard.metadata();
            plugins.push(PluginInfo {
                id: meta.name.clone(),
                name: meta.name.clone(),
                version: meta.version.clone(),
                description: meta.description.clone(),
                author: if meta.author.is_empty() { None } else { Some(meta.author.clone()) },
                enabled: true, // If it's registered, it's enabled
                has_dashboard: false,
                config: None,
            });
        }
    }

    plugins
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
