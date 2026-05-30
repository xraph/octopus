//! Reconciler that turns Kubernetes routing resources into live router state.
//!
//! [`RouteReconciler`] owns a [`RouteStore`] and the live [`Router`]. As
//! resources are upserted/removed it re-translates, merges every source, and
//! applies the result to the router — so the router always reflects the merged
//! intent of static config + Gateway API + Octopus CRDs.

use crate::apply::apply_to_router;
use crate::crds::{
    OctopusPolicy, OctopusPolicySpec, OctopusRoute, OctopusRouteSpec, OctopusUpstream,
    OctopusUpstreamSpec,
};
use crate::gateway_api::{GRPCRoute, GRPCRouteSpec, Gateway, HTTPRoute, HTTPRouteSpec};
use crate::ir::{IntermediateRoute, RouteSource, RouteStore, SourceKey};
use crate::policy::{apply_overlays, PolicyOverlay};
use crate::refgrant::ReferenceGrant;
use crate::tls::TlsReconciler;
use crate::translate::{
    grpcroute_to_route, httproute_to_route, octopus_route_to_route, octopus_upstream_to_cluster,
};
use futures::TryStreamExt;
use k8s_openapi::api::core::v1::Secret;
use kube::{
    api::Api,
    runtime::{watcher, watcher::Config as WatcherConfig},
    Client,
};
use octopus_core::UpstreamCluster;
use octopus_router::Router;
use octopus_tls::SwappableTlsAcceptor;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Reconciles routing resources into the live [`Router`].
pub struct RouteReconciler {
    store: Mutex<RouteStore>,
    /// Policy overlays keyed by `namespace/name`.
    policies: Mutex<HashMap<String, PolicyOverlay>>,
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
            policies: Mutex::new(HashMap::new()),
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

    /// Upsert a `GRPCRoute` and re-apply the merged routing table.
    pub fn upsert_grpcroute(&self, name: &str, namespace: &str, spec: &GRPCRouteSpec) {
        let (routes, upstreams) = grpcroute_to_route(name, namespace, spec);
        if let Ok(mut store) = self.store.lock() {
            store.insert(
                SourceKey::new(RouteSource::GatewayApi, format!("grpc/{namespace}/{name}")),
                routes,
                upstreams,
            );
        }
        self.reapply();
    }

    /// Remove a `GRPCRoute` and re-apply the merged routing table.
    pub fn remove_grpcroute(&self, name: &str, namespace: &str) {
        if let Ok(mut store) = self.store.lock() {
            store.remove(&SourceKey::new(
                RouteSource::GatewayApi,
                format!("grpc/{namespace}/{name}"),
            ));
        }
        self.reapply();
    }

    /// Upsert an `OctopusRoute` and re-apply.
    pub fn upsert_octopus_route(&self, name: &str, namespace: &str, spec: &OctopusRouteSpec) {
        let routes = octopus_route_to_route(name, namespace, spec);
        if let Ok(mut store) = self.store.lock() {
            store.insert(
                SourceKey::new(RouteSource::OctopusRoute, format!("{namespace}/{name}")),
                routes,
                Vec::new(),
            );
        }
        self.reapply();
    }

    /// Remove an `OctopusRoute` and re-apply.
    pub fn remove_octopus_route(&self, name: &str, namespace: &str) {
        if let Ok(mut store) = self.store.lock() {
            store.remove(&SourceKey::new(
                RouteSource::OctopusRoute,
                format!("{namespace}/{name}"),
            ));
        }
        self.reapply();
    }

    /// Upsert an `OctopusUpstream` and re-apply.
    pub fn upsert_octopus_upstream(&self, name: &str, namespace: &str, spec: &OctopusUpstreamSpec) {
        let cluster = octopus_upstream_to_cluster(name, namespace, spec);
        if let Ok(mut store) = self.store.lock() {
            // Stored as a routeless entry contributing only the upstream cluster.
            store.insert(
                SourceKey::new(
                    RouteSource::OctopusRoute,
                    format!("upstream/{namespace}/{name}"),
                ),
                Vec::new(),
                vec![cluster],
            );
        }
        self.reapply();
    }

    /// Remove an `OctopusUpstream` and re-apply.
    pub fn remove_octopus_upstream(&self, name: &str, namespace: &str) {
        if let Ok(mut store) = self.store.lock() {
            store.remove(&SourceKey::new(
                RouteSource::OctopusRoute,
                format!("upstream/{namespace}/{name}"),
            ));
        }
        self.reapply();
    }

