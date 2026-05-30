//! Reconciler that turns Kubernetes routing resources into live router state.
//!
//! [`RouteReconciler`] owns a [`RouteStore`] and the live [`Router`]. As
//! resources are upserted/removed it re-translates, merges every source, and
//! applies the result to the router — so the router always reflects the merged
//! intent of static config + Gateway API + Octopus CRDs.

use crate::apply::apply_to_router;
use crate::gateway_api::{HTTPRoute, HTTPRouteSpec};
use crate::ir::{IntermediateRoute, RouteSource, RouteStore, SourceKey};
use crate::translate::httproute_to_route;
use futures::TryStreamExt;
use kube::{
    api::Api,
    runtime::{watcher, watcher::Config as WatcherConfig},
    Client,
};
use octopus_core::UpstreamCluster;
use octopus_router::Router;
use std::sync::{Arc, Mutex};

/// Reconciles routing resources into the live [`Router`].
pub struct RouteReconciler {
    store: Mutex<RouteStore>,
    router: Arc<Router>,
}

impl std::fmt::Debug for RouteReconciler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RouteReconciler").finish()
    }
}

impl RouteReconciler {
    /// Create a reconciler driving `router`.
    pub fn new(router: Arc<Router>) -> Self {
        Self {
            store: Mutex::new(RouteStore::new()),
            router,
        }
    }

    /// Seed static (config-file) routes so they survive merges with
    /// dynamically-reconciled sources.
    pub fn seed_static(&self, routes: Vec<IntermediateRoute>, upstreams: Vec<UpstreamCluster>) {
        if let Ok(mut store) = self.store.lock() {
            store.insert(
                SourceKey::new(RouteSource::Static, "config"),
                routes,
                upstreams,
            );
        }
        self.reapply();
    }

    /// Upsert an `HTTPRoute` and re-apply the merged routing table.
    pub fn upsert_httproute(&self, name: &str, namespace: &str, spec: &HTTPRouteSpec) {
        let (routes, upstreams) = httproute_to_route(name, namespace, spec);
        if let Ok(mut store) = self.store.lock() {
            store.insert(
                SourceKey::new(RouteSource::GatewayApi, format!("{namespace}/{name}")),
                routes,
                upstreams,
            );
        }
        self.reapply();
    }

    /// Remove an `HTTPRoute` and re-apply the merged routing table.
    pub fn remove_httproute(&self, name: &str, namespace: &str) {
        if let Ok(mut store) = self.store.lock() {
            store.remove(&SourceKey::new(
                RouteSource::GatewayApi,
                format!("{namespace}/{name}"),
            ));
        }
        self.reapply();
    }

    /// Merge all sources and apply the result to the live router.
    fn reapply(&self) {
        let table = match self.store.lock() {
            Ok(store) => store.merge(),
            Err(_) => {
                tracing::error!("RouteStore lock poisoned; skipping reapply");
                return;
            }
        };
        if let Err(e) = apply_to_router(&self.router, &table) {
            tracing::error!(error = %e, "Failed to apply routing table to router");
        }
    }
}

/// Create a Kubernetes client and spawn HTTPRoute watcher task(s) driving
/// `reconciler`. An empty `namespaces` watches all namespaces (one watcher);
/// otherwise one watcher is spawned per namespace.
pub async fn start(reconciler: Arc<RouteReconciler>, namespaces: Vec<String>) -> crate::Result<()> {
    let client = Client::try_default().await?;

    if namespaces.is_empty() {
        let rec = Arc::clone(&reconciler);
        tokio::spawn(async move { run_http_route_watcher(client, rec, None).await });
    } else {
        for ns in namespaces {
            let rec = Arc::clone(&reconciler);
            let client = client.clone();
            tokio::spawn(async move { run_http_route_watcher(client, rec, Some(ns)).await });
        }
    }

    Ok(())
}

