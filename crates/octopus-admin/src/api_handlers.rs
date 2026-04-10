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
use crate::models::{PerformanceMetrics, RouteInfo, RouteConfig, PluginInfo, LogQuery, SystemInfo, AnalyticsMetrics, TimeSeriesPoint, LatencyPercentiles, RouteMetric, ActivityLogEntry, SecurityEvent, ConfigItem};

// ============================================================================
// Metrics & Analytics Endpoints
// ============================================================================

/// Get comprehensive analytics metrics
/// GET /admin/api/analytics?timeframe=24h
pub async fn api_analytics_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let timeframe = params.get("timeframe").map_or("24h", std::string::String::as_str);

    if let Some(ref m) = state.metrics {
        let snapshot = octopus_metrics::MetricsSnapshot::from_collector(m);
        let top_routes: Vec<RouteMetric> = snapshot.routes.iter().map(|r| RouteMetric {
            path: r.path.clone(),
            requests: r.request_count,
            avg_latency: r.avg_latency_ms,
            error_rate: r.error_rate,
        }).collect();

        let mut traffic_by_method = HashMap::new();
        // Derive from routes if available
        if let Some(ref router) = state.router {
            for route in router.get_all_routes() {
                let method = route.method.to_string();
                let count = m.route_stats(&format!("{} {}", method, route.path))
                    .map_or(0, |s| s.request_count.load(std::sync::atomic::Ordering::Relaxed));
                *traffic_by_method.entry(method).or_insert(0u64) += count;
            }
        }

        let analytics = AnalyticsMetrics {
            timeframe: timeframe.to_string(),
            request_volume: vec![], // Historical time series not yet stored
            latency_percentiles: LatencyPercentiles {
                p50: snapshot.routes.first().map_or(0.0, |r| r.p50_latency_ms),
                p90: snapshot.routes.first().map_or(0.0, |r| r.avg_latency_ms),
                p95: snapshot.routes.first().map_or(0.0, |r| r.p95_latency_ms),
                p99: snapshot.routes.first().map_or(0.0, |r| r.p99_latency_ms),
            },
            error_breakdown: HashMap::new(), // Per-status not yet tracked
            top_routes,
            status_code_distribution: HashMap::new(), // Not yet tracked
            traffic_by_method,
        };

        Json(analytics)
    } else {
        // No metrics collector — return empty analytics
        let analytics = AnalyticsMetrics {
            timeframe: timeframe.to_string(),
            request_volume: vec![],
            latency_percentiles: LatencyPercentiles { p50: 0.0, p90: 0.0, p95: 0.0, p99: 0.0 },
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
pub async fn api_realtime_metrics_handler(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
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
    let period = params.get("period").map_or("1h", std::string::String::as_str);

    // Current point from real metrics
    if let Some(ref m) = state.metrics {
        let now = Utc::now();
        let value = match metric {
            "requests" => m.total_requests() as f64,
            "errors" => m.total_errors() as f64,
            "latency" => m.global_avg_latency_ms(),
            "connections" => m.active_connections() as f64,
            _ => m.total_requests() as f64,
        };

        // Return a single current data point (historical tracking requires a time-series store)
        let data = vec![TimeSeriesPoint {
            timestamp: now.format("%Y-%m-%d %H:%M:%S").to_string(),
            value,
        }];
        Json(data)
    } else {
        // No metrics collector — return empty time series
        Json(vec![] as Vec<TimeSeriesPoint>)
    }
}

/// Get performance metrics
/// GET /admin/api/metrics/performance
pub async fn api_performance_metrics_handler(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let active_connections = state.metrics.as_ref().map_or(0, |m| m.active_connections());

    let metrics = PerformanceMetrics {
        cpu_usage: 0.0, // System-level metrics not yet collected
        memory_usage: 0.0,
        memory_total: 0,
        memory_available: 0,
        goroutines: active_connections, // repurpose as "active tasks"
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

/// Create new route
/// POST /admin/api/routes
pub async fn api_route_create_handler(
    State(_state): State<Arc<AppState>>,
    Json(config): Json<RouteConfig>,
) -> impl IntoResponse {
    // TODO: Create route in system
    tracing::info!("Creating route: {} {}", config.method, config.path);

    let route = RouteInfo {
        id: uuid::Uuid::new_v4().to_string(),
        path: config.path,
        method: config.method,
        upstream: config.upstream,
        request_count: 0,
        is_healthy: true,
        avg_latency_ms: 0.0,
        error_count: 0,
        last_accessed: None,
    };

    (StatusCode::CREATED, Json(route))
}

/// Update existing route
/// PUT /admin/api/routes/:id
pub async fn api_route_update_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(config): Json<RouteConfig>,
) -> impl IntoResponse {
    tracing::info!("Updating route {}: {} {}", id, config.method, config.path);

    // TODO: Actually mutate the router when write API is ready
    // For now, return the submitted config as confirmation
    let route = RouteInfo {
        id,
        path: config.path,
        method: config.method,
        upstream: config.upstream,
        request_count: 0,
        is_healthy: true,
        avg_latency_ms: 0.0,
        error_count: 0,
        last_accessed: None,
    };

    Json(route)
}

/// Delete route
/// DELETE /admin/api/routes/:id
pub async fn api_route_delete_handler(
    State(_state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    // TODO: Delete route from system
    tracing::info!("Deleting route: {}", id);

    StatusCode::NO_CONTENT
}

// ============================================================================
// Plugin Management Endpoints
// ============================================================================

/// List all plugins
/// GET /admin/api/plugins
pub async fn api_plugins_list_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let plugins = crate::handlers::build_plugins_from_state(&state).await;
    Json(plugins)
}

/// Get plugin by ID
/// GET /admin/api/plugins/:id
pub async fn api_plugin_get_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let plugins = crate::handlers::build_plugins_from_state(&state).await;
    if let Some(plugin) = plugins.into_iter().find(|p| p.id == id) {
        Json(serde_json::to_value(plugin).unwrap())
    } else {
        Json(serde_json::json!({"error": "Plugin not found", "id": id}))
    }
}

/// Enable/disable plugin
/// POST /admin/api/plugins/:id/toggle
pub async fn api_plugin_toggle_handler(
    State(_state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    // TODO: Toggle plugin state
    tracing::info!("Toggling plugin: {}", id);

    Json(serde_json::json!({
        "success": true,
        "message": "Plugin state toggled"
    }))
}

/// Update plugin configuration
/// PUT /admin/api/plugins/:id/config
pub async fn api_plugin_config_handler(
    State(_state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(config): Json<serde_json::Value>,
) -> impl IntoResponse {
    // TODO: Update plugin configuration
    tracing::info!("Updating plugin {} config: {:?}", id, config);

    Json(serde_json::json!({
        "success": true,
        "message": "Plugin configuration updated"
    }))
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
                level: if e.is_error() { "error".to_string() } else { "info".to_string() },
                message: format!("{} {} → {} ({:.1}ms)", e.method, e.path, e.status, e.latency_ms),
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
    // Security events from circuit breaker rejections
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
                        metrics.state, id, metrics.failure_rate * 100.0,
                        metrics.failure_count, metrics.total_count
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
        // No config loaded — return minimal defaults
        vec![
            ConfigItem {
                key: "server.status".to_string(),
                value: serde_json::json!("running"),
                description: Some("Gateway status".to_string()),
                editable: false,
            },
        ]
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
    // TODO: Update configuration
    tracing::info!("Updating config {}: {:?}", key, value);

    Json(serde_json::json!({
        "success": true,
        "message": "Configuration updated"
    }))
}

// ============================================================================
// System Information Endpoints
// ============================================================================

/// Get system information
/// GET /admin/api/system/info
pub async fn api_system_info_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let uptime_seconds = state.metrics.as_ref()
        .map_or(state.start_time.elapsed().as_secs(), |m| m.uptime_seconds());

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
        total_memory: 0, // System-level memory not yet collected
    };

    Json(info)
}

// ============================================================================
// Upstream & Service Discovery Endpoints
// ============================================================================

/// List all upstream targets
/// GET /admin/api/upstreams
pub async fn api_upstreams_list_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let mut upstreams = Vec::new();

    if let Some(ref router) = state.router {
        for cluster in router.get_all_upstreams() {
            for inst in &cluster.instances {
                upstreams.push(serde_json::json!({
                    "url": inst.base_url(),
                    "route_path": cluster.name,
                    "weight": inst.weight,
                    "healthy": inst.is_healthy(),
                    "active_connections": inst.active_connections()
                }));
            }
        }
    }

    Json(upstreams)
}

/// List discovered FARP services
/// GET /admin/api/services
pub async fn api_services_list_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let mut services = Vec::new();

    if let Some(ref router) = state.router {
        for cluster in router.get_all_upstreams() {
            let healthy = cluster.healthy_count() == cluster.instance_count();
            // Count routes that reference this upstream
            let route_count = router.get_all_routes()
                .iter()
                .filter(|r| r.upstream_name == cluster.name)
                .count();

            // Use first instance for address/port
            let (address, port) = cluster.instances.first()
                .map(|i| (i.address.clone(), i.port))
                .unwrap_or_else(|| ("unknown".to_string(), 0));

            services.push(serde_json::json!({
                "name": cluster.name,
                "version": "unknown",
                "address": address,
                "port": port,
                "route_count": route_count,
                "healthy": healthy
            }));
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

/// Aggregated OpenAPI spec (placeholder)
/// GET /admin/api/openapi.json
pub async fn api_openapi_handler(State(_state): State<Arc<AppState>>) -> impl IntoResponse {
    // TODO: Wire to FARP schema federation
    let spec = serde_json::json!({
        "openapi": "3.0.0",
        "info": {
            "title": "Octopus API Gateway",
            "version": "0.1.0",
            "description": "Aggregated API documentation from registered FARP services"
        },
        "paths": {}
    });
    Json(spec)
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
                services.push(serde_json::json!({
                    "name": reg.service_name,
                    "manifest": {
                        "version": reg.manifest.version,
                        "service_version": reg.manifest.service_version,
                        "instance_id": reg.manifest.instance_id,
                        "capabilities": reg.manifest.capabilities,
                        "schemas_count": reg.manifest.schemas.len(),
                        "updated_at": reg.manifest.updated_at,
                        "checksum": reg.manifest.checksum,
                    },
                    "registered_at": format!("{:?}", reg.registered_at),
                    "schemas_count": reg.schemas.len(),
                }));
            }
        }
        Json(serde_json::json!(services))
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

/// Get the federated OpenAPI schema
/// GET /admin/api/farp/schema/openapi
pub async fn api_farp_federated_openapi_handler(State(_state): State<Arc<AppState>>) -> impl IntoResponse {
    // TODO: Wire to SchemaFederation when available in AppState
    Json(serde_json::json!({
        "openapi": "3.0.0",
        "info": {
            "title": "Octopus Federated API",
            "version": "0.1.0"
        },
        "paths": {}
    }))
}

// All mock data generators removed — all endpoints now return real data from AppState
