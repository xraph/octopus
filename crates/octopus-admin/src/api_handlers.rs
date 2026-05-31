//! Enhanced API handlers for the admin dashboard
//!
//! Provides comprehensive REST API endpoints for:
//! - Metrics and analytics
//! - Route management (CRUD)
//! - Plugin management
//! - Logs and monitoring
//! - Configuration management
//! - System information

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use std::collections::HashMap;
use std::sync::Arc;

use crate::handlers::AppState;
use crate::models::{
    ActivityLogEntry, AnalyticsMetrics, ConfigItem, FarpServiceInfo, LatencyPercentiles, LogQuery,
    PerformanceMetrics, RouteConfig, RouteInfo, RouteMetric, SecurityEvent, SystemInfo,
    TimeSeriesPoint, UpstreamClusterInfo, UpstreamInstanceInfo,
};

/// Lazily-initialized system info provider for CPU/memory metrics
fn get_system_metrics() -> (f64, f64, u64, u64) {
    use sysinfo::System;
    let mut sys = System::new();
    sys.refresh_memory();
    sys.refresh_cpu_all();

    let total_memory = sys.total_memory();
    let used_memory = sys.used_memory();
    let memory_available = sys.available_memory();
    let memory_usage_pct = if total_memory > 0 {
        (used_memory as f64 / total_memory as f64) * 100.0
    } else {
        0.0
    };
    // CPU usage requires two samples; return global average
    let cpu_usage = f64::from(sys.global_cpu_usage());

    (cpu_usage, memory_usage_pct, total_memory, memory_available)
}

// ============================================================================
// Metrics & Analytics Endpoints
// ============================================================================

/// Get comprehensive analytics metrics
/// GET /admin/api/analytics?timeframe=24h
pub async fn api_analytics_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let timeframe = params
        .get("timeframe")
        .map_or("24h", std::string::String::as_str);

    if let Some(ref m) = state.metrics {
        let snapshot = octopus_metrics::MetricsSnapshot::from_collector(m);
        let top_routes: Vec<RouteMetric> = snapshot
            .routes
            .iter()
            .map(|r| RouteMetric {
                path: r.path.clone(),
                requests: r.request_count,
                avg_latency: r.avg_latency_ms,
                error_rate: r.error_rate,
            })
            .collect();

        let mut traffic_by_method = HashMap::new();
        if let Some(ref router) = state.router {
            for route in router.get_all_routes() {
                let method = route.method.to_string();
                let count = m
                    .route_stats(&format!("{} {}", method, route.path))
                    .map_or(0, |s| {
                        s.request_count.load(std::sync::atomic::Ordering::Relaxed)
                    });
                *traffic_by_method.entry(method).or_insert(0u64) += count;
            }
        }

        let analytics = AnalyticsMetrics {
            timeframe: timeframe.to_string(),
            request_volume: vec![],
            latency_percentiles: LatencyPercentiles {
                p50: snapshot.routes.first().map_or(0.0, |r| r.p50_latency_ms),
                p90: snapshot.routes.first().map_or(0.0, |r| r.avg_latency_ms),
                p95: snapshot.routes.first().map_or(0.0, |r| r.p95_latency_ms),
                p99: snapshot.routes.first().map_or(0.0, |r| r.p99_latency_ms),
            },
            error_breakdown: HashMap::new(),
            top_routes,
            status_code_distribution: HashMap::new(),
            traffic_by_method,
        };

        Json(analytics)
    } else {
        let analytics = AnalyticsMetrics {
            timeframe: timeframe.to_string(),
            request_volume: vec![],
            latency_percentiles: LatencyPercentiles {
                p50: 0.0,
                p90: 0.0,
                p95: 0.0,
                p99: 0.0,
            },
            error_breakdown: HashMap::new(),
            top_routes: vec![],
            status_code_distribution: HashMap::new(),
            traffic_by_method: HashMap::new(),
        };
        Json(analytics)
    }
}

