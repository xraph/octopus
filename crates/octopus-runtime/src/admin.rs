//! Admin dashboard integration
//!
//! Integrates the Askama-based admin dashboard into the Octopus runtime.

use bytes::Bytes;
use http::{Method, Response, StatusCode};
use http_body_util::Full;
use octopus_admin::{
    AnalyticsTemplate, AppState, ConfigTemplate, DashboardStats, HealthCheckInfo, HealthTemplate,
    LogsTemplate, OverviewTemplate, PluginInfo, PluginStatsCard, PluginsTemplate, RouteInfo,
    RoutesTemplate,
};
use octopus_core::{Error, Result};
use octopus_health::{CircuitBreaker, HealthTracker};
use octopus_metrics::{ActivityLog, MetricsCollector};
use octopus_plugin_runtime::PluginManager;
use octopus_router::Router;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tracing::debug;

/// Admin dashboard handler
#[derive(Clone)]
pub struct AdminHandler {
    router: Arc<Router>,
    request_count: Arc<AtomicUsize>,
    #[allow(dead_code)]
    app_state: Arc<AppState>,
    health_tracker: Option<Arc<HealthTracker>>,
    #[allow(dead_code)]
    circuit_breaker: Option<Arc<CircuitBreaker>>,
    plugin_manager: Option<Arc<PluginManager>>,
    metrics_collector: Option<Arc<MetricsCollector>>,
    #[allow(dead_code)]
    activity_log: Option<Arc<ActivityLog>>,
}

impl std::fmt::Debug for AdminHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdminHandler")
            .field("request_count", &self.request_count)
            .finish()
    }
}

impl AdminHandler {
    /// Create a new admin handler
    pub fn new(router: Arc<Router>, request_count: Arc<AtomicUsize>) -> Self {
        Self {
            router,
            request_count,
            app_state: Arc::new(AppState::new()),
            health_tracker: None,
            circuit_breaker: None,
            plugin_manager: None,
            metrics_collector: None,
            activity_log: None,
        }
    }

    /// Create a new admin handler with health monitoring
    pub fn with_health(
        router: Arc<Router>,
        request_count: Arc<AtomicUsize>,
        health_tracker: Arc<HealthTracker>,
        circuit_breaker: Arc<CircuitBreaker>,
    ) -> Self {
        Self {
            router,
            request_count,
            app_state: Arc::new(AppState::new()),
            health_tracker: Some(health_tracker),
            circuit_breaker: Some(circuit_breaker),
            plugin_manager: None,
            metrics_collector: None,
            activity_log: None,
        }
    }

    /// Create a new admin handler with all features
    pub fn with_all(
        router: Arc<Router>,
        request_count: Arc<AtomicUsize>,
        health_tracker: Option<Arc<HealthTracker>>,
        circuit_breaker: Option<Arc<CircuitBreaker>>,
        plugin_manager: Option<Arc<PluginManager>>,
        metrics_collector: Option<Arc<MetricsCollector>>,
        activity_log: Option<Arc<ActivityLog>>,
    ) -> Self {
        Self {
            router,
            request_count,
            app_state: Arc::new(AppState::new()),
            health_tracker,
            circuit_breaker,
            plugin_manager,
            metrics_collector,
            activity_log,
        }
    }

