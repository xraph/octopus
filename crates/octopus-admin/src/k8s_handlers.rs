//! Kubernetes CRD view endpoints.
//!
//! Read-only views of the Octopus custom resources (`OctopusGateway`,
//! `OctopusRoute`, `OctopusPolicy`, `OctopusUpstream`). Compiled only when the
//! `kubernetes` feature is enabled; otherwise the handlers report that the
//! feature is unavailable. A fresh in-cluster/kubeconfig client is created per
//! request, which is fine for an occasionally-used admin view.

use std::sync::Arc;

use axum::{extract::State, response::IntoResponse, Json};

use crate::handlers::AppState;
#[cfg(feature = "kubernetes")]
use crate::models::K8sResourceSummary;
use crate::models::K8sStatus;

#[cfg(feature = "kubernetes")]
mod live {
    use super::K8sResourceSummary;
    use kube::{api::ListParams, Api, Client, Resource, ResourceExt};
    use octopus_k8s::crds::{OctopusGateway, OctopusPolicy, OctopusRoute, OctopusUpstream};
    use serde::{de::DeserializeOwned, Serialize};

    /// Try to build a Kubernetes client from the ambient environment.
    pub async fn client() -> Result<Client, String> {
        Client::try_default().await.map_err(|e| e.to_string())
    }

    /// List a CRD kind and project each item into a summary DTO.
    pub async fn list_kind<K>(client: &Client, kind: &str) -> Vec<K8sResourceSummary>
    where
        K: Resource<DynamicType = ()> + Clone + DeserializeOwned + Serialize + std::fmt::Debug,
    {
        let api: Api<K> = Api::all(client.clone());
        match api.list(&ListParams::default()).await {
            Ok(list) => list
                .items
                .into_iter()
                .map(|item| {
                    let created_at = item
                        .meta()
                        .creation_timestamp
                        .as_ref()
                        .map(|t| t.0.to_rfc3339());
                    let spec = serde_json::to_value(&item)
                        .ok()
                        .and_then(|v| v.get("spec").cloned())
                        .unwrap_or(serde_json::Value::Null);
                    K8sResourceSummary {
                        name: item.name_any(),
                        namespace: item.namespace(),
                        kind: kind.to_string(),
                        spec,
                        created_at,
                    }
                })
                .collect(),
            Err(e) => {
                tracing::warn!("Failed to list {kind}: {e}");
                vec![]
            }
        }
    }

    /// Dispatch a list by kind name.
    pub async fn list_by_kind(client: &Client, kind: &str) -> Vec<K8sResourceSummary> {
        match kind {
            "OctopusGateway" => list_kind::<OctopusGateway>(client, kind).await,
            "OctopusRoute" => list_kind::<OctopusRoute>(client, kind).await,
            "OctopusPolicy" => list_kind::<OctopusPolicy>(client, kind).await,
            "OctopusUpstream" => list_kind::<OctopusUpstream>(client, kind).await,
            _ => vec![],
        }
    }
}

/// Common list handler body for a given CRD kind.
#[cfg(feature = "kubernetes")]
async fn list_resources(kind: &str) -> Json<Vec<K8sResourceSummary>> {
    match live::client().await {
        Ok(client) => Json(live::list_by_kind(&client, kind).await),
        Err(e) => {
            tracing::warn!("Kubernetes client unavailable: {e}");
            Json(vec![])
        }
    }
}

#[cfg(not(feature = "kubernetes"))]
async fn list_resources(_kind: &str) -> Json<Vec<crate::models::K8sResourceSummary>> {
    Json(vec![])
}

/// `GET /admin/api/k8s/gateways`
pub async fn api_k8s_gateways_handler(State(_state): State<Arc<AppState>>) -> impl IntoResponse {
    list_resources("OctopusGateway").await
}

/// `GET /admin/api/k8s/routes`
pub async fn api_k8s_routes_handler(State(_state): State<Arc<AppState>>) -> impl IntoResponse {
    list_resources("OctopusRoute").await
}

/// `GET /admin/api/k8s/policies`
pub async fn api_k8s_policies_handler(State(_state): State<Arc<AppState>>) -> impl IntoResponse {
    list_resources("OctopusPolicy").await
}

/// `GET /admin/api/k8s/upstreams`
pub async fn api_k8s_upstreams_handler(State(_state): State<Arc<AppState>>) -> impl IntoResponse {
    list_resources("OctopusUpstream").await
}

/// `GET /admin/api/k8s/status`
pub async fn api_k8s_status_handler(State(_state): State<Arc<AppState>>) -> impl IntoResponse {
    #[cfg(feature = "kubernetes")]
    {
        let mut counts = std::collections::HashMap::new();
        let (connected, detail) = match live::client().await {
            Ok(client) => {
                for kind in [
                    "OctopusGateway",
                    "OctopusRoute",
                    "OctopusPolicy",
                    "OctopusUpstream",
                ] {
                    counts.insert(
                        kind.to_string(),
                        live::list_by_kind(&client, kind).await.len(),
                    );
                }
                (true, None)
            }
            Err(e) => (false, Some(e)),
        };
        Json(K8sStatus {
            connected,
            feature_enabled: true,
            detail,
            counts,
        })
    }
    #[cfg(not(feature = "kubernetes"))]
    {
        Json(K8sStatus {
            connected: false,
            feature_enabled: false,
            detail: Some(
                "Built without the `kubernetes` feature. Rebuild octopus-admin with \
                 --features kubernetes to enable live CRD views."
                    .to_string(),
            ),
            counts: std::collections::HashMap::new(),
        })
    }
}