    /// Upsert an `OctopusPolicy` and re-apply.
    pub fn upsert_policy(&self, name: &str, namespace: &str, spec: &OctopusPolicySpec) {
        let key = format!("{namespace}/{name}");
        if let Ok(mut policies) = self.policies.lock() {
            match PolicyOverlay::from_spec(namespace, spec) {
                Some(overlay) => {
                    policies.insert(key, overlay);
                }
                None => {
                    // Unsupported target kind — drop any prior overlay.
                    policies.remove(&key);
                    tracing::warn!(
                        policy = %name,
                        kind = %spec.target_ref.kind,
                        "OctopusPolicy targets an unsupported kind; ignoring"
                    );
                }
            }
        }
        self.reapply();
    }

    /// Remove an `OctopusPolicy` and re-apply.
    pub fn remove_policy(&self, name: &str, namespace: &str) {
        if let Ok(mut policies) = self.policies.lock() {
            policies.remove(&format!("{namespace}/{name}"));
        }
        self.reapply();
    }

    /// Merge all sources, apply policy overlays, and program the live router.
    fn reapply(&self) {
        let mut table = match self.store.lock() {
            Ok(store) => store.merge(),
            Err(_) => {
                tracing::error!("RouteStore lock poisoned; skipping reapply");
                return;
            }
        };

        if let Ok(policies) = self.policies.lock() {
            let overlays: Vec<PolicyOverlay> = policies.values().cloned().collect();
            apply_overlays(&mut table.routes, &overlays);
        }

        if let Err(e) = apply_to_router(&self.router, &table) {
            tracing::error!(error = %e, "Failed to apply routing table to router");
        }
    }
}

/// Create a Kubernetes client and spawn watchers for HTTPRoute and the Octopus
/// CRDs, driving `reconciler`. An empty `namespaces` watches all namespaces
/// (requires cluster-scoped RBAC); otherwise watchers are spawned per namespace.
pub async fn start(
    reconciler: Arc<RouteReconciler>,
    namespaces: Vec<String>,
    tls_acceptor: Option<SwappableTlsAcceptor>,
) -> crate::Result<()> {
    let client = Client::try_default().await?;

    let scopes: Vec<Option<String>> = if namespaces.is_empty() {
        vec![None]
    } else {
        namespaces.into_iter().map(Some).collect()
    };

    // When the gateway listener terminates operator-managed TLS, drive a
    // TlsReconciler from Gateway + Secret watches.
    let tls = tls_acceptor.map(|a| Arc::new(TlsReconciler::new(a)));

    for scope in &scopes {
        spawn_watchers(client.clone(), Arc::clone(&reconciler), scope.clone());
        if let Some(ref tls) = tls {
            spawn_tls_watchers(client.clone(), Arc::clone(tls), scope.clone());
        }
    }

    Ok(())
}

/// Spawn Gateway + Secret watchers driving a [`TlsReconciler`] for one scope.
fn spawn_tls_watchers(client: Client, tls: Arc<TlsReconciler>, namespace: Option<String>) {
    let gateways: Api<Gateway> = match &namespace {
        Some(ns) => Api::namespaced(client.clone(), ns),
        None => Api::all(client.clone()),
    };
    let secrets: Api<Secret> = match &namespace {
        Some(ns) => Api::namespaced(client.clone(), ns),
        None => Api::all(client.clone()),
    };
    let grants: Api<ReferenceGrant> = match &namespace {
        Some(ns) => Api::namespaced(client.clone(), ns),
        None => Api::all(client),
    };
    tokio::spawn({
        let tls = Arc::clone(&tls);
        async move { run_watcher(gateways, tls, "Gateway", on_gateway).await }
    });
    tokio::spawn({
        let tls = Arc::clone(&tls);
        async move { run_watcher(secrets, tls, "Secret(TLS)", on_tls_secret).await }
    });
    tokio::spawn(
        async move { run_watcher(grants, tls, "ReferenceGrant", on_reference_grant).await },
    );
}