/// Watch `HTTPRoute` resources and drive `reconciler`. Runs until the watch
/// stream ends (typically the lifetime of the process).
///
/// `namespace` of `None` watches all namespaces (requires cluster-scoped RBAC);
/// `Some(ns)` restricts to a single namespace.
pub async fn run_http_route_watcher(
    client: Client,
    reconciler: Arc<RouteReconciler>,
    namespace: Option<String>,
) {
    let api: Api<HTTPRoute> = match &namespace {
        Some(ns) => Api::namespaced(client, ns),
        None => Api::all(client),
    };

    tracing::info!(?namespace, "Starting HTTPRoute watcher");

    let mut stream = std::pin::pin!(watcher(api, WatcherConfig::default()));
    loop {
        match stream.try_next().await {
            Ok(Some(event)) => handle_event(&reconciler, event),
            Ok(None) => break,
            Err(e) => tracing::error!(error = %e, "HTTPRoute watch error"),
        }
    }

    tracing::warn!("HTTPRoute watcher stream ended");
}

fn handle_event(reconciler: &RouteReconciler, event: watcher::Event<HTTPRoute>) {
    match event {
        watcher::Event::Apply(route) | watcher::Event::InitApply(route) => {
            let name = route.metadata.name.clone().unwrap_or_default();
            let namespace = route.metadata.namespace.clone().unwrap_or_default();
            if name.is_empty() {
                return;
            }
            tracing::debug!(route = %name, namespace = %namespace, "HTTPRoute applied");
            reconciler.upsert_httproute(&name, &namespace, &route.spec);
        }
        watcher::Event::Delete(route) => {
            let name = route.metadata.name.clone().unwrap_or_default();
            let namespace = route.metadata.namespace.clone().unwrap_or_default();
            tracing::debug!(route = %name, namespace = %namespace, "HTTPRoute deleted");
            reconciler.remove_httproute(&name, &namespace);
        }
        watcher::Event::Init | watcher::Event::InitDone => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway_api::{HttpBackendRef, HttpPathMatch, HttpRouteMatch, HttpRouteRule};
    use http::Method;

    fn httproute_spec(path: &str, svc: &str, port: u16) -> HTTPRouteSpec {
        HTTPRouteSpec {
            parent_refs: vec![],
            hostnames: vec![],
            rules: vec![HttpRouteRule {
                matches: vec![HttpRouteMatch {
                    path: Some(HttpPathMatch {
                        path_type: "PathPrefix".into(),
                        value: path.into(),
                    }),
                    method: Some("GET".into()),
                    headers: vec![],
                }],
                filters: vec![],
                backend_refs: vec![HttpBackendRef {
                    name: svc.into(),
                    namespace: None,
                    port: Some(port),
                    weight: None,
                }],
            }],
        }
    }

    #[test]
    fn upsert_then_remove_updates_router() {
        let router = Arc::new(Router::new());
        let rec = RouteReconciler::new(Arc::clone(&router));

        rec.upsert_httproute(
            "api-route",
            "default",
            &httproute_spec("/api", "api-svc", 8080),
        );
        assert!(
            router.match_route(&Method::GET, "/api").is_ok(),
            "exact prefix matches"
        );
        assert!(
            router.match_route(&Method::GET, "/api/users").is_ok(),
            "subpath matches via wildcard"
        );

        rec.remove_httproute("api-route", "default");
        assert!(
            router.match_route(&Method::GET, "/api").is_err(),
            "route removed after delete"
        );
    }

    #[test]
    fn static_routes_survive_gateway_reconcile() {
        let router = Arc::new(Router::new());
        let rec = RouteReconciler::new(Arc::clone(&router));

        rec.seed_static(
            vec![IntermediateRoute::new(
                Method::GET,
                "/static",
                "static-up",
                RouteSource::Static,
            )],
            vec![UpstreamCluster::new("static-up")],
        );
        assert!(router.match_route(&Method::GET, "/static").is_ok());

        rec.upsert_httproute(
            "api-route",
            "default",
            &httproute_spec("/api", "api-svc", 8080),
        );
        assert!(
            router.match_route(&Method::GET, "/static").is_ok(),
            "static preserved"
        );
        assert!(
            router.match_route(&Method::GET, "/api").is_ok(),
            "gateway route added"
        );

        rec.remove_httproute("api-route", "default");
        assert!(
            router.match_route(&Method::GET, "/static").is_ok(),
            "static still present"
        );
        assert!(
            router.match_route(&Method::GET, "/api").is_err(),
            "gateway route gone"
        );
    }
}
