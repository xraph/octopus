//! Upstream cluster management endpoints (CRUD).
//!
//! Mutates the live [`octopus_router::Router`], mirroring the route CRUD
//! handlers. Clusters that are owned by the Kubernetes operator or loaded from
//! config may be overwritten on the next reconcile; such edits are therefore
//! effectively ephemeral when the operator is active.

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use octopus_core::{LoadBalanceStrategy, UpstreamCluster, UpstreamInstance};

use crate::handlers::AppState;
use crate::models::{UpstreamClusterInfo, UpstreamConfig, UpstreamInstanceInfo};

/// Parse a load-balancing strategy string (tolerant of common aliases).
fn parse_strategy(raw: Option<&str>) -> LoadBalanceStrategy {
    match raw.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
        Some("least_connections" | "least_conn" | "leastconnections") => {
            LoadBalanceStrategy::LeastConnections
        }
        Some("weighted_round_robin" | "weighted" | "weighted_round" | "weightedroundrobin") => {
            LoadBalanceStrategy::WeightedRoundRobin
        }
        Some("random") => LoadBalanceStrategy::Random,
        Some("ip_hash" | "iphash") => LoadBalanceStrategy::IpHash,
        _ => LoadBalanceStrategy::RoundRobin,
    }
}

/// Build an [`UpstreamCluster`] from an [`UpstreamConfig`] payload.
fn build_cluster(cfg: &UpstreamConfig) -> UpstreamCluster {
    let mut cluster = UpstreamCluster::new(&cfg.name);
    cluster.strategy = parse_strategy(cfg.strategy.as_deref());
    for (i, inst) in cfg.instances.iter().enumerate() {
        let id = inst
            .id
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| format!("{}-{i}", cfg.name));
        let mut instance = UpstreamInstance::new(id, inst.address.clone(), inst.port);
        if let Some(w) = inst.weight {
            instance.weight = w;
        }
        cluster.add_instance(instance);
    }
    cluster
}

/// Map a live cluster (plus health data) into the admin DTO.
pub(crate) fn cluster_to_info(state: &AppState, cluster: &UpstreamCluster) -> UpstreamClusterInfo {
    let instances: Vec<UpstreamInstanceInfo> = cluster
        .instances
        .iter()
        .map(|inst| {
            let instance_id = format!("{}/{}", cluster.name, inst.id);
            let (avg_latency_ms, error_rate) = state
                .health_tracker
                .as_ref()
                .and_then(|ht| ht.get_snapshot(&instance_id))
                .map_or((0.0, 0.0), |snap| {
                    (snap.avg_latency.as_secs_f64() * 1000.0, snap.error_rate)
                });
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
    UpstreamClusterInfo {
        name: cluster.name.clone(),
        strategy: format!("{:?}", cluster.strategy),
        instance_count: instances.len(),
        healthy_count,
        instances,
    }
}

/// Create a new upstream cluster.
/// `POST /admin/api/upstreams`
pub async fn api_upstream_create_handler(
    State(state): State<Arc<AppState>>,
    Json(cfg): Json<UpstreamConfig>,
) -> impl IntoResponse {
    let Some(ref router) = state.router else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "Router not available" })),
        );
    };

    if cfg.name.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Upstream name is required" })),
        );
    }
    if router.get_upstream(&cfg.name).is_some() {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": format!("Upstream '{}' already exists", cfg.name)
            })),
        );
    }

    let name = cfg.name.clone();
    router.register_upstream(build_cluster(&cfg));
    tracing::info!("Created upstream cluster '{name}'");

    let info = router
        .get_upstream(&name)
        .map(|c| cluster_to_info(&state, &c));
    (
        StatusCode::CREATED,
        Json(serde_json::to_value(info).unwrap_or_default()),
    )
}

/// Get a single upstream cluster.
/// `GET /admin/api/upstreams/:name`
pub async fn api_upstream_get_handler(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    if let Some(ref router) = state.router {
        if let Some(cluster) = router.get_upstream(&name) {
            return (
                StatusCode::OK,
                Json(serde_json::to_value(cluster_to_info(&state, &cluster)).unwrap_or_default()),
            );
        }
    }
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({ "error": "Upstream not found", "name": name })),
    )
}

/// Update (upsert) an upstream cluster.
/// `PUT /admin/api/upstreams/:name`
pub async fn api_upstream_update_handler(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(mut cfg): Json<UpstreamConfig>,
) -> impl IntoResponse {
    let Some(ref router) = state.router else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "Router not available" })),
        );
    };

    // The path name is authoritative.
    cfg.name.clone_from(&name);
    router.register_upstream(build_cluster(&cfg));
    tracing::info!("Updated upstream cluster '{name}'");

    let info = router
        .get_upstream(&name)
        .map(|c| cluster_to_info(&state, &c));
    (
        StatusCode::OK,
        Json(serde_json::to_value(info).unwrap_or_default()),
    )
}

/// Delete an upstream cluster.
/// `DELETE /admin/api/upstreams/:name`
pub async fn api_upstream_delete_handler(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    if let Some(ref router) = state.router {
        if router.remove_upstream(&name) {
            tracing::info!("Deleted upstream cluster '{name}'");
            return StatusCode::NO_CONTENT;
        }
    }
    StatusCode::NOT_FOUND
}