/// Spawn one watcher per resource type for a single namespace scope.
fn spawn_watchers(client: Client, reconciler: Arc<RouteReconciler>, namespace: Option<String>) {
    fn api<K>(client: &Client, namespace: &Option<String>) -> Api<K>
    where
        K: kube::Resource<Scope = kube::core::NamespaceResourceScope>,
        <K as kube::Resource>::DynamicType: Default,
    {
        match namespace {
            Some(ns) => Api::namespaced(client.clone(), ns),
            None => Api::all(client.clone()),
        }
    }

    tokio::spawn({
        let (api, rec) = (
            api::<HTTPRoute>(&client, &namespace),
            Arc::clone(&reconciler),
        );
        async move { run_watcher(api, rec, "HTTPRoute", on_http_route).await }
    });
    tokio::spawn({
        let (api, rec) = (
            api::<GRPCRoute>(&client, &namespace),
            Arc::clone(&reconciler),
        );
        async move { run_watcher(api, rec, "GRPCRoute", on_grpcroute).await }
    });
    tokio::spawn({
        let (api, rec) = (
            api::<OctopusRoute>(&client, &namespace),
            Arc::clone(&reconciler),
        );
        async move { run_watcher(api, rec, "OctopusRoute", on_octopus_route).await }
    });
    tokio::spawn({
        let (api, rec) = (
            api::<OctopusUpstream>(&client, &namespace),
            Arc::clone(&reconciler),
        );
        async move { run_watcher(api, rec, "OctopusUpstream", on_octopus_upstream).await }
    });
    tokio::spawn({
        let (api, rec) = (
            api::<OctopusPolicy>(&client, &namespace),
            Arc::clone(&reconciler),
        );
        async move { run_watcher(api, rec, "OctopusPolicy", on_octopus_policy).await }
    });
}

/// Drive a context `ctx` from a resource's watch stream until it ends.
async fn run_watcher<K, C>(
    api: Api<K>,
    ctx: Arc<C>,
    label: &'static str,
    handler: fn(&C, watcher::Event<K>),
) where
    K: kube::Resource + Clone + std::fmt::Debug + serde::de::DeserializeOwned + Send + 'static,
    <K as kube::Resource>::DynamicType: Default + Clone + Eq + std::hash::Hash,
    C: Send + Sync + 'static,
{
    tracing::info!(resource = label, "Starting watcher");
    let mut stream = std::pin::pin!(watcher(api, WatcherConfig::default()));
    loop {
        match stream.try_next().await {
            Ok(Some(event)) => handler(&ctx, event),
            Ok(None) => break,
            Err(e) => tracing::error!(resource = label, error = %e, "watch error"),
        }
    }
    tracing::warn!(resource = label, "watcher stream ended");
}

/// Extract `(name, namespace)` from a namespaced resource.
fn ident<K: kube::Resource>(obj: &K) -> Option<(String, String)> {
    let meta = obj.meta();
    match (&meta.name, &meta.namespace) {
        (Some(name), Some(ns)) => Some((name.clone(), ns.clone())),
        _ => None,
    }
}

fn on_http_route(rec: &RouteReconciler, event: watcher::Event<HTTPRoute>) {
    match event {
        watcher::Event::Apply(o) | watcher::Event::InitApply(o) => {
            if let Some((name, ns)) = ident(&o) {
                rec.upsert_httproute(&name, &ns, &o.spec);
            }
        }
        watcher::Event::Delete(o) => {
            if let Some((name, ns)) = ident(&o) {
                rec.remove_httproute(&name, &ns);
            }
        }
        watcher::Event::Init | watcher::Event::InitDone => {}
    }
}

fn on_gateway(tls: &TlsReconciler, event: watcher::Event<Gateway>) {
    match event {
        watcher::Event::Apply(o) | watcher::Event::InitApply(o) => {
            if let Some((name, ns)) = ident(&o) {
                tls.set_gateway(&name, &ns, &o.spec);
            }
        }
        watcher::Event::Delete(o) => {
            if let Some((name, ns)) = ident(&o) {
                tls.remove_gateway(&name, &ns);
            }
        }
        watcher::Event::Init | watcher::Event::InitDone => {}
    }
}

fn on_tls_secret(tls: &TlsReconciler, event: watcher::Event<Secret>) {
    match event {
        watcher::Event::Apply(o) | watcher::Event::InitApply(o) => {
            if let Some((name, ns)) = ident(&o) {
                if let Some((cert, key)) = tls_secret_payload(&o) {
                    tls.set_secret(&name, &ns, cert, key);
                }
            }
        }
        watcher::Event::Delete(o) => {
            if let Some((name, ns)) = ident(&o) {
                tls.remove_secret(&name, &ns);
            }
        }
        watcher::Event::Init | watcher::Event::InitDone => {}
    }
}

fn on_reference_grant(tls: &TlsReconciler, event: watcher::Event<ReferenceGrant>) {
    match event {
        watcher::Event::Apply(o) | watcher::Event::InitApply(o) => {
            if let Some((name, ns)) = ident(&o) {
                tls.set_grant(&name, &ns, o.spec.clone());
            }
        }
        watcher::Event::Delete(o) => {
            if let Some((name, ns)) = ident(&o) {
                tls.remove_grant(&name, &ns);
            }
        }
        watcher::Event::Init | watcher::Event::InitDone => {}
    }
}