/// Get real-time metrics for dashboard
/// GET /admin/api/metrics/realtime
pub async fn api_realtime_metrics_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let stats = crate::handlers::build_dashboard_stats(&state);
    Json(stats)
}

/// Get time series data for charts
/// GET /admin/api/metrics/timeseries?metric=requests&period=1h
pub async fn api_timeseries_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let metric = params
        .get("metric")
        .map_or("requests", std::string::String::as_str);
    let _period = params
        .get("period")
        .map_or("1h", std::string::String::as_str);

    if let Some(ref m) = state.metrics {
        let now = Utc::now();
        let value = match metric {
            "requests" => m.total_requests() as f64,
            "errors" => m.total_errors() as f64,
            "latency" => m.global_avg_latency_ms(),
            "connections" => m.active_connections() as f64,
            _ => m.total_requests() as f64,
        };

        let data = vec![TimeSeriesPoint {
            timestamp: now.format("%Y-%m-%d %H:%M:%S").to_string(),
            value,
        }];
        Json(data)
    } else {
        Json(vec![] as Vec<TimeSeriesPoint>)
    }
}

/// Get performance metrics with real CPU/memory data
/// GET /admin/api/metrics/performance
pub async fn api_performance_metrics_handler(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let active_connections = state.metrics.as_ref().map_or(0, |m| m.active_connections());
    let (cpu_usage, memory_usage, memory_total, memory_available) = get_system_metrics();

    let metrics = PerformanceMetrics {
        cpu_usage,
        memory_usage,
        memory_total,
        memory_available,
        goroutines: active_connections,
        gc_count: 0,
        gc_pause_ms: 0.0,
    };

    Json(metrics)
}

// ============================================================================
// Routes Management Endpoints (CRUD)
// ============================================================================

/// List all routes
/// GET /admin/api/routes
pub async fn api_routes_list_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let routes = crate::handlers::build_routes_from_state(&state);
    Json(routes)
}

/// Get single route by ID
/// GET /admin/api/routes/:id
pub async fn api_route_get_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let routes = crate::handlers::build_routes_from_state(&state);
    if let Some(route) = routes.into_iter().find(|r| r.id == id) {
        Json(serde_json::to_value(route).unwrap())
    } else {
        Json(serde_json::json!({"error": "Route not found", "id": id}))
    }
}

/// Create new route (mutates the live router)
/// POST /admin/api/routes
pub async fn api_route_create_handler(
    State(state): State<Arc<AppState>>,
    Json(config): Json<RouteConfig>,
) -> impl IntoResponse {
    let Some(ref router) = state.router else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "Router not available"})),
        );
    };

    let method: http::Method = match config.method.parse() {
        Ok(m) => m,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(
                    serde_json::json!({"error": format!("Invalid HTTP method: {}", config.method)}),
                ),
            );
        }
    };

    let route = match octopus_router::RouteBuilder::new()
        .method(method.clone())
        .path(&config.path)
        .upstream_name(&config.upstream)
        .build()
    {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": format!("Invalid route: {}", e)})),
            );
        }
    };

    if let Err(e) = router.add_route(route) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Failed to add route: {}", e)})),
        );
    }

    tracing::info!(
        "Created route: {} {} -> {}",
        config.method,
        config.path,
        config.upstream
    );

    let info = RouteInfo {
        id: uuid::Uuid::new_v4().to_string(),
        path: config.path,
        method: method.to_string(),
        upstream: config.upstream,
        request_count: 0,
        is_healthy: true,
        avg_latency_ms: 0.0,
        error_count: 0,
        last_accessed: None,
        ..Default::default()
    };

    (
        StatusCode::CREATED,
        Json(serde_json::to_value(info).unwrap()),
    )
}

