//! Reconciler that turns Kubernetes routing resources into live router state.
//!
//! [`RouteReconciler`] owns a [`RouteStore`] and the live [`Router`]. As
//! resources are upserted/removed it re-translates, merges every source, and
//! applies the result to the router — so the router always reflects the merged
//! intent of static config + Gateway API + Octopus CRDs.

use crate::apply::apply_to_router;
use crate::crds::{
    ConventionSpec, OctopusPolicy, OctopusPolicySpec, OctopusRoute, OctopusRouteSpec,
    OctopusUpstream, OctopusUpstreamSpec,
};
use crate::gateway_api::{
    GRPCRoute, GRPCRouteSpec, Gateway, GatewaySpec, HTTPRoute, HTTPRouteSpec, ParentRef,
};
use crate::ir::{IntermediateRoute, RouteSource, RouteStore, SourceKey};
use crate::policy::{apply_overlays, PolicyOverlay};
use crate::refgrant::{is_permitted, RefRequest, ReferenceGrant, ReferenceGrantSpec};
use crate::tls::TlsReconciler;
use crate::translate::{
    grpcroute_to_route, httproute_to_route, octopus_route_to_route, octopus_upstream_to_cluster,
};
use futures::TryStreamExt;
use k8s_openapi::api::core::v1::{ConfigMap, Secret};
use crate::status::{build_status, ReconcileOutcome};
use crate::validate::{validate_policy, validate_route, validate_upstream};
use kube::{
    api::{Api, Patch, PatchParams},
    core::NamespaceResourceScope,
    runtime::{watcher, watcher::Config as WatcherConfig},
    Client,
};
use octopus_core::UpstreamCluster;
use octopus_router::Router;
use octopus_tls::SwappableTlsAcceptor;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, OnceLock};