    /// Handle admin routes
    pub fn handle(&self, method: &Method, path: &str) -> Result<Response<Full<Bytes>>> {
        debug!(method = %method, path = %path, "Handling admin route");

        match (method, path) {
            // Main dashboard pages
            (&Method::GET, "/admin" | "/admin/") => self.overview_page(),
            (&Method::GET, "/admin/routes") => self.routes_page(),
            (&Method::GET, "/admin/health") => self.health_page(),
            (&Method::GET, "/admin/plugins") => self.plugins_page(),
            (&Method::GET, "/admin/analytics") => self.analytics_page(),
            (&Method::GET, "/admin/logs") => self.logs_page(),
            (&Method::GET, "/admin/config") => self.config_page(),

            // Prometheus metrics endpoint (supports both /__metrics and /metrics)
            (&Method::GET, "/metrics") | (&Method::GET, "/__metrics") => self.metrics_endpoint(),

            // API endpoints for HTMX and real-time data
            (&Method::GET, "/admin/api/stats") => self.api_stats(),
            (&Method::GET, "/admin/api/activity") => self.api_activity(),
            (&Method::GET, "/admin/api/health") => self.api_health_checks(),
            (&Method::GET, "/admin/api/routes") => self.api_routes_list(),
            (&Method::GET, "/admin/api/plugins") => self.api_plugins_list(),

            // New API endpoints for enhanced dashboard
            (&Method::GET, p) if p.starts_with("/admin/api/analytics") => self.api_analytics(),
            (&Method::GET, p) if p.starts_with("/admin/api/metrics/") => self.api_metrics(path),
            (&Method::GET, "/admin/api/logs") => self.api_logs(),
            (&Method::GET, "/admin/api/security/events") => self.api_security_events(),
            (&Method::GET, "/admin/api/config") => self.api_config_list(),
            (&Method::GET, "/admin/api/system/info") => self.api_system_info(),

            // Static files (CSS, JS, etc.)
            (&Method::GET, p) if p.starts_with("/admin/static/") => self.serve_static(path),

            // Astro UI (experimental alternative interface)
            (&Method::GET, p) if p.starts_with("/admin/ui") => self.serve_ui(path),

            // Fallback
            _ => self.not_found(),
        }
    }

    // Dashboard Pages

    fn overview_page(&self) -> Result<Response<Full<Bytes>>> {
        // Use metrics collector if available, otherwise fall back to basic counter
        let (total_requests, active_routes, avg_latency_ms) =
            if let Some(metrics) = &self.metrics_collector {
                (
                    metrics.total_requests(),
                    metrics.route_count(),
                    metrics.global_avg_latency_ms(),
                )
            } else {
                (
                    self.request_count.load(Ordering::Relaxed) as u64,
                    self.router.total_route_count(),
                    0.0,
                )
            };

        // Determine health status from metrics or health tracker
        let health_status = if let Some(metrics) = &self.metrics_collector {
            let error_rate = metrics.global_error_rate();
            if error_rate < 1.0 {
                "healthy"
            } else if error_rate < 5.0 {
                "warning"
            } else {
                "critical"
            }
        } else if let Some(health_tracker) = &self.health_tracker {
            let snapshots = health_tracker.get_all_snapshots();
            let all_healthy = snapshots
                .iter()
                .all(|(_, s)| s.error_rate < 0.5 && s.total_requests >= 10);
            if all_healthy {
                "healthy"
            } else {
                "degraded"
            }
        } else {
            "healthy"
        };

        // Get plugin stats cards
        let mut plugin_cards = vec![];
        if let Some(plugin_manager) = &self.plugin_manager {
            let stats = plugin_manager.stats();
            plugin_cards.push(PluginStatsCard {
                title: "Total Plugins".to_string(),
                value: stats.total.to_string(),
            });
            plugin_cards.push(PluginStatsCard {
                title: "Active Plugins".to_string(),
                value: stats.started.to_string(),
            });
            plugin_cards.push(PluginStatsCard {
                title: "Failed Plugins".to_string(),
                value: stats.failed.to_string(),
            });
        }

        let template = OverviewTemplate {
            total_requests,
            active_routes,
            avg_latency_ms,
            health_status: health_status.to_string(),
            plugin_cards,
        };

        self.render_template(template)
    }