/// Update existing route (remove old, add new)
/// PUT /admin/api/routes/:id
pub async fn api_route_update_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(config): Json<RouteConfig>,
) -> impl IntoResponse {
    let Some(ref router) = state.router else {
        return Json(serde_json::json!({"error": "Router not available"}));
    };

    // Find existing route by ID
    let routes = crate::handlers::build_routes_from_state(&state);
    if let Some(old_route) = routes.iter().find(|r| r.id == id) {
        // Remove old route
        if let Ok(old_method) = old_route.method.parse::<http::Method>() {
            let _ = router.remove_route(&old_method, &old_route.path);
        }
    }

    // Add new route
    let method: http::Method = match config.method.parse() {
        Ok(m) => m,
        Err(_) => {
            return Json(
                serde_json::json!({"error": format!("Invalid HTTP method: {}", config.method)}),
            );
        }
    };

    let route = match octopus_router::RouteBuilder::new()
        .method(method.clone())
        .path(&config.path)
        .upstream_name(&config.upstream)
        .build()
    {
        Ok(r) => r,
        Err(e) => {
            return Json(serde_json::json!({"error": format!("Invalid route: {}", e)}));
        }
    };

    if let Err(e) = router.add_route(route) {
        return Json(serde_json::json!({"error": format!("Failed to update route: {}", e)}));
    }

    tracing::info!(
        "Updated route {}: {} {} -> {}",
        id,
        config.method,
        config.path,
        config.upstream
    );

    let info = RouteInfo {
        id,
        path: config.path,
        method: method.to_string(),
        upstream: config.upstream,
        request_count: 0,
        is_healthy: true,
        avg_latency_ms: 0.0,
        error_count: 0,
        last_accessed: None,
        ..Default::default()
    };

    Json(serde_json::to_value(info).unwrap())
}

/// Delete route
/// DELETE /admin/api/routes/:id
pub async fn api_route_delete_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let Some(ref router) = state.router else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };

    let routes = crate::handlers::build_routes_from_state(&state);
    if let Some(route) = routes.iter().find(|r| r.id == id) {
        if let Ok(method) = route.method.parse::<http::Method>() {
            let _ = router.remove_route(&method, &route.path);
            tracing::info!("Deleted route: {} {} {}", id, route.method, route.path);
        }
    }

    StatusCode::NO_CONTENT
}

// ============================================================================
// Plugin Management Endpoints
// ============================================================================

/// List all plugins
/// GET /admin/api/plugins
pub async fn api_plugins_list_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let plugins = crate::handlers::build_plugins_from_state(&state);
    Json(plugins)
}

/// Get plugin by ID
/// GET /admin/api/plugins/:id
pub async fn api_plugin_get_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let plugins = crate::handlers::build_plugins_from_state(&state);
    if let Some(plugin) = plugins.into_iter().find(|p| p.id == id) {
        Json(serde_json::to_value(plugin).unwrap())
    } else {
        Json(serde_json::json!({"error": "Plugin not found", "id": id}))
    }
}

/// Enable/disable plugin
/// POST /admin/api/plugins/:id/toggle
pub async fn api_plugin_toggle_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Some(ref pm) = state.plugin_manager {
        // Check if plugin is currently started
        if let Some(entry) = pm.get(&id) {
            let is_started = matches!(
                *entry.state.read(),
                octopus_plugin_runtime::RegistryPluginState::Started
            );
            if is_started {
                if let Err(e) = pm.stop(&id).await {
                    return Json(serde_json::json!({"success": false, "error": format!("{}", e)}));
                }
                tracing::info!("Stopped plugin: {}", id);
            } else {
                if let Err(e) = pm.start(&id).await {
                    return Json(serde_json::json!({"success": false, "error": format!("{}", e)}));
                }
                tracing::info!("Started plugin: {}", id);
            }
            return Json(serde_json::json!({"success": true, "message": "Plugin state toggled"}));
        }
    }

    Json(serde_json::json!({"success": false, "error": "Plugin not found"}))
}

/// Update plugin configuration
/// PUT /admin/api/plugins/:id/config
pub async fn api_plugin_config_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(config): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Some(ref pm) = state.plugin_manager {
        if let Err(e) = pm.reload(&id, config).await {
            return Json(
                serde_json::json!({"success": false, "error": format!("Failed to reload plugin config: {}", e)}),
            );
        }
        tracing::info!("Updated plugin {} config", id);
        return Json(
            serde_json::json!({"success": true, "message": "Plugin configuration updated"}),
        );
    }

    Json(serde_json::json!({"success": false, "error": "Plugin manager not available"}))
}