/// Reconciles routing resources into the live [`Router`].
pub struct RouteReconciler {
    store: Mutex<RouteStore>,
    /// Policy overlays keyed by `namespace/name`.
    policies: Mutex<HashMap<String, PolicyOverlay>>,
    router: Arc<Router>,
    /// Raw OctopusRoute specs (`ns/name` → spec), retained so routes can be
    /// re-translated when a referenced ConfigMap or ReferenceGrant changes.
    octopus_routes: Mutex<HashMap<String, OctopusRouteSpec>>,
    /// ConfigMap data (`ns/name` → data) for resolving convention `script_ref`s.
    configmaps: Mutex<HashMap<String, BTreeMap<String, String>>>,
    /// ReferenceGrants (`ns/name` → (grant namespace, spec)) for cross-namespace
    /// ConfigMap script references.
    route_grants: Mutex<HashMap<String, (String, ReferenceGrantSpec)>>,
    /// Parent `Gateway` specs (`ns/name` → spec) for hostname intersection.
    gateways: Mutex<HashMap<String, GatewaySpec>>,
    /// Raw HTTPRoute/GRPCRoute specs retained for re-translation when a parent
    /// Gateway's listener hostnames change.
    httproutes: Mutex<HashMap<String, HTTPRouteSpec>>,
    grpcroutes: Mutex<HashMap<String, GRPCRouteSpec>>,
    /// Kubernetes client, set once at startup. Enables `.status` writeback;
    /// when absent (e.g. in unit tests) status patches are silently skipped.
    client: OnceLock<Client>,
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
            octopus_routes: Mutex::new(HashMap::new()),
            configmaps: Mutex::new(HashMap::new()),
            route_grants: Mutex::new(HashMap::new()),
            gateways: Mutex::new(HashMap::new()),
            httproutes: Mutex::new(HashMap::new()),
            grpcroutes: Mutex::new(HashMap::new()),
            client: OnceLock::new(),
        }
    }

    /// Provide the Kubernetes client used for `.status` writeback. Called once
    /// at startup; subsequent calls are ignored.
    pub fn set_client(&self, client: Client) {
        let _ = self.client.set(client);
    }

    /// Patch the `.status` subresource of namespaced resource `K` named
    /// `namespace/name` with the condition for `outcome`. A no-op when no client
    /// has been set. Runs the actual patch on a spawned task so callers stay sync.
    fn write_status<K>(
        &self,
        name: &str,
        namespace: &str,
        generation: Option<i64>,
        outcome: &ReconcileOutcome,
    ) where
        K: kube::Resource<Scope = NamespaceResourceScope>
            + Clone
            + std::fmt::Debug
            + serde::de::DeserializeOwned
            + 'static,
        <K as kube::Resource>::DynamicType: Default,
    {
        let Some(client) = self.client.get().cloned() else {
            return;
        };
        let now = k8s_openapi::chrono::Utc::now().to_rfc3339();
        let status = build_status(generation, outcome, &now);
        let (name, namespace) = (name.to_string(), namespace.to_string());
        let kind = std::any::type_name::<K>();
        tokio::spawn(async move {
            let api: Api<K> = Api::namespaced(client, &namespace);
            let patch = serde_json::json!({ "status": status });
            if let Err(e) = api
                .patch_status(&name, &PatchParams::default(), &Patch::Merge(&patch))
                .await
            {
                tracing::warn!(resource = kind, name = %name, namespace = %namespace, error = %e, "status writeback failed");
            }
        });
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
        if let Ok(mut m) = self.httproutes.lock() {
            m.insert(format!("{namespace}/{name}"), spec.clone());
        }
        self.translate_httproute(name, namespace, spec);
        self.reapply();
    }

    fn translate_httproute(&self, name: &str, namespace: &str, spec: &HTTPRouteSpec) {
        let listeners = self.listener_hostnames_for(&spec.parent_refs, namespace);
        let (routes, upstreams) = httproute_to_route(name, namespace, spec, &listeners);
        if let Ok(mut store) = self.store.lock() {
            store.insert(
                SourceKey::new(RouteSource::GatewayApi, format!("{namespace}/{name}")),
                routes,
                upstreams,
            );
        }
    }

    /// Remove an `HTTPRoute` and re-apply the merged routing table.
    pub fn remove_httproute(&self, name: &str, namespace: &str) {
        if let Ok(mut m) = self.httproutes.lock() {
            m.remove(&format!("{namespace}/{name}"));
        }
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
        if let Ok(mut m) = self.grpcroutes.lock() {
            m.insert(format!("{namespace}/{name}"), spec.clone());
        }
        self.translate_grpcroute(name, namespace, spec);
        self.reapply();
    }

    fn translate_grpcroute(&self, name: &str, namespace: &str, spec: &GRPCRouteSpec) {
        let listeners = self.listener_hostnames_for(&spec.parent_refs, namespace);
        let (routes, upstreams) = grpcroute_to_route(name, namespace, spec, &listeners);
        if let Ok(mut store) = self.store.lock() {
            store.insert(
                SourceKey::new(RouteSource::GatewayApi, format!("grpc/{namespace}/{name}")),
                routes,
                upstreams,
            );
        }
    }

    /// Remove a `GRPCRoute` and re-apply the merged routing table.
    pub fn remove_grpcroute(&self, name: &str, namespace: &str) {
        if let Ok(mut m) = self.grpcroutes.lock() {
            m.remove(&format!("{namespace}/{name}"));
        }
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
        if let Ok(mut m) = self.octopus_routes.lock() {
            m.insert(format!("{namespace}/{name}"), spec.clone());
        }
        self.translate_octopus_route(name, namespace, spec);
        self.reapply();
    }

    /// Translate one OctopusRoute into the store, resolving any ConfigMap-backed
    /// convention `script_ref` to an inline script first. Does not re-apply.
    fn translate_octopus_route(&self, name: &str, namespace: &str, spec: &OctopusRouteSpec) {
        let effective = self.with_resolved_script(namespace, spec);
        let routes = octopus_route_to_route(name, namespace, &effective);
        if let Ok(mut store) = self.store.lock() {
            store.insert(
                SourceKey::new(RouteSource::OctopusRoute, format!("{namespace}/{name}")),
                routes,
                Vec::new(),
            );
        }
    }

    /// Return a spec whose convention script is populated from its `script_ref`
    /// ConfigMap when applicable; otherwise the spec unchanged.
    fn with_resolved_script(&self, route_ns: &str, spec: &OctopusRouteSpec) -> OctopusRouteSpec {
        let Some(conv) = &spec.convention else {
            return spec.clone();
        };
        if conv.script.is_some() || conv.script_ref.is_none() {
            return spec.clone();
        }
        let grants_by_ns = self.grants_by_ns();
        let resolved = match self.configmaps.lock() {
            Ok(cms) => resolve_script_ref(conv, route_ns, &cms, &grants_by_ns),
            Err(_) => None,
        };
        match resolved {
            Some(script) => {
                let mut s = spec.clone();
                if let Some(c) = &mut s.convention {
                    c.script = Some(script);
                }
                s
            }
            None => spec.clone(),
        }
    }

    /// Group known ReferenceGrants by their (target) namespace for lookups.
    fn grants_by_ns(&self) -> HashMap<String, Vec<ReferenceGrantSpec>> {
        let mut out: HashMap<String, Vec<ReferenceGrantSpec>> = HashMap::new();
        if let Ok(grants) = self.route_grants.lock() {
            for (ns, spec) in grants.values() {
                out.entry(ns.clone()).or_default().push(spec.clone());
            }
        }
        out
    }

    /// Re-translate every retained OctopusRoute (e.g. after a ConfigMap or
    /// ReferenceGrant change) and re-apply once.
    fn reresolve_octopus_routes(&self) {
        let specs: Vec<(String, String)> = match self.octopus_routes.lock() {
            Ok(m) => m.keys().cloned().filter_map(|k| split_key(&k)).collect(),
            Err(_) => return,
        };
        for (ns, name) in specs {
            let spec = self
                .octopus_routes
                .lock()
                .ok()
                .and_then(|m| m.get(&format!("{ns}/{name}")).cloned());
            if let Some(spec) = spec {
                self.translate_octopus_route(&name, &ns, &spec);
            }
        }
        self.reapply();
    }

    /// Store/replace a ConfigMap's data and re-resolve dependent routes.
    pub fn set_configmap(&self, name: &str, namespace: &str, data: BTreeMap<String, String>) {
        if let Ok(mut m) = self.configmaps.lock() {
            m.insert(format!("{namespace}/{name}"), data);
        }
        self.reresolve_octopus_routes();
    }

    /// Drop a ConfigMap and re-resolve dependent routes.
    pub fn remove_configmap(&self, name: &str, namespace: &str) {
        if let Ok(mut m) = self.configmaps.lock() {
            m.remove(&format!("{namespace}/{name}"));
        }
        self.reresolve_octopus_routes();
    }

    /// Store/replace a ReferenceGrant and re-resolve dependent routes.
    pub fn set_route_grant(&self, name: &str, namespace: &str, spec: ReferenceGrantSpec) {
        if let Ok(mut m) = self.route_grants.lock() {
            m.insert(format!("{namespace}/{name}"), (namespace.to_string(), spec));
        }
        self.reresolve_octopus_routes();
    }

    /// Drop a ReferenceGrant and re-resolve dependent routes.
    pub fn remove_route_grant(&self, name: &str, namespace: &str) {
        if let Ok(mut m) = self.route_grants.lock() {
            m.remove(&format!("{namespace}/{name}"));
        }
        self.reresolve_octopus_routes();
    }

    /// Union of the hostnames of the listeners the given `parent_refs` attach to.
    /// Empty = unconstrained (no parent found yet, or listeners have no hostname).
    fn listener_hostnames_for(&self, parent_refs: &[ParentRef], route_ns: &str) -> Vec<String> {
        let Ok(gateways) = self.gateways.lock() else {
            return Vec::new();
        };
        let mut out: Vec<String> = Vec::new();
        for pr in parent_refs {
            let gw_ns = pr.namespace.as_deref().unwrap_or(route_ns);
            let Some(gw) = gateways.get(&format!("{gw_ns}/{}", pr.name)) else {
                continue;
            };
            for l in &gw.listeners {
                if pr.section_name.as_ref().is_some_and(|s| s != &l.name) {
                    continue;
                }
                if let Some(h) = &l.hostname {
                    if !out.contains(h) {
                        out.push(h.clone());
                    }
                }
            }
        }
        out
    }

    /// Store/replace a parent `Gateway` and re-translate dependent routes.
    pub fn set_gateway_route(&self, name: &str, namespace: &str, spec: GatewaySpec) {
        if let Ok(mut m) = self.gateways.lock() {
            m.insert(format!("{namespace}/{name}"), spec);
        }
        self.reresolve_gateway_routes();
    }

    /// Drop a parent `Gateway` and re-translate dependent routes.
    pub fn remove_gateway_route(&self, name: &str, namespace: &str) {
        if let Ok(mut m) = self.gateways.lock() {
            m.remove(&format!("{namespace}/{name}"));
        }
        self.reresolve_gateway_routes();
    }

    /// Re-translate every retained Gateway-API route (after a Gateway change),
    /// then re-apply once.
    fn reresolve_gateway_routes(&self) {
        let https: Vec<(String, String)> = self
            .httproutes
            .lock()
            .map(|m| m.keys().filter_map(|k| split_key(k)).collect())
            .unwrap_or_default();
        for (ns, name) in https {
            let spec = self
                .httproutes
                .lock()
                .ok()
                .and_then(|m| m.get(&format!("{ns}/{name}")).cloned());
            if let Some(spec) = spec {
                self.translate_httproute(&name, &ns, &spec);
            }
        }
        let grpcs: Vec<(String, String)> = self
            .grpcroutes
            .lock()
            .map(|m| m.keys().filter_map(|k| split_key(k)).collect())
            .unwrap_or_default();
        for (ns, name) in grpcs {
            let spec = self
                .grpcroutes
                .lock()
                .ok()
                .and_then(|m| m.get(&format!("{ns}/{name}")).cloned());
            if let Some(spec) = spec {
                self.translate_grpcroute(&name, &ns, &spec);
            }
        }
        self.reapply();
    }

    /// Remove an `OctopusRoute` and re-apply.
    pub fn remove_octopus_route(&self, name: &str, namespace: &str) {
        if let Ok(mut m) = self.octopus_routes.lock() {
            m.remove(&format!("{namespace}/{name}"));
        }
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
    // Hand the client to the reconciler so it can write `.status` back onto
    // reconciled Octopus resources.
    reconciler.set_client(client.clone());

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
    tokio::spawn({
        let (api, rec) = (
            api::<ConfigMap>(&client, &namespace),
            Arc::clone(&reconciler),
        );
        async move { run_watcher(api, rec, "ConfigMap", on_configmap).await }
    });
    tokio::spawn({
        let (api, rec) = (
            api::<ReferenceGrant>(&client, &namespace),
            Arc::clone(&reconciler),
        );
        async move { run_watcher(api, rec, "ReferenceGrant(routes)", on_route_grant).await }
    });
    tokio::spawn({
        let (api, rec) = (api::<Gateway>(&client, &namespace), Arc::clone(&reconciler));
        async move { run_watcher(api, rec, "Gateway(routes)", on_gateway_route).await }
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

/// Resolve a convention's `script_ref` against known ConfigMaps. Returns the
/// script text to inline, or `None` (inline script wins, no ref, ConfigMap/key
/// absent, or a cross-namespace ref without a permitting `ReferenceGrant`).
fn resolve_script_ref(
    convention: &ConventionSpec,
    route_ns: &str,
    configmaps: &HashMap<String, BTreeMap<String, String>>,
    grants_by_ns: &HashMap<String, Vec<ReferenceGrantSpec>>,
) -> Option<String> {
    if convention.script.is_some() {
        return None; // inline script wins
    }
    let r = convention.script_ref.as_ref()?;
    let cm_ns = r.namespace.as_deref().unwrap_or(route_ns);

    // Cross-namespace references require a permitting ReferenceGrant in the
    // ConfigMap's namespace; same-namespace needs none.
    if cm_ns != route_ns {
        let req = RefRequest {
            from_group: "gateway.octopus.io",
            from_kind: "OctopusRoute",
            from_namespace: route_ns,
            to_group: "",
            to_kind: "ConfigMap",
            to_name: &r.name,
        };
        let grants = grants_by_ns.get(cm_ns).map(Vec::as_slice).unwrap_or(&[]);
        if !is_permitted(grants, &req) {
            tracing::warn!(
                configmap = %format!("{cm_ns}/{}", r.name),
                from_ns = %route_ns,
                "cross-namespace script ConfigMap reference not permitted by any ReferenceGrant"
            );
            return None;
        }
    }

    configmaps
        .get(&format!("{cm_ns}/{}", r.name))?
        .get(&r.key)
        .cloned()
}

/// Split a `namespace/name` store key into its parts.
fn split_key(key: &str) -> Option<(String, String)> {
    key.split_once('/')
        .map(|(ns, name)| (ns.to_string(), name.to_string()))
}

fn on_configmap(rec: &RouteReconciler, event: watcher::Event<ConfigMap>) {
    match event {
        watcher::Event::Apply(o) | watcher::Event::InitApply(o) => {
            if let Some((name, ns)) = ident(&o) {
                rec.set_configmap(&name, &ns, o.data.clone().unwrap_or_default());
            }
        }
        watcher::Event::Delete(o) => {
            if let Some((name, ns)) = ident(&o) {
                rec.remove_configmap(&name, &ns);
            }
        }
        watcher::Event::Init | watcher::Event::InitDone => {}
    }
}

fn on_route_grant(rec: &RouteReconciler, event: watcher::Event<ReferenceGrant>) {
    match event {
        watcher::Event::Apply(o) | watcher::Event::InitApply(o) => {
            if let Some((name, ns)) = ident(&o) {
                rec.set_route_grant(&name, &ns, o.spec.clone());
            }
        }
        watcher::Event::Delete(o) => {
            if let Some((name, ns)) = ident(&o) {
                rec.remove_route_grant(&name, &ns);
            }
        }
        watcher::Event::Init | watcher::Event::InitDone => {}
    }
}

fn on_gateway_route(rec: &RouteReconciler, event: watcher::Event<Gateway>) {
    match event {
        watcher::Event::Apply(o) | watcher::Event::InitApply(o) => {
            if let Some((name, ns)) = ident(&o) {
                rec.set_gateway_route(&name, &ns, o.spec.clone());
            }
        }
        watcher::Event::Delete(o) => {
            if let Some((name, ns)) = ident(&o) {
                rec.remove_gateway_route(&name, &ns);
            }
        }
        watcher::Event::Init | watcher::Event::InitDone => {}
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
                let outcome = validate_route(&o.spec);
                if outcome == ReconcileOutcome::Accepted {
                    rec.upsert_octopus_route(&name, &ns, &o.spec);
                } else {
                    rec.remove_octopus_route(&name, &ns);
                }
                rec.write_status::<OctopusRoute>(&name, &ns, o.metadata.generation, &outcome);
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
                let outcome = validate_upstream(&o.spec);
                if outcome == ReconcileOutcome::Accepted {
                    rec.upsert_octopus_upstream(&name, &ns, &o.spec);
                } else {
                    rec.remove_octopus_upstream(&name, &ns);
                }
                rec.write_status::<OctopusUpstream>(&name, &ns, o.metadata.generation, &outcome);
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
                let outcome = validate_policy(&o.spec);
                if outcome == ReconcileOutcome::Accepted {
                    rec.upsert_policy(&name, &ns, &o.spec);
                } else {
                    rec.remove_policy(&name, &ns);
                }
                rec.write_status::<OctopusPolicy>(&name, &ns, o.metadata.generation, &outcome);
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
    use crate::crds::ScriptConfigMapRef;
    use crate::gateway_api::{
        GatewayListener, HttpBackendRef, HttpPathMatch, HttpRouteMatch, HttpRouteRule,
    };
    use crate::refgrant::{ReferenceGrantFrom, ReferenceGrantTo};
    use http::Method;

    #[test]
    fn httproute_hostnames_intersect_parent_gateway_listener() {
        let router = Arc::new(Router::new());
        let rec = RouteReconciler::new(Arc::clone(&router));

        // A Gateway "gw" in ns "infra" with a wildcard listener.
        rec.set_gateway_route(
            "gw",
            "infra",
            GatewaySpec {
                gateway_class_name: "octopus".into(),
                listeners: vec![GatewayListener {
                    name: "https".into(),
                    hostname: Some("*.acme.com".into()),
                    port: 443,
                    protocol: "HTTPS".into(),
                    tls: None,
                }],
            },
        );

        // An HTTPRoute attaching to gw, declaring one in-scope and one out-of-scope host.
        let mut spec = httproute_spec("/", "svc", 80);
        spec.parent_refs = vec![ParentRef {
            name: "gw".into(),
            namespace: Some("infra".into()),
            section_name: None,
        }];
        spec.hostnames = vec!["api.acme.com".into(), "evil.com".into()];
        rec.upsert_httproute("r", "default", &spec);

        // Only the host within the listener's wildcard attaches.
        assert!(router
            .match_route("api.acme.com", &Method::GET, "/")
            .is_ok());
        assert!(router.match_route("evil.com", &Method::GET, "/").is_err());
    }

    fn conv_with_ref(name: &str, key: &str, namespace: Option<&str>) -> ConventionSpec {
        ConventionSpec {
            base_domain: "platform.com".into(),
            layout: vec!["service".into(), "namespace".into()],
            default_service: None,
            port: None,
            script: None,
            script_ref: Some(ScriptConfigMapRef {
                name: name.into(),
                key: key.into(),
                namespace: namespace.map(Into::into),
            }),
            backend_strategy: None,
        }
    }

    #[test]
    fn script_ref_resolves_same_namespace() {
        let mut cms = HashMap::new();
        let mut data = BTreeMap::new();
        data.insert(
            "resolve.rhai".to_string(),
            "#{ namespace: \"x\" }".to_string(),
        );
        cms.insert("acme/scripts".to_string(), data);

        let conv = conv_with_ref("scripts", "resolve.rhai", None);
        let got = resolve_script_ref(&conv, "acme", &cms, &HashMap::new());
        assert_eq!(got.as_deref(), Some("#{ namespace: \"x\" }"));
    }

    #[test]
    fn script_ref_missing_configmap_or_key_is_none() {
        let conv = conv_with_ref("scripts", "resolve.rhai", None);
        assert_eq!(
            resolve_script_ref(&conv, "acme", &HashMap::new(), &HashMap::new()),
            None
        );

        let mut cms = HashMap::new();
        cms.insert("acme/scripts".to_string(), BTreeMap::new()); // present, wrong key
        assert_eq!(
            resolve_script_ref(&conv, "acme", &cms, &HashMap::new()),
            None
        );
    }

    #[test]
    fn inline_script_takes_precedence_over_ref() {
        let mut conv = conv_with_ref("scripts", "resolve.rhai", None);
        conv.script = Some("inline".into());
        let mut cms = HashMap::new();
        let mut data = BTreeMap::new();
        data.insert("resolve.rhai".to_string(), "from-cm".to_string());
        cms.insert("acme/scripts".to_string(), data);
        assert_eq!(
            resolve_script_ref(&conv, "acme", &cms, &HashMap::new()),
            None
        );
    }

    #[test]
    fn cross_namespace_ref_requires_grant() {
        let mut cms = HashMap::new();
        let mut data = BTreeMap::new();
        data.insert("k".to_string(), "script".to_string());
        cms.insert("shared/scripts".to_string(), data);
        let conv = conv_with_ref("scripts", "k", Some("shared"));

        // No grant → denied.
        assert_eq!(
            resolve_script_ref(&conv, "acme", &cms, &HashMap::new()),
            None
        );

        // Grant in the target ns permitting OctopusRoute@acme → ConfigMap.
        let mut grants = HashMap::new();
        grants.insert(
            "shared".to_string(),
            vec![ReferenceGrantSpec {
                from: vec![ReferenceGrantFrom {
                    group: "gateway.octopus.io".into(),
                    kind: "OctopusRoute".into(),
                    namespace: "acme".into(),
                }],
                to: vec![ReferenceGrantTo {
                    group: "".into(),
                    kind: "ConfigMap".into(),
                    name: None,
                }],
            }],
        );
        assert_eq!(
            resolve_script_ref(&conv, "acme", &cms, &grants).as_deref(),
            Some("script")
        );
    }

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
            router
                .match_route("example.com", &Method::GET, "/api")
                .is_ok(),
            "exact prefix matches"
        );
        assert!(
            router
                .match_route("example.com", &Method::GET, "/api/users")
                .is_ok(),
            "subpath matches via wildcard"
        );

        rec.remove_httproute("api-route", "default");
        assert!(
            router
                .match_route("example.com", &Method::GET, "/api")
                .is_err(),
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
        assert!(router
            .match_route("example.com", &Method::GET, "/static")
            .is_ok());

        rec.upsert_httproute(
            "api-route",
            "default",
            &httproute_spec("/api", "api-svc", 8080),
        );
        assert!(
            router
                .match_route("example.com", &Method::GET, "/static")
                .is_ok(),
            "static preserved"
        );
        assert!(
            router
                .match_route("example.com", &Method::GET, "/api")
                .is_ok(),
            "gateway route added"
        );

        rec.remove_httproute("api-route", "default");
        assert!(
            router
                .match_route("example.com", &Method::GET, "/static")
                .is_ok(),
            "static still present"
        );
        assert!(
            router
                .match_route("example.com", &Method::GET, "/api")
                .is_err(),
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

        let m = router
            .match_route("example.com", &Method::GET, "/orders")
            .unwrap();
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
        let m = router
            .match_route("example.com", &Method::GET, "/orders")
            .unwrap();
        assert_eq!(
            m.route.auth_provider.as_deref(),
            Some("jwt"),
            "policy enriched the route"
        );

        // Removing the policy reverts the enrichment.
        rec.remove_policy("orders-auth", "shop");
        let m = router
            .match_route("example.com", &Method::GET, "/orders")
            .unwrap();
        assert_eq!(m.route.auth_provider, None, "policy removal reverts auth");
    }
}
