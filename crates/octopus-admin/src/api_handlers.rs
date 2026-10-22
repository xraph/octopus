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
    extract::{rejection::JsonRejection, Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use std::collections::HashMap;
use std::sync::Arc;

use crate::handlers::AppState;
use crate::models::{
    ActivityLogEntry, AnalyticsMetrics, ConfigItem, FarpBindingInfo, FarpBindingUpdate,
    FarpServiceInfo, LatencyPercentiles, LogQuery, PerformanceMetrics, RouteConfig, RouteMetric,
    SecurityEvent, SystemInfo, TimeSeriesPoint, UpstreamClusterInfo, UpstreamInstanceInfo,
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

/// A failed admin API call: the status and JSON body to answer with.
#[derive(Debug)]
pub(crate) struct ApiError {
    pub status: StatusCode,
    pub body: serde_json::Value,
}

impl ApiError {
    /// An error answering `status` with `{"error": message}`.
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            body: serde_json::json!({ "error": message.into() }),
        }
    }

    /// An error answering `409` with `{"error": message, "conflict": other}`.
    fn conflict(message: impl Into<String>, other: &str) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            body: serde_json::json!({ "error": message.into(), "conflict": other }),
        }
    }

    /// A `404` for the route `id`.
    fn route_not_found(id: &str) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            body: serde_json::json!({ "error": "Route not found", "id": id }),
        }
    }

    /// The router is not wired into the admin state.
    fn router_unavailable() -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, "Router not available")
    }

    /// A route body that is not valid JSON or misses a required field.
    fn bad_body(rejection: &JsonRejection) -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            format!("Invalid route body: {}", rejection.body_text()),
        )
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (self.status, Json(self.body)).into_response()
    }
}

/// Serializes admin route writes, so one request's conflict check and swap
/// never interleave with another's, and a rollback never removes entries a
/// concurrent request inserted.
static ROUTE_WRITES: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Hold the admin route write lock. Never held across an `.await`.
fn route_writes() -> std::sync::MutexGuard<'static, ()> {
    ROUTE_WRITES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Whether `a` and `b` are the same router entry: the key the trie keeps
/// unique per method.
fn same_entry(a: &octopus_router::Route, b: &octopus_router::Route) -> bool {
    a.method == b.method && a.path == b.path && a.host == b.host && a.id == b.id
}

/// Whether `id` has the `route-<n>` shape the list uses for routes without
/// an id. Those ids are never given to a route.
fn is_positional_id(id: &str) -> bool {
    id.strip_prefix("route-")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// The id-less route the list shows as `id`, when `id` is a positional
/// `route-{i}`.
fn positional_route(router: &octopus_router::Router, id: &str) -> Option<octopus_router::Route> {
    if !is_positional_id(id) {
        return None;
    }
    router
        .get_all_routes()
        .into_iter()
        .enumerate()
        .find(|(index, route)| route.id.is_none() && format!("route-{index}") == id)
        .map(|(_, route)| route)
}

/// The error for a body or path id of the reserved positional shape.
fn reserved_id(id: &str) -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        format!("Route id {id} is reserved: route-<n> names a listed route without an id"),
    )
}

/// Create or replace the route `id` from an admin API body.
///
/// - An id carried by routes from the config file or the admin API replaces
///   all of them.
/// - An id carried by a discovered route (FARP or the Kubernetes operator)
///   is a 409: those routes belong to their discoverer.
/// - A positional `route-{i}` id replaces that one id-less route, and the
///   new entries get a fresh UUID v4 id.
/// - Any other `route-<n>` id is a 400, and any other id creates the route.
///
/// Admin route writes are serialized, so the check and the swap are atomic
/// against other admin requests.
pub(crate) fn upsert_admin_route(
    router: &octopus_router::Router,
    id: &str,
    body: &RouteConfig,
) -> Result<Vec<octopus_router::Route>, ApiError> {
    let _writes = route_writes();

    let held = router.routes_by_id(id);
    if let Some(discovered) = held.iter().find(|route| !route.origin.is_declared()) {
        return Err(ApiError::conflict(
            format!(
                "Route {id} was discovered by {} and can't be replaced through the admin API",
                discovered.origin.as_str()
            ),
            id,
        ));
    }

    if !held.is_empty() {
        return swap_route(router, id, &held, body);
    }
    if is_positional_id(id) {
        let Some(entry) = positional_route(router, id) else {
            return Err(reserved_id(id));
        };
        let new_id = uuid::Uuid::new_v4().to_string();
        return swap_route(router, &new_id, &[entry], body);
    }
    swap_route(router, id, &[], body)
}