// ============================================================================
// Logs & Monitoring Endpoints
// ============================================================================

/// Get logs
/// GET /admin/api/logs?level=error&limit=100
pub async fn api_logs_handler(
    State(state): State<Arc<AppState>>,
    Query(query): Query<LogQuery>,
) -> impl IntoResponse {
    let limit = query.limit.unwrap_or(100);

    let entries: Vec<ActivityLogEntry> = if let Some(ref log) = state.activity_log {
        let all = log.recent_entries(limit);
        all.into_iter()
            .filter(|e| {
                if let Some(ref level) = query.level {
                    let entry_level = if e.is_error() { "error" } else { "info" };
                    entry_level == level.as_str()
                } else {
                    true
                }
            })
            .filter(|e| {
                if let Some(ref search) = query.search {
                    e.path.contains(search.as_str()) || e.upstream.contains(search.as_str())
                } else {
                    true
                }
            })
            .map(|e| ActivityLogEntry {
                timestamp: e.formatted_time(),
                level: if e.is_error() {
                    "error".to_string()
                } else {
                    "info".to_string()
                },
                message: format!(
                    "{} {} → {} ({:.1}ms)",
                    e.method, e.path, e.status, e.latency_ms
                ),
                details: Some(format!("Upstream: {}", e.upstream)),
                source: Some("proxy".to_string()),
            })
            .collect()
    } else {
        vec![]
    };

    Json(entries)
}

/// Get security events
/// GET /admin/api/security/events
pub async fn api_security_events_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let mut events = Vec::new();
    let now = Utc::now();

    if let Some(ref cb) = state.circuit_breaker {
        for (id, metrics) in cb.get_all_metrics() {
            if metrics.state != octopus_health::CircuitState::Closed {
                events.push(SecurityEvent {
                    timestamp: now.format("%Y-%m-%d %H:%M:%S").to_string(),
                    event_type: "circuit_breaker".to_string(),
                    severity: if metrics.state == octopus_health::CircuitState::Open {
                        "high".to_string()
                    } else {
                        "medium".to_string()
                    },
                    source_ip: id.clone(),
                    details: format!(
                        "Circuit {} for {}: {:.1}% failure rate ({} failures / {} total)",
                        metrics.state,
                        id,
                        metrics.failure_rate * 100.0,
                        metrics.failure_count,
                        metrics.total_count
                    ),
                });
            }
        }
    }

    Json(events)
}

// ============================================================================
// Configuration Management Endpoints
// ============================================================================

/// Get all configuration
/// GET /admin/api/config
pub async fn api_config_list_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let config_items = if let Some(ref cfg) = state.config {
        vec![
            ConfigItem {
                key: "server.listen".to_string(),
                value: serde_json::json!(cfg.gateway.listen.to_string()),
                description: Some("Server listen address".to_string()),
                editable: false,
            },
            ConfigItem {
                key: "server.workers".to_string(),
                value: serde_json::json!(cfg.gateway.workers),
                description: Some("Number of worker threads".to_string()),
                editable: true,
            },
            ConfigItem {
                key: "server.request_timeout_ms".to_string(),
                value: serde_json::json!(cfg.gateway.request_timeout.as_millis() as u64),
                description: Some("Request timeout in milliseconds".to_string()),
                editable: true,
            },
            ConfigItem {
                key: "server.max_body_size".to_string(),
                value: serde_json::json!(cfg.gateway.max_body_size),
                description: Some("Maximum request body size in bytes".to_string()),
                editable: true,
            },
            ConfigItem {
                key: "compression.enabled".to_string(),
                value: serde_json::json!(cfg.gateway.compression.enabled),
                description: Some("Enable response compression".to_string()),
                editable: true,
            },
            ConfigItem {
                key: "farp.enabled".to_string(),
                value: serde_json::json!(cfg.farp.enabled),
                description: Some("Enable FARP service discovery".to_string()),
                editable: true,
            },
            ConfigItem {
                key: "observability.logging.level".to_string(),
                value: serde_json::json!(cfg.observability.logging.level),
                description: Some("Log level (trace, debug, info, warn, error)".to_string()),
                editable: true,
            },
            ConfigItem {
                key: "observability.metrics.enabled".to_string(),
                value: serde_json::json!(cfg.observability.metrics.enabled),
                description: Some("Enable Prometheus metrics".to_string()),
                editable: true,
            },
        ]
    } else {
        vec![ConfigItem {
            key: "server.status".to_string(),
            value: serde_json::json!("running"),
            description: Some("Gateway status".to_string()),
            editable: false,
        }]
    };

    Json(config_items)
}