    fn routes_page(&self) -> Result<Response<Full<Bytes>>> {
        // Get all routes from router
        let router_routes = self.router.get_all_routes();

        // Debug logging
        debug!(
            route_count = router_routes.len(),
            upstream_count = self.router.upstream_count(),
            "Loading routes for dashboard"
        );

        // Convert router routes to RouteInfo for display with real metrics
        let routes: Vec<RouteInfo> = router_routes
            .iter()
            .enumerate()
            .map(|(idx, route)| {
                let path = &route.path;

                // Get real request count from metrics if available
                let request_count = if let Some(metrics) = &self.metrics_collector {
                    metrics
                        .route_stats(path)
                        .map(|stats| stats.request_count.load(Ordering::Relaxed))
                        .unwrap_or(0)
                } else {
                    0
                };

                // Check upstream health from health tracker if available
                let is_healthy = if let Some(health_tracker) = &self.health_tracker {
                    // Check if any instance of this upstream is healthy
                    let snapshots = health_tracker.get_all_snapshots();
                    snapshots.iter().any(|(_, snapshot)| {
                        snapshot.error_rate < 0.5 && snapshot.total_requests >= 1
                    })
                } else {
                    true // Assume healthy if no health tracker
                };

                RouteInfo {
                    id: format!("route-{}", idx),
                    path: path.clone(),
                    method: route.method.to_string(),
                    upstream: route.upstream_name.clone(),
                    request_count,
                    is_healthy,
                    avg_latency_ms: if let Some(metrics) = &self.metrics_collector {
                        metrics
                            .route_stats(path)
                            .map(|stats| stats.avg_latency_ms())
                            .unwrap_or(0.0)
                    } else {
                        0.0
                    },
                    error_count: if let Some(metrics) = &self.metrics_collector {
                        metrics
                            .route_stats(path)
                            .map(|stats| stats.error_count.load(Ordering::Relaxed))
                            .unwrap_or(0)
                    } else {
                        0
                    },
                    last_accessed: Some(chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()),
                }
            })
            .collect();

        let total_routes = routes.len();
        let page_start = if total_routes > 0 { 1 } else { 0 };
        let page_end = total_routes;

        let template = RoutesTemplate {
            routes,
            page_start,
            page_end,
            total_routes,
        };

        self.render_template(template)
    }

    fn health_page(&self) -> Result<Response<Full<Bytes>>> {
        let mut health_checks = vec![];
        let mut overall_healthy = true;

        // Get health information from health tracker if available
        if let Some(health_tracker) = &self.health_tracker {
            let all_snapshots = health_tracker.get_all_snapshots();

            for (instance_id, snapshot) in &all_snapshots {
                // Instance is healthy if error rate is below 50% and has enough requests
                let is_healthy = snapshot.error_rate < 0.5 && snapshot.total_requests >= 10;
                let status = if is_healthy {
                    "passing"
                } else {
                    overall_healthy = false;
                    "failing"
                };

                health_checks.push(HealthCheckInfo {
                    name: instance_id.clone(),
                    status: status.to_string(),
                    response_time_ms: snapshot.avg_latency.as_millis() as u64,
                    message: Some(format!(
                        "Error rate: {:.2}%, Success rate: {:.2}%, Requests: {}",
                        snapshot.error_rate * 100.0,
                        snapshot.success_rate * 100.0,
                        snapshot.total_requests
                    )),
                    endpoint: None,
                    last_check: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                    consecutive_failures: snapshot.failed_requests as u32,
                });
            }
        }

        // Add gateway health check
        health_checks.insert(
            0,
            HealthCheckInfo {
                name: "Gateway".to_string(),
                status: "passing".to_string(),
                response_time_ms: 1,
                message: Some("All systems operational".to_string()),
                endpoint: None,
                last_check: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                consecutive_failures: 0,
            },
        );

        // Calculate uptime (mock for now, would need persistent storage)
        let uptime = if overall_healthy { 99.9 } else { 98.5 };

        let template = HealthTemplate {
            overall_status: if overall_healthy {
                "healthy".to_string()
            } else {
                "degraded".to_string()
            },
            last_check_time: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            health_checks,
            uptime_24h: uptime,
            uptime_7d: uptime - 0.1,
            uptime_30d: uptime - 0.2,
        };

        self.render_template(template)
    }