/// Swap `old_entries` for the entries `body` describes under `id`, with the
/// route write lock held.
///
/// Every new entry (one per method and host, origin
/// [`RouteOrigin::Admin`]) is built and checked before the router changes.
/// An unknown upstream is a 400, and an entry on the same method, trie node
/// and host as a declared route of another id is a 409 naming that route's
/// listed id in `conflict`. If an insert fails, the router is put back
/// exactly as it was.
///
/// [`RouteOrigin::Admin`]: octopus_router::RouteOrigin::Admin
fn swap_route(
    router: &octopus_router::Router,
    id: &str,
    old_entries: &[octopus_router::Route],
    body: &RouteConfig,
) -> Result<Vec<octopus_router::Route>, ApiError> {
    let new_entries = body
        .to_config_route(id)
        .to_routes(octopus_router::RouteOrigin::Admin)
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, format!("Invalid route: {e}")))?;

    if router.get_upstream(&body.upstream).is_none() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            format!("Unknown upstream: {}", body.upstream),
        ));
    }

    let new_keys: Vec<_> = new_entries
        .iter()
        .map(|new| (&new.method, octopus_router::node_key(&new.path), &new.host))
        .collect();

    for (index, existing) in router.get_all_routes().into_iter().enumerate() {
        if !existing.origin.is_declared()
            || existing.id.as_deref() == Some(id)
            || old_entries.iter().any(|old| same_entry(old, &existing))
        {
            continue;
        }
        let key = octopus_router::node_key(&existing.path);
        let clashes = new_keys.iter().any(|(method, path, host)| {
            **method == existing.method && *path == key && **host == existing.host
        });
        if clashes {
            let other = crate::handlers::listed_route_id(index, &existing);
            return Err(ApiError::conflict(
                format!(
                    "Route {other} already serves {} {} on {}",
                    existing.method,
                    existing.path,
                    existing.host.pattern().as_deref().unwrap_or("any host"),
                ),
                &other,
            ));
        }
    }

    for old in old_entries {
        router.remove_route_exact(&old.method, &old.path, &old.host, old.id.as_deref());
    }

    for (inserted, route) in new_entries.iter().enumerate() {
        if let Err(e) = router.add_route(route.clone()) {
            for new in &new_entries[..inserted] {
                router.remove_route_exact(&new.method, &new.path, &new.host, new.id.as_deref());
            }
            for old in old_entries {
                if let Err(restore) = router.add_route(old.clone()) {
                    tracing::error!(id = %id, error = %restore, "Could not restore route");
                }
            }
            return Err(ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to add route: {e}"),
            ));
        }
    }

    tracing::info!(
        id = %id,
        path = %body.path,
        upstream = %body.upstream,
        entries = new_entries.len(),
        replaced = old_entries.len(),
        "Route upserted through the admin API"
    );

    Ok(new_entries)
}

/// The `RouteInfo` of the first entry of a freshly upserted route.
fn upserted_info(entries: &[octopus_router::Route]) -> serde_json::Value {
    entries.first().map_or(serde_json::Value::Null, |route| {
        serde_json::to_value(crate::handlers::route_to_info(
            crate::handlers::listed_route_id(0, route),
            route,
            0,
            0,
            0.0,
            true,
        ))
        .unwrap_or_default()
    })
}

/// Get single route by ID
/// GET /admin/api/routes/:id
pub async fn api_route_get_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if state.router.is_none() {
        return Err(ApiError::router_unavailable());
    }
    let routes = crate::handlers::build_routes_from_state(&state);
    routes
        .into_iter()
        .find(|r| r.id == id)
        .map(Json)
        .ok_or_else(|| ApiError::route_not_found(&id))
}

/// Create new route (mutates the live router)
/// POST /admin/api/routes
pub async fn api_route_create_handler(
    State(state): State<Arc<AppState>>,
    body: Result<Json<RouteConfig>, JsonRejection>,
) -> impl IntoResponse {
    let Some(ref router) = state.router else {
        return Err(ApiError::router_unavailable());
    };
    let Json(config) = body.map_err(|rejection| ApiError::bad_body(&rejection))?;

    let id = config
        .id
        .clone()
        .filter(|id| !id.trim().is_empty())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let _writes = route_writes();
    if !router.routes_by_id(&id).is_empty() {
        return Err(ApiError::conflict(
            format!("Route {id} already exists"),
            &id,
        ));
    }
    if is_positional_id(&id) {
        return Err(reserved_id(&id));
    }

    let entries = swap_route(router, &id, &[], &config)?;
    Ok((StatusCode::CREATED, Json(upserted_info(&entries))))
}

/// Create or replace a route by id (idempotent upsert; the path id wins over
/// a body id)
/// PUT /admin/api/routes/:id
pub async fn api_route_update_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    body: Result<Json<RouteConfig>, JsonRejection>,
) -> impl IntoResponse {
    let Some(ref router) = state.router else {
        return Err(ApiError::router_unavailable());
    };
    let Json(config) = body.map_err(|rejection| ApiError::bad_body(&rejection))?;

    let entries = upsert_admin_route(router, &id, &config)?;
    Ok(Json(upserted_info(&entries)))
}