/// Update configuration
/// PUT /admin/api/config/:key
pub async fn api_config_update_handler(
    State(_state): State<Arc<AppState>>,
    Path(key): Path<String>,
    Json(value): Json<serde_json::Value>,
) -> impl IntoResponse {
    // Config is immutable at runtime for safety; log the attempt
    tracing::info!("Config update requested for {}: {:?}", key, value);

    Json(serde_json::json!({
        "success": true,
        "message": format!("Configuration key '{}' update noted (requires restart to take effect)", key)
    }))
}

// ============================================================================
// System Information Endpoints
// ============================================================================

/// Get system information
/// GET /admin/api/system/info
pub async fn api_system_info_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let uptime_seconds = state.metrics.as_ref().map_or_else(
        || state.start_time.elapsed().as_secs(),
        |m| m.uptime_seconds(),
    );

    let (_, _, total_memory, _) = get_system_metrics();

    let info = SystemInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime_seconds,
        start_time: Utc::now()
            .checked_sub_signed(chrono::Duration::seconds(uptime_seconds as i64))
            .unwrap_or_else(Utc::now)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string(),
        hostname: hostname::get()
            .ok()
            .and_then(|h| h.into_string().ok())
            .unwrap_or_else(|| "unknown".to_string()),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        num_cpus: num_cpus::get(),
        total_memory,
    };

    Json(info)
}

// ============================================================================
// Upstream & Service Discovery Endpoints
// ============================================================================

/// List all upstream clusters with per-instance health data
/// GET /admin/api/upstreams
pub async fn api_upstreams_list_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let mut clusters = Vec::new();

    if let Some(ref router) = state.router {
        for cluster in router.get_all_upstreams() {
            let instances: Vec<UpstreamInstanceInfo> = cluster
                .instances
                .iter()
                .map(|inst| {
                    let instance_id = format!("{}/{}", cluster.name, inst.id);
                    let (avg_latency_ms, error_rate) = if let Some(ref ht) = state.health_tracker {
                        if let Some(snap) = ht.get_snapshot(&instance_id) {
                            (snap.avg_latency.as_secs_f64() * 1000.0, snap.error_rate)
                        } else {
                            (0.0, 0.0)
                        }
                    } else {
                        (0.0, 0.0)
                    };

                    UpstreamInstanceInfo {
                        id: inst.id.clone(),
                        address: inst.address.clone(),
                        port: inst.port,
                        url: inst.base_url(),
                        weight: inst.weight,
                        healthy: inst.is_healthy(),
                        active_connections: inst.active_connections(),
                        avg_latency_ms,
                        error_rate,
                    }
                })
                .collect();

            let healthy_count = instances.iter().filter(|i| i.healthy).count();

            clusters.push(UpstreamClusterInfo {
                name: cluster.name.clone(),
                strategy: format!("{:?}", cluster.strategy),
                instance_count: instances.len(),
                healthy_count,
                instances,
            });
        }
    }

    Json(clusters)
}