    fn plugins_page(&self) -> Result<Response<Full<Bytes>>> {
        let plugins = if let Some(plugin_manager) = &self.plugin_manager {
            let plugin_infos = plugin_manager.list();
            plugin_infos
                .into_iter()
                .map(|info| {
                    PluginInfo {
                        id: info.metadata.name.clone(),
                        name: info.metadata.name.clone(),
                        version: info.metadata.version.clone(),
                        description: info.metadata.description.clone(),
                        author: Some(info.metadata.author.clone()),
                        enabled: info.state.is_started(),
                        has_dashboard: false, // TODO: Check if plugin has dashboard
                        config: None,         // TODO: Get plugin configuration
                    }
                })
                .collect()
        } else {
            vec![]
        };

        let total_plugins = plugins.len();
        let active_plugins = plugins.iter().filter(|p| p.enabled).count();

        let template = PluginsTemplate {
            plugins: plugins.clone(),
            total_plugins,
            active_plugins,
            updates_available: 0, // TODO: Check for plugin updates
        };

        self.render_template(template)
    }

    fn analytics_page(&self) -> Result<Response<Full<Bytes>>> {
        let template = AnalyticsTemplate {
            timeframe: "24h".to_string(),
        };

        self.render_template(template)
    }

    fn logs_page(&self) -> Result<Response<Full<Bytes>>> {
        let template = LogsTemplate {
            total_logs: 0, // Will be loaded via API
        };

        self.render_template(template)
    }

    fn config_page(&self) -> Result<Response<Full<Bytes>>> {
        let template = ConfigTemplate {
            total_config: 0, // Will be loaded via API
        };

        self.render_template(template)
    }

    // API Endpoints

    fn api_stats(&self) -> Result<Response<Full<Bytes>>> {
        let total_requests = self.request_count.load(Ordering::Relaxed) as u64;
        let active_routes = self.router.total_route_count();

        let avg_latency_ms = if let Some(health_tracker) = &self.health_tracker {
            let snapshots = health_tracker.get_all_snapshots();
            if !snapshots.is_empty() {
                let total_latency: u128 = snapshots
                    .iter()
                    .map(|(_, s)| s.avg_latency.as_millis())
                    .sum();
                (total_latency / snapshots.len() as u128) as f64
            } else {
                0.0
            }
        } else {
            0.0
        };

        let health_status = if let Some(health_tracker) = &self.health_tracker {
            let snapshots = health_tracker.get_all_snapshots();
            let all_healthy = snapshots
                .iter()
                .all(|(_, s)| s.error_rate < 0.5 && s.total_requests >= 10);
            if all_healthy {
                "healthy"
            } else {
                "degraded"
            }
        } else {
            "healthy"
        };

        let stats = DashboardStats {
            total_requests,
            active_routes,
            avg_latency_ms,
            health_status: health_status.to_string(),
            requests_per_second: 0.0, // TODO: Calculate from metrics
            error_rate: if let Some(metrics) = &self.metrics_collector {
                metrics.global_error_rate() / 100.0 // Convert percentage to decimal
            } else {
                0.0
            },
            active_connections: 0, // TODO: Track active connections
            cpu_usage: 0.0,        // TODO: Get CPU usage from system metrics
            memory_usage: 0.0,     // TODO: Get memory usage from system metrics
        };

        let json = serde_json::to_string(&stats)
            .map_err(|e| Error::Internal(format!("Failed to serialize stats: {}", e)))?;

        Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .body(Full::new(Bytes::from(json)))
            .map_err(|e| Error::Internal(format!("Failed to build response: {}", e)))
    }

    fn api_activity(&self) -> Result<Response<Full<Bytes>>> {
        // TODO: Return real activity logs
        let html = r#"
            <div class="px-4 py-5 sm:p-6">
                <p class="text-sm text-gray-500 dark:text-gray-400">No recent activity</p>
            </div>
        "#;

        Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/html")
            .body(Full::new(Bytes::from(html)))
            .map_err(|e| Error::Internal(format!("Failed to build response: {}", e)))
    }