/// Delete every entry of a route by id, or one id-less route by its legacy
/// positional id. Routes discovered by FARP or the Kubernetes operator are
/// left to their discoverer.
/// DELETE /admin/api/routes/:id
pub async fn api_route_delete_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let Some(ref router) = state.router else {
        return Err(ApiError::router_unavailable());
    };

    let _writes = route_writes();
    let declared: Vec<_> = router
        .routes_by_id(&id)
        .into_iter()
        .filter(|route| route.origin.is_declared())
        .collect();
    if !declared.is_empty() {
        for route in &declared {
            router.remove_route_exact(&route.method, &route.path, &route.host, route.id.as_deref());
        }
        tracing::info!(id = %id, removed = declared.len(), "Route deleted through the admin API");
        return Ok(StatusCode::NO_CONTENT);
    }

    if router.routes_by_id(&id).is_empty() {
        if let Some(route) = positional_route(router, &id) {
            if router.remove_route_exact(&route.method, &route.path, &route.host, None) {
                tracing::info!(id = %id, method = %route.method, path = %route.path, "Route deleted through the admin API");
                return Ok(StatusCode::NO_CONTENT);
            }
        }
    }

    Err(ApiError::route_not_found(&id))
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

/// The wall-clock time this process started, as `YYYY-MM-DD HH:MM:SS` UTC.
/// It comes from a value captured once at startup (the metrics collector's,
/// else the admin state's), so it stays the same until Octopus restarts.
fn start_time_string(state: &AppState) -> String {
    let started_ms = state.metrics.as_ref().map_or_else(
        || {
            state
                .started_at
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        },
        |m| m.start_time_ms(),
    );
    chrono::DateTime::<Utc>::from_timestamp_millis(i64::try_from(started_ms).unwrap_or(i64::MAX))
        .unwrap_or_default()
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

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
        start_time: start_time_string(&state),
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

/// The 404 for the FARP binding endpoints when FARP is not enabled.
fn farp_not_enabled() -> ApiError {
    ApiError::new(StatusCode::NOT_FOUND, "FARP is not enabled")
}

/// The binding as the admin API reports it, with the router's FARP route
/// entry count.
fn farp_binding_info(
    binding: Option<&octopus_farp::GatewayBinding>,
    router: Option<&octopus_router::Router>,
) -> FarpBindingInfo {
    FarpBindingInfo {
        hostnames: binding
            .map(octopus_farp::GatewayBinding::hostnames)
            .unwrap_or_default(),
        gateway_id: binding.and_then(|b| b.gateway_id.clone()),
        farp_routes: router.map_or(0, |router| {
            router
                .get_all_routes()
                .iter()
                .filter(|route| route.origin == octopus_router::RouteOrigin::Farp)
                .count()
        }),
    }
}

/// The hostnames FARP routes are bound to
/// GET /admin/api/farp/binding
pub async fn api_farp_binding_get_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let cell = state.farp_binding.as_ref().ok_or_else(farp_not_enabled)?;
    let binding = cell.load();
    Ok::<_, ApiError>(Json(farp_binding_info(
        (**binding).as_ref(),
        state.router.as_deref(),
    )))
}

/// Bind FARP routes to new hostnames and move the existing ones there.
/// PUT /admin/api/farp/binding
///
/// The binding's other policy (gateway id, auth, rate limit, timeout) stays.
/// The change lives in memory only, so the config file must carry the same
/// `farp.gateway.hostnames` to survive a restart.
pub async fn api_farp_binding_put_handler(
    State(state): State<Arc<AppState>>,
    body: Result<Json<FarpBindingUpdate>, JsonRejection>,
) -> impl IntoResponse {
    let cell = state.farp_binding.as_ref().ok_or_else(farp_not_enabled)?;
    let Json(update) = body.map_err(|rejection| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            format!("Invalid binding body: {}", rejection.body_text()),
        )
    })?;

    let mut hosts: Vec<octopus_router::HostMatch> = Vec::with_capacity(update.hostnames.len());
    for hostname in &update.hostnames {
        let host = octopus_router::HostMatch::parse_pattern(hostname).map_err(|e| {
            ApiError::new(StatusCode::BAD_REQUEST, format!("Invalid hostname: {e}"))
        })?;
        if !hosts.contains(&host) {
            hosts.push(host);
        }
    }

    // Rehosting rewrites routes, so it is serialized with the route writes.
    let _writes = route_writes();
    let current = cell.load();
    let mut binding = (**current)
        .clone()
        .unwrap_or_else(|| octopus_farp::GatewayBinding::for_hostnames(&[]));
    binding.hosts.clone_from(&hosts);
    cell.store(Arc::new(Some(binding.clone())));

    if let Some(ref router) = state.router {
        let moved = router.rehost_routes(octopus_router::RouteOrigin::Farp, &hosts);
        tracing::info!(
            hostnames = ?binding.hostnames(),
            routes = moved,
            "FARP binding updated through the admin API"
        );
    }

    Ok::<_, ApiError>(Json(farp_binding_info(
        Some(&binding),
        state.router.as_deref(),
    )))
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
                    octopus_config::types::AuthProviderConfig::ConventionAuth(_) => "convention",
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