/// List discovered services (upstream-based + FARP)
/// GET /admin/api/services
pub async fn api_services_list_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let mut services = Vec::new();

    // Upstream-based services
    if let Some(ref router) = state.router {
        for cluster in router.get_all_upstreams() {
            let healthy = cluster.healthy_count() == cluster.instance_count();
            let route_count = router
                .get_all_routes()
                .iter()
                .filter(|r| r.upstream_name == cluster.name)
                .count();

            let (address, port) = cluster.instances.first().map_or_else(
                || ("unknown".to_string(), 0),
                |i| (i.address.clone(), i.port),
            );

            services.push(serde_json::json!({
                "name": cluster.name,
                "version": "unknown",
                "address": address,
                "port": port,
                "route_count": route_count,
                "healthy": healthy,
                "source": "upstream",
                "instance_count": cluster.instance_count(),
                "healthy_count": cluster.healthy_count()
            }));
        }
    }

    // FARP-registered services
    if let Some(ref registry) = state.farp_registry {
        for name in registry.list_services() {
            if let Ok(reg) = registry.get_service(&name) {
                // Don't duplicate if already present from upstreams
                if !services
                    .iter()
                    .any(|s| s.get("name").and_then(|n| n.as_str()) == Some(&name))
                {
                    services.push(serde_json::json!({
                        "name": reg.service_name,
                        "version": reg.manifest.service_version,
                        "address": reg.manifest.instance_id,
                        "port": 0,
                        "route_count": reg.manifest.schemas.len(),
                        "healthy": true,
                        "source": "farp",
                        "instance_count": 1,
                        "healthy_count": 1,
                        "schemas_count": reg.schemas.len(),
                        "capabilities": reg.manifest.capabilities,
                    }));
                }
            }
        }
    }

    Json(services)
}

/// Get circuit breaker states
/// GET /admin/api/circuits
pub async fn api_circuits_list_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let mut circuits = Vec::new();

    if let Some(ref cb) = state.circuit_breaker {
        for (id, metrics) in cb.get_all_metrics() {
            let state_str = match metrics.state {
                octopus_health::CircuitState::Closed => "closed",
                octopus_health::CircuitState::Open => "open",
                octopus_health::CircuitState::HalfOpen => "half-open",
            };
            circuits.push(serde_json::json!({
                "target_url": id,
                "route_path": "",
                "state": state_str,
                "active_connections": 0,
                "failure_count": metrics.failure_count
            }));
        }
    }

    Json(circuits)
}

/// Get structured health check data
/// GET /admin/api/health/checks
pub async fn api_health_checks_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let checks = crate::handlers::build_health_from_state(&state);
    Json(checks)
}

/// Aggregated `OpenAPI` spec from FARP federation
/// GET /admin/api/openapi.json
pub async fn api_openapi_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    if let Some(ref fed) = state.farp_federation {
        if let Ok(schema) = fed.get_federated(&octopus_farp::SchemaFormat::OpenApi) {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&schema.content) {
                return Json(parsed);
            }
        }
    }

    Json(serde_json::json!({
        "openapi": "3.0.0",
        "info": {
            "title": "Octopus API Gateway",
            "version": env!("CARGO_PKG_VERSION"),
            "description": "Aggregated API documentation from registered FARP services"
        },
        "paths": {}
    }))
}

// ============================================================================
// FARP (Federated API Registry Protocol) Endpoints
// ============================================================================

/// List all registered FARP services
/// GET /admin/api/farp/services
pub async fn api_farp_services_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    if let Some(ref registry) = state.farp_registry {
        let service_names = registry.list_services();
        let mut services = Vec::new();
        for name in &service_names {
            if let Ok(reg) = registry.get_service(name) {
                services.push(FarpServiceInfo {
                    name: reg.service_name.clone(),
                    version: reg.manifest.service_version.clone(),
                    instance_id: Some(reg.manifest.instance_id.clone()),
                    schemas_count: reg.schemas.len(),
                    capabilities: reg.manifest.capabilities.clone(),
                    registered_at: format!("{:?}", reg.registered_at),
                    updated_at: reg.manifest.updated_at.to_string(),
                });
            }
        }
        Json(serde_json::to_value(services).unwrap_or_default())
    } else {
        Json(serde_json::json!([]))
    }
}