    fn api_health_checks(&self) -> Result<Response<Full<Bytes>>> {
        let mut checks = vec![];

        // Get health information from health tracker if available
        if let Some(health_tracker) = &self.health_tracker {
            let all_snapshots = health_tracker.get_all_snapshots();

            for (instance_id, snapshot) in &all_snapshots {
                // Instance is healthy if error rate is below 50% and has enough requests
                let is_healthy = snapshot.error_rate < 0.5 && snapshot.total_requests >= 10;
                let status = if is_healthy { "passing" } else { "failing" };

                checks.push(HealthCheckInfo {
                    name: instance_id.clone(),
                    status: status.to_string(),
                    response_time_ms: snapshot.avg_latency.as_millis() as u64,
                    message: Some(format!(
                        "Avg latency: {}ms, Error rate: {:.2}%, Success: {}/{} requests",
                        snapshot.avg_latency.as_millis(),
                        snapshot.error_rate * 100.0,
                        snapshot.successful_requests,
                        snapshot.total_requests
                    )),
                    endpoint: None,
                    last_check: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                    consecutive_failures: snapshot.failed_requests as u32,
                });
            }
        }

        // Add gateway health check
        checks.insert(
            0,
            HealthCheckInfo {
                name: "Gateway".to_string(),
                status: "passing".to_string(),
                response_time_ms: 1,
                message: Some("All systems operational".to_string()),
                endpoint: None,
                last_check: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                consecutive_failures: 0,
            },
        );

        let json = serde_json::to_string(&checks)
            .map_err(|e| Error::Internal(format!("Failed to serialize health checks: {}", e)))?;

        Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .body(Full::new(Bytes::from(json)))
            .map_err(|e| Error::Internal(format!("Failed to build response: {}", e)))
    }

    fn api_routes_list(&self) -> Result<Response<Full<Bytes>>> {
        let router_routes = self.router.get_all_routes();
        let routes: Vec<RouteInfo> = router_routes
            .iter()
            .enumerate()
            .map(|(idx, route)| RouteInfo {
                id: format!("route-{}", idx),
                path: route.path.clone(),
                method: route.method.to_string(),
                upstream: route.upstream_name.clone(),
                request_count: 0,
                is_healthy: true,
                avg_latency_ms: 0.0,
                error_count: 0,
                last_accessed: None,
            })
            .collect();

        let json = serde_json::to_string(&routes)
            .map_err(|e| Error::Internal(format!("Failed to serialize routes: {}", e)))?;

        Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .body(Full::new(Bytes::from(json)))
            .map_err(|e| Error::Internal(format!("Failed to build response: {}", e)))
    }

    fn api_plugins_list(&self) -> Result<Response<Full<Bytes>>> {
        let plugins = if let Some(plugin_manager) = &self.plugin_manager {
            let plugin_infos = plugin_manager.list();
            plugin_infos
                .into_iter()
                .map(|info| PluginInfo {
                    id: info.metadata.name.clone(),
                    name: info.metadata.name.clone(),
                    version: info.metadata.version.clone(),
                    description: info.metadata.description.clone(),
                    author: Some(info.metadata.author.clone()),
                    enabled: info.state.is_started(),
                    has_dashboard: false,
                    config: None,
                })
                .collect()
        } else {
            vec![]
        };

        let json = serde_json::to_string(&plugins)
            .map_err(|e| Error::Internal(format!("Failed to serialize plugins: {}", e)))?;

        Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .body(Full::new(Bytes::from(json)))
            .map_err(|e| Error::Internal(format!("Failed to build response: {}", e)))
    }

    // New Enhanced API Endpoints