/// Extract `(tls.crt, tls.key)` PEM bytes from a `kubernetes.io/tls` Secret.
fn tls_secret_payload(secret: &Secret) -> Option<(Vec<u8>, Vec<u8>)> {
    if secret.type_.as_deref() != Some("kubernetes.io/tls") {
        return None;
    }
    let data = secret.data.as_ref()?;
    let cert = data.get("tls.crt")?.0.clone();
    let key = data.get("tls.key")?.0.clone();
    Some((cert, key))
}

fn on_grpcroute(rec: &RouteReconciler, event: watcher::Event<GRPCRoute>) {
    match event {
        watcher::Event::Apply(o) | watcher::Event::InitApply(o) => {
            if let Some((name, ns)) = ident(&o) {
                rec.upsert_grpcroute(&name, &ns, &o.spec);
            }
        }
        watcher::Event::Delete(o) => {
            if let Some((name, ns)) = ident(&o) {
                rec.remove_grpcroute(&name, &ns);
            }
        }
        watcher::Event::Init | watcher::Event::InitDone => {}
    }
}

fn on_octopus_route(rec: &RouteReconciler, event: watcher::Event<OctopusRoute>) {
    match event {
        watcher::Event::Apply(o) | watcher::Event::InitApply(o) => {
            if let Some((name, ns)) = ident(&o) {
                rec.upsert_octopus_route(&name, &ns, &o.spec);
            }
        }
        watcher::Event::Delete(o) => {
            if let Some((name, ns)) = ident(&o) {
                rec.remove_octopus_route(&name, &ns);
            }
        }
        watcher::Event::Init | watcher::Event::InitDone => {}
    }
}

fn on_octopus_upstream(rec: &RouteReconciler, event: watcher::Event<OctopusUpstream>) {
    match event {
        watcher::Event::Apply(o) | watcher::Event::InitApply(o) => {
            if let Some((name, ns)) = ident(&o) {
                rec.upsert_octopus_upstream(&name, &ns, &o.spec);
            }
        }
        watcher::Event::Delete(o) => {
            if let Some((name, ns)) = ident(&o) {
                rec.remove_octopus_upstream(&name, &ns);
            }
        }
        watcher::Event::Init | watcher::Event::InitDone => {}
    }
}

fn on_octopus_policy(rec: &RouteReconciler, event: watcher::Event<OctopusPolicy>) {
    match event {
        watcher::Event::Apply(o) | watcher::Event::InitApply(o) => {
            if let Some((name, ns)) = ident(&o) {
                rec.upsert_policy(&name, &ns, &o.spec);
            }
        }
        watcher::Event::Delete(o) => {
            if let Some((name, ns)) = ident(&o) {
                rec.remove_policy(&name, &ns);
            }
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

    #[test]
    fn octopus_route_and_upstream_and_policy_flow() {
        use crate::crds::{
            OctopusPolicySpec, OctopusRouteSpec, OctopusUpstreamSpec, PolicyTargetRef,
            UpstreamTarget,
        };

        let router = Arc::new(Router::new());
        let rec = RouteReconciler::new(Arc::clone(&router));

        // Upstream + route (no inline auth).
        rec.upsert_octopus_upstream(
            "orders-up",
            "shop",
            &OctopusUpstreamSpec {
                targets: vec![UpstreamTarget {
                    host: "10.0.0.1".into(),
                    port: 8080,
                    weight: None,
                }],
                lb_strategy: None,
            },
        );
        rec.upsert_octopus_route(
            "orders",
            "shop",
            &OctopusRouteSpec {
                path: "/orders".into(),
                methods: vec!["GET".into()],
                upstream: "orders-up".into(),
                ..Default::default()
            },
        );

        let m = router.match_route(&Method::GET, "/orders").unwrap();
        assert_eq!(m.route.upstream_name, "orders-up");
        assert_eq!(m.route.auth_provider, None, "no inline auth yet");

        // Policy attaches auth to the route.
        rec.upsert_policy(
            "orders-auth",
            "shop",
            &OctopusPolicySpec {
                target_ref: PolicyTargetRef {
                    group: "gateway.octopus.io".into(),
                    kind: "OctopusRoute".into(),
                    name: "orders".into(),
                    section_name: None,
                },
                auth_provider: Some("jwt".into()),
                ..Default::default()
            },
        );
        let m = router.match_route(&Method::GET, "/orders").unwrap();
        assert_eq!(
            m.route.auth_provider.as_deref(),
            Some("jwt"),
            "policy enriched the route"
        );

        // Removing the policy reverts the enrichment.
        rec.remove_policy("orders-auth", "shop");
        let m = router.match_route(&Method::GET, "/orders").unwrap();
        assert_eq!(m.route.auth_provider, None, "policy removal reverts auth");
    }
}