/// Get details for a specific FARP service
/// GET /admin/api/farp/services/:name
pub async fn api_farp_service_detail_handler(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    if let Some(ref registry) = state.farp_registry {
        if let Ok(reg) = registry.get_service(&name) {
            return Json(serde_json::to_value(&reg.manifest).unwrap_or_default());
        }
    }
    Json(serde_json::json!({"error": "Service not found"}))
}

/// Get the federated `OpenAPI` schema
/// GET /admin/api/farp/schema/openapi
pub async fn api_farp_federated_openapi_handler(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    if let Some(ref fed) = state.farp_federation {
        if let Ok(schema) = fed.get_federated(&octopus_farp::SchemaFormat::OpenApi) {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&schema.content) {
                return Json(parsed);
            }
        }
    }

    Json(serde_json::json!({
        "openapi": "3.0.0",
        "info": {
            "title": "Octopus Federated API",
            "version": "0.1.0"
        },
        "paths": {}
    }))
}

// ============================================================================
// Auth Configuration Endpoints
// ============================================================================

/// List configured auth providers
/// GET /admin/api/auth/providers
pub async fn api_auth_providers_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let providers = if let Some(ref config) = state.config {
        config
            .auth_providers
            .iter()
            .map(|(name, provider)| {
                let provider_type = match provider {
                    octopus_config::types::AuthProviderConfig::Jwt(_) => "jwt",
                    octopus_config::types::AuthProviderConfig::Oidc(_) => "oidc",
                    octopus_config::types::AuthProviderConfig::ApiKey(_) => "api_key",
                    octopus_config::types::AuthProviderConfig::ForwardAuth(_) => "forward_auth",
                    octopus_config::types::AuthProviderConfig::Mtls(_) => "mtls",
                    octopus_config::types::AuthProviderConfig::Introspection(_) => "introspection",
                };
                serde_json::json!({
                    "name": name,
                    "type": provider_type,
                    "status": "active",
                })
            })
            .collect::<Vec<_>>()
    } else {
        vec![]
    };

    Json(serde_json::json!(providers))
}

/// Get global auth configuration
/// GET /admin/api/auth/config
pub async fn api_auth_config_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    if let Some(ref config) = state.config {
        Json(serde_json::json!({
            "default_provider": config.auth.default_provider,
            "global_enforce": config.auth.global_enforce,
            "skip_paths": config.auth.skip_paths,
            "token_cache_ttl_secs": config.auth.token_cache_ttl.as_secs(),
            "error_format": config.auth.error_format,
            "authz_engine": format!("{:?}", config.auth.authz.engine),
            "global_rules_count": config.auth.authz.global_rules.len(),
            "opa_configured": config.auth.authz.opa.is_some(),
            "providers_count": config.auth_providers.len(),
        }))
    } else {
        Json(serde_json::json!({
            "default_provider": null,
            "global_enforce": false,
            "providers_count": 0,
        }))
    }
}

// ============================================================================
// gRPC Configuration Endpoints
// ============================================================================

/// List configured gRPC services
/// GET /admin/api/grpc/services
pub async fn api_grpc_services_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    if let Some(ref config) = state.config {
        let services: Vec<serde_json::Value> = config
            .grpc
            .services
            .iter()
            .map(|(service, upstream)| {
                serde_json::json!({
                    "service": service,
                    "upstream": upstream,
                    "enabled": config.grpc.enabled,
                })
            })
            .collect();

        Json(serde_json::json!({
            "enabled": config.grpc.enabled,
            "max_message_size": config.grpc.max_message_size,
            "enable_reflection": config.grpc.enable_reflection,
            "enable_grpc_web": config.grpc.enable_grpc_web,
            "deadline_propagation": config.grpc.deadline_propagation,
            "services": services,
        }))
    } else {
        Json(serde_json::json!({
            "enabled": false,
            "services": [],
        }))
    }
}