    fn api_analytics(&self) -> Result<Response<Full<Bytes>>> {
        // Return mock analytics data
        // TODO: Return real analytics from metrics collector
        let json = r#"{
            "timeframe": "24h",
            "request_volume": [],
            "latency_percentiles": {"p50": 23.5, "p90": 67.8, "p95": 123.4, "p99": 456.7},
            "error_breakdown": {"4xx": 123, "5xx": 45, "timeout": 12},
            "top_routes": [],
            "status_code_distribution": {"200": 95432, "201": 3421, "400": 856, "500": 45},
            "traffic_by_method": {"GET": 75000, "POST": 18000, "PUT": 4500, "DELETE": 2500}
        }"#;

        Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .body(Full::new(Bytes::from(json)))
            .map_err(|e| Error::Internal(format!("Failed to build response: {}", e)))
    }

    fn api_metrics(&self, path: &str) -> Result<Response<Full<Bytes>>> {
        // Handle different metrics endpoints
        if path.contains("/realtime") {
            return self.api_stats();
        }

        // Return mock timeseries or performance data
        let json = r#"[]"#;

        Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .body(Full::new(Bytes::from(json)))
            .map_err(|e| Error::Internal(format!("Failed to build response: {}", e)))
    }

    fn api_logs(&self) -> Result<Response<Full<Bytes>>> {
        // Return mock logs
        // TODO: Return real logs from activity logger
        let json = r#"[
            {
                "timestamp": "2024-01-15 10:30:00",
                "level": "info",
                "message": "Route registered",
                "details": null,
                "source": "router"
            }
        ]"#;

        Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .body(Full::new(Bytes::from(json)))
            .map_err(|e| Error::Internal(format!("Failed to build response: {}", e)))
    }

    fn api_security_events(&self) -> Result<Response<Full<Bytes>>> {
        // Return mock security events
        let json = r#"[]"#;

        Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .body(Full::new(Bytes::from(json)))
            .map_err(|e| Error::Internal(format!("Failed to build response: {}", e)))
    }

    fn api_config_list(&self) -> Result<Response<Full<Bytes>>> {
        // Return mock configuration
        let json = r#"[
            {
                "key": "server.port",
                "value": 8080,
                "description": "Server listen port",
                "editable": true
            },
            {
                "key": "server.timeout_seconds",
                "value": 30,
                "description": "Request timeout in seconds",
                "editable": true
            },
            {
                "key": "logging.level",
                "value": "info",
                "description": "Logging level",
                "editable": true
            }
        ]"#;

        Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .body(Full::new(Bytes::from(json)))
            .map_err(|e| Error::Internal(format!("Failed to build response: {}", e)))
    }

    fn api_system_info(&self) -> Result<Response<Full<Bytes>>> {
        // Return system information
        let info = serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "uptime_seconds": 0,
            "start_time": chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            "hostname": "octopus-gateway",
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "num_cpus": num_cpus::get(),
            "total_memory": 0
        });

        let json = serde_json::to_string(&info)
            .map_err(|e| Error::Internal(format!("Failed to serialize system info: {}", e)))?;

        Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .body(Full::new(Bytes::from(json)))
            .map_err(|e| Error::Internal(format!("Failed to build response: {}", e)))
    }

    // Helper Methods

    fn render_template<T>(&self, template: T) -> Result<Response<Full<Bytes>>>
    where
        T: askama::Template,
    {
        let html = template
            .render()
            .map_err(|e| Error::Internal(format!("Failed to render template: {}", e)))?;

        Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/html; charset=utf-8")
            .header("cache-control", "no-cache")
            .body(Full::new(Bytes::from(html)))
            .map_err(|e| Error::Internal(format!("Failed to build response: {}", e)))
    }

    fn not_found(&self) -> Result<Response<Full<Bytes>>> {
        Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header("content-type", "text/plain")
            .body(Full::new(Bytes::from("Not found")))
            .map_err(|e| Error::Internal(format!("Failed to build response: {}", e)))
    }

    /// Serve static files (CSS, JS, etc.)
    fn serve_static(&self, path: &str) -> Result<Response<Full<Bytes>>> {
        // Strip the /admin/static/ prefix
        let file_path = path
            .strip_prefix("/admin/static/")
            .ok_or_else(|| Error::InvalidRequest("Invalid static file path".to_string()))?;

        // Prevent directory traversal attacks
        if file_path.contains("..") || file_path.starts_with("/") {
            return Err(Error::InvalidRequest("Invalid file path".to_string()));
        }

        debug!(requested_file = file_path, "Serving static file");

        // For now, we'll embed the output.css at compile time
        // In the future, you could extend this to serve other static assets
        match file_path {
            "output.css" => {
                // Embed the compiled CSS at compile time
                let css = include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../octopus-admin/static/output.css"
                ));

                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/css; charset=utf-8")
                    .header("cache-control", "public, max-age=31536000") // Cache for 1 year
                    .body(Full::new(Bytes::from(css)))
                    .map_err(|e| Error::Internal(format!("Failed to build response: {}", e)))
            }
            _ => {
                debug!(file = file_path, "Static file not found");
                self.not_found()
            }
        }
    }

    /// Serve Astro UI files with SPA fallback
    ///
    /// This implements robust SPA serving:
    /// - Serves actual files (JS, CSS, images) when they exist
    /// - Falls back to index.html for client-side routes
    /// - Handles directories by serving index.html
    fn serve_ui(&self, path: &str) -> Result<Response<Full<Bytes>>> {
        use std::path::Path;

        // Strip the /admin/ui prefix
        let relative_path = path
            .strip_prefix("/admin/ui")
            .unwrap_or("")
            .trim_start_matches('/');

        // Prevent directory traversal attacks
        if relative_path.contains("..") {
            return Err(Error::InvalidRequest("Invalid file path".to_string()));
        }

        debug!(
            requested_path = path,
            relative_path = relative_path,
            "Serving UI file"
        );

        // Determine what file to serve
        let file_to_serve = if relative_path.is_empty() {
            "index.html"
        } else {
            relative_path
        };

        // Build the actual file path
        let ui_dist = concat!(env!("CARGO_MANIFEST_DIR"), "/../octopus-admin/ui/dist/");
        let file_path = Path::new(ui_dist).join(file_to_serve);

        debug!(file_path = ?file_path, "Attempting to read file");

        // Try to read the file
        match std::fs::read(&file_path) {
            Ok(contents) => {
                // Determine content type based on extension
                let content_type = match file_path.extension().and_then(|e| e.to_str()) {
                    Some("html") => "text/html; charset=utf-8",
                    Some("css") => "text/css; charset=utf-8",
                    Some("js") => "application/javascript; charset=utf-8",
                    Some("json") => "application/json",
                    Some("svg") => "image/svg+xml",
                    Some("png") => "image/png",
                    Some("jpg") | Some("jpeg") => "image/jpeg",
                    Some("woff") => "font/woff",
                    Some("woff2") => "font/woff2",
                    _ => "application/octet-stream",
                };

                // Cache static assets but not HTML
                let cache_header = if content_type.contains("html") {
                    "no-cache"
                } else {
                    "public, max-age=31536000"
                };

                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", content_type)
                    .header("cache-control", cache_header)
                    .body(Full::new(Bytes::from(contents)))
                    .map_err(|e| Error::Internal(format!("Failed to build response: {}", e)))
            }
            Err(e) => {
                debug!(error = %e, "File not found, falling back to index.html");

                // Fall back to index.html for SPA client-side routing
                let index_path = Path::new(ui_dist).join("index.html");
                match std::fs::read(&index_path) {
                    Ok(contents) => Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "text/html; charset=utf-8")
                        .header("cache-control", "no-cache")
                        .body(Full::new(Bytes::from(contents)))
                        .map_err(|e| Error::Internal(format!("Failed to build response: {}", e))),
                    Err(e) => {
                        debug!(error = %e, "index.html not found");
                        self.not_found()
                    }
                }
            }
        }
    }

    /// Prometheus metrics endpoint
    fn metrics_endpoint(&self) -> Result<Response<Full<Bytes>>> {
        if let Some(ref metrics_collector) = self.metrics_collector {
            let metrics_text = octopus_metrics::PrometheusExporter::export(metrics_collector);

            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "text/plain; version=0.0.4; charset=utf-8")
                .body(Full::new(Bytes::from(metrics_text)))
                .unwrap())
        } else {
            // Return empty metrics if collector not available
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "text/plain; version=0.0.4; charset=utf-8")
                .body(Full::new(Bytes::from("# No metrics available\n")))
                .unwrap())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_admin_handler_creation() {
        let router = Arc::new(Router::new());
        let request_count = Arc::new(AtomicUsize::new(0));
        let _handler = AdminHandler::new(router, request_count);
    }
}
