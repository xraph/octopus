//! Reconciler that turns Kubernetes routing resources into live router state.
//!
//! [`RouteReconciler`] owns a [`RouteStore`] and the live [`Router`]. As
//! resources are upserted/removed it re-translates, merges every source, and
//! applies the result to the router — so the router always reflects the merged
//! intent of static config + Gateway API + Octopus CRDs.

use crate::apply::apply_to_router;
use crate::crds::{
    ConventionSpec, GatewayIsolation, OctopusGateway, OctopusGatewaySpec, OctopusPolicy,
    OctopusPolicySpec, OctopusRoute, OctopusRouteSpec, OctopusUpstream, OctopusUpstreamSpec,
};
use crate::dedicated::DedicatedGatewayReconciler;
use crate::gateway_api::{
    GRPCRoute, GRPCRouteSpec, Gateway, GatewayClass, GatewaySpec, HTTPRoute, HTTPRouteSpec,
    ParentRef,
};
use crate::ir::{IntermediateRoute, RouteSource, RouteStore, SourceKey};
use crate::policy::{apply_overlays, PolicyOverlay};
use crate::refgrant::{is_permitted, RefRequest, ReferenceGrant, ReferenceGrantSpec};
use crate::status::{build_gateway_conditions, build_status, ReconcileOutcome};
use crate::tls::TlsReconciler;
use crate::translate::{
    grpcroute_to_route, httproute_to_route, octopus_gateway_to_entry, octopus_route_to_route,
    octopus_upstream_to_cluster,
};
use crate::validate::{gatewayclass_is_ours, validate_policy, validate_route, validate_upstream};
use arc_swap::ArcSwap;
use futures::TryStreamExt;
use k8s_openapi::api::core::v1::{ConfigMap, Secret};
use kube::{
    api::{Api, Patch, PatchParams},
    core::{ClusterResourceScope, NamespaceResourceScope},
    runtime::{watcher, watcher::Config as WatcherConfig},
    Client,
};
use octopus_core::UpstreamCluster;
use octopus_farp::FarpApiHandler;
use octopus_router::{Router, VirtualGatewayIndex};
use octopus_tls::SwappableTlsAcceptor;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};

/// HTTPRoute spec + its metadata annotations, stored together so re-translation
/// on a Gateway change can reproduce the same proxy-mode config from annotations.
type HttpRouteEntry = (HTTPRouteSpec, BTreeMap<String, String>);

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
    /// `Gateway` metadata (`ns/name` → (generation, gatewayClassName)) for
    /// status writeback once we know the class is ours.
    gateway_meta: Mutex<HashMap<String, (Option<i64>, String)>>,
    /// Names of GatewayClasses whose `controllerName` is ours — Gateways
    /// referencing them get `Accepted`/`Programmed` status.
    owned_gateway_classes: Mutex<HashSet<String>>,
    /// Raw HTTPRoute specs and their annotations (`ns/name` → (spec, annotations)),
    /// retained for re-translation when a parent Gateway's listener hostnames change.
    httproutes: Mutex<HashMap<String, HttpRouteEntry>>,
    grpcroutes: Mutex<HashMap<String, GRPCRouteSpec>>,
    /// Raw `OctopusGateway` specs (`ns/name` → spec), retained so the virtual
    /// gateway index can be rebuilt whenever any gateway changes.
    octopus_gateways: Mutex<HashMap<String, OctopusGatewaySpec>>,
    /// Virtual gateway index: routes attach to a gateway by host and inherit its
    /// policy defaults during apply. Rebuilt from `octopus_gateways` on change.
    /// Behind `ArcSwap` so the data-plane handler can share it and read it
    /// lock-free on the request hot path (see [`Self::gateway_index_handle`]).
    gateway_index: Arc<ArcSwap<VirtualGatewayIndex>>,
    /// Renders `Dedicated` gateways into their own workloads. Set only on the
    /// replica that owns writes (the leader); `None` elsewhere / in unit tests,
    /// where dedicated reconciliation is skipped.
    dedicated_reconciler: OnceLock<Arc<DedicatedGatewayReconciler>>,
    /// When set, this instance IS the dedicated child for that gateway: it serves
    /// ONLY that gateway's routes (treated as local). When unset, this is the edge
    /// and `Dedicated` gateways are excluded (served directly by their children).
    serve_only_gateway: OnceLock<String>,
    /// FARP handler, so a gateway with `farp_binding: true` can drive the FARP
    /// federation binding from its CRD. `None` when FARP is disabled.
    farp_handler: OnceLock<Arc<FarpApiHandler>>,
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
            gateway_meta: Mutex::new(HashMap::new()),
            owned_gateway_classes: Mutex::new(HashSet::new()),
            httproutes: Mutex::new(HashMap::new()),
            grpcroutes: Mutex::new(HashMap::new()),
            octopus_gateways: Mutex::new(HashMap::new()),
            gateway_index: Arc::new(ArcSwap::from_pointee(VirtualGatewayIndex::default())),
            dedicated_reconciler: OnceLock::new(),
            serve_only_gateway: OnceLock::new(),
            farp_handler: OnceLock::new(),
            client: OnceLock::new(),
        }
    }

    /// Provide the reconciler that renders `Dedicated` gateway workloads. Called
    /// once on the write-owning replica; subsequent calls are ignored.
    pub fn set_dedicated_reconciler(&self, reconciler: Arc<DedicatedGatewayReconciler>) {
        let _ = self.dedicated_reconciler.set(reconciler);
    }

    /// Mark this instance as the dedicated child for `gateway`: it will serve only
    /// that gateway's routes. Called once at startup; subsequent calls are ignored.
    pub fn set_serve_only_gateway(&self, gateway: String) {
        let _ = self.serve_only_gateway.set(gateway);
    }

    /// Provide the FARP handler so an `OctopusGateway` with `farp_binding: true`
    /// can drive the FARP federation binding. Called once at startup.
    pub fn set_farp_handler(&self, handler: Arc<FarpApiHandler>) {
        let _ = self.farp_handler.set(handler);
    }

    /// Drive the FARP federation binding from an `OctopusGateway`: when
    /// `farp_binding` is set, discovered services are served under this gateway's
    /// first hostname and inherit its policy. No-op without a FARP handler.
    fn reconcile_farp_binding(&self, name: &str, spec: &OctopusGatewaySpec) {
        let Some(farp) = self.farp_handler.get() else {
            return;
        };
        if spec.farp_binding {
            if let Some(binding) = gateway_binding_from_spec(name, spec) {
                farp.set_binding(Some(binding));
            }
        }
    }

    /// Clear the FARP binding when a `farp_binding` gateway is removed.
    fn clear_farp_binding(&self) {
        if let Some(farp) = self.farp_handler.get() {
            farp.set_binding(None);
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

    /// Like [`Self::write_status`] but for a cluster-scoped resource (e.g.
    /// `GatewayClass`).
    fn write_cluster_status<K>(
        &self,
        name: &str,
        generation: Option<i64>,
        outcome: &ReconcileOutcome,
    ) where
        K: kube::Resource<Scope = ClusterResourceScope>
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
        let name = name.to_string();
        let kind = std::any::type_name::<K>();
        tokio::spawn(async move {
            let api: Api<K> = Api::all(client);
            let patch = serde_json::json!({ "status": status });
            if let Err(e) = api
                .patch_status(&name, &PatchParams::default(), &Patch::Merge(&patch))
                .await
            {
                tracing::warn!(resource = kind, name = %name, error = %e, "cluster status writeback failed");
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
    pub fn upsert_httproute(
        &self,
        name: &str,
        namespace: &str,
        spec: &HTTPRouteSpec,
        annotations: &BTreeMap<String, String>,
    ) {
        if let Ok(mut m) = self.httproutes.lock() {
            m.insert(
                format!("{namespace}/{name}"),
                (spec.clone(), annotations.clone()),
            );
        }
        self.translate_httproute(name, namespace, spec, annotations);
        self.reapply();
    }

    fn translate_httproute(
        &self,
        name: &str,
        namespace: &str,
        spec: &HTTPRouteSpec,
        annotations: &BTreeMap<String, String>,
    ) {
        let listeners = self.listener_hostnames_for(&spec.parent_refs, namespace);
        let (routes, upstreams) =
            httproute_to_route(name, namespace, spec, &listeners, annotations);
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

    /// Store/replace a parent `Gateway` and re-translate dependent routes. When
    /// the Gateway names a class we own, report `Accepted`/`Programmed` on it.
    pub fn set_gateway_route(
        &self,
        name: &str,
        namespace: &str,
        generation: Option<i64>,
        spec: GatewaySpec,
    ) {
        let key = format!("{namespace}/{name}");
        let class = spec.gateway_class_name.clone();
        if let Ok(mut m) = self.gateways.lock() {
            m.insert(key.clone(), spec);
        }
        if let Ok(mut meta) = self.gateway_meta.lock() {
            meta.insert(key, (generation, class.clone()));
        }
        self.reresolve_gateway_routes();
        let owned = self
            .owned_gateway_classes
            .lock()
            .map(|o| o.contains(&class))
            .unwrap_or(false);
        if owned {
            self.write_gateway_status(name, namespace, generation);
        }
    }

    /// Drop a parent `Gateway` and re-translate dependent routes.
    pub fn remove_gateway_route(&self, name: &str, namespace: &str) {
        let key = format!("{namespace}/{name}");
        if let Ok(mut m) = self.gateways.lock() {
            m.remove(&key);
        }
        if let Ok(mut meta) = self.gateway_meta.lock() {
            meta.remove(&key);
        }
        self.reresolve_gateway_routes();
    }

    /// Mark a GatewayClass as ours and program `Accepted`/`Programmed` status on
    /// any Gateways already referencing it (handles the class being claimed after
    /// its Gateways were first seen).
    pub fn claim_gateway_class(&self, class: &str) {
        if let Ok(mut owned) = self.owned_gateway_classes.lock() {
            if !owned.insert(class.to_string()) {
                return; // already claimed; nothing new to status
            }
        }
        let targets: Vec<(String, Option<i64>)> = match self.gateway_meta.lock() {
            Ok(meta) => meta
                .iter()
                .filter(|(_, (_, cls))| cls == class)
                .map(|(k, (gen, _))| (k.clone(), *gen))
                .collect(),
            Err(_) => return,
        };
        for (key, generation) in targets {
            if let Some((ns, name)) = key.split_once('/') {
                self.write_gateway_status(name, ns, generation);
            }
        }
    }

    /// Forget a GatewayClass we no longer own.
    pub fn unclaim_gateway_class(&self, class: &str) {
        if let Ok(mut owned) = self.owned_gateway_classes.lock() {
            owned.remove(class);
        }
    }

    /// Patch a `Gateway`'s `.status` with `Accepted`/`Programmed` conditions. A
    /// no-op until a client is set (only the leader writes status), mirroring
    /// [`Self::write_status`].
    fn write_gateway_status(&self, name: &str, namespace: &str, generation: Option<i64>) {
        let Some(client) = self.client.get().cloned() else {
            return;
        };
        let now = k8s_openapi::chrono::Utc::now().to_rfc3339();
        let conditions = build_gateway_conditions(generation, &now);
        let (name, namespace) = (name.to_string(), namespace.to_string());
        tokio::spawn(async move {
            let api: Api<Gateway> = Api::namespaced(client, &namespace);
            let patch = serde_json::json!({ "status": { "conditions": conditions } });
            if let Err(e) = api
                .patch_status(&name, &PatchParams::default(), &Patch::Merge(&patch))
                .await
            {
                tracing::warn!(name = %name, namespace = %namespace, error = %e, "gateway status writeback failed");
            }
        });
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
            let entry = self
                .httproutes
                .lock()
                .ok()
                .and_then(|m| m.get(&format!("{ns}/{name}")).cloned());
            if let Some((spec, annotations)) = entry {
                self.translate_httproute(&name, &ns, &spec, &annotations);
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

    /// Upsert an `OctopusGateway` (virtual gateway), rebuild the index, re-apply.
    pub fn upsert_virtual_gateway(&self, name: &str, namespace: &str, spec: &OctopusGatewaySpec) {
        if let Ok(mut gws) = self.octopus_gateways.lock() {
            gws.insert(format!("{namespace}/{name}"), spec.clone());
        }
        self.rebuild_gateway_index();
        self.reapply();
    }

    /// Remove an `OctopusGateway`, rebuild the index, re-apply.
    pub fn remove_virtual_gateway(&self, name: &str, namespace: &str) {
        if let Ok(mut gws) = self.octopus_gateways.lock() {
            gws.remove(&format!("{namespace}/{name}"));
        }
        self.rebuild_gateway_index();
        self.reapply();
    }

    /// Rebuild the virtual gateway index from all stored `OctopusGateway` specs.
    /// The gateway's metadata `name` is its id (and the routes' `gateway_id`).
    fn rebuild_gateway_index(&self) {
        // Edge (serve_only unset): only `Shared` gateways are served here;
        // `Dedicated` ones are reached directly via their own child. Child
        // (serve_only = x): only gateway `x`, served locally regardless of tier.
        let serve_only = self.serve_only_gateway.get().map(String::as_str);
        let entries = match self.octopus_gateways.lock() {
            Ok(gws) => gws
                .iter()
                .filter(|(key, spec)| {
                    let name = key.split_once('/').map(|(_, n)| n).unwrap_or(key.as_str());
                    match serve_only {
                        Some(only) => name == only,
                        None => spec.isolation == GatewayIsolation::Shared,
                    }
                })
                .map(|(key, spec)| {
                    let name = key.split_once('/').map(|(_, n)| n).unwrap_or(key.as_str());
                    octopus_gateway_to_entry(name, spec)
                })
                .collect(),
            Err(_) => {
                tracing::error!("octopus_gateways lock poisoned; keeping previous gateway index");
                return;
            }
        };
        self.gateway_index
            .store(Arc::new(VirtualGatewayIndex::new(entries)));
    }

    /// A shared handle to the live virtual gateway index, for the data-plane
    /// handler to resolve a request's gateway by host (lock-free `load`).
    pub fn gateway_index_handle(&self) -> Arc<ArcSwap<VirtualGatewayIndex>> {
        Arc::clone(&self.gateway_index)
    }

    /// Reconcile a gateway's dedicated workload from its isolation tier:
    /// `Dedicated` → apply the child workload; `Shared` → ensure no stale child
    /// remains (e.g. after a downgrade). No-op unless a dedicated reconciler is
    /// set (write-owning replica only). Runs on a spawned task to stay sync.
    fn reconcile_dedicated(&self, name: &str, namespace: &str, spec: &OctopusGatewaySpec) {
        let Some(reconciler) = self.dedicated_reconciler.get().cloned() else {
            return;
        };
        let (name, namespace) = (name.to_string(), namespace.to_string());
        match spec.isolation {
            GatewayIsolation::Dedicated => {
                let spec = spec.clone();
                tokio::spawn(async move {
                    if let Err(e) = reconciler.reconcile(&name, &namespace, &spec).await {
                        tracing::warn!(gateway = %name, error = %e, "failed to reconcile dedicated gateway");
                    }
                });
            }
            GatewayIsolation::Shared => {
                tokio::spawn(async move {
                    let _ = reconciler.delete(&name, &namespace).await;
                });
            }
        }
    }

    /// Delete a gateway's dedicated workload (on `OctopusGateway` delete). No-op
    /// unless a dedicated reconciler is set.
    fn delete_dedicated(&self, name: &str, namespace: &str) {
        let Some(reconciler) = self.dedicated_reconciler.get().cloned() else {
            return;
        };
        let (name, namespace) = (name.to_string(), namespace.to_string());
        tokio::spawn(async move {
            let _ = reconciler.delete(&name, &namespace).await;
        });
    }

    /// Index of `Dedicated` gateways' domains, used by the edge to drop routes
    /// whose host a dedicated child serves directly. `None` when none exist.
    fn dedicated_host_index(&self) -> Option<Arc<VirtualGatewayIndex>> {
        let gws = self.octopus_gateways.lock().ok()?;
        let entries: Vec<_> = gws
            .iter()
            .filter(|(_, spec)| spec.isolation == GatewayIsolation::Dedicated)
            .map(|(key, spec)| {
                let name = key.split_once('/').map(|(_, n)| n).unwrap_or(key.as_str());
                octopus_gateway_to_entry(name, spec)
            })
            .collect();
        if entries.is_empty() {
            None
        } else {
            Some(Arc::new(VirtualGatewayIndex::new(entries)))
        }
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

        let gateways = self.gateway_index.load_full();

        // Host-based auto-attachment: a route with no explicit gateway attaches
        // to the virtual gateway that owns its host scope, so it inherits that
        // gateway's policy defaults during apply.
        if !gateways.is_empty() {
            for route in &mut table.routes {
                if route.gateway_id.is_none() {
                    if let Some(gw) = gateways.attach(&route.host) {
                        route.gateway_id = Some(gw.id.to_string());
                    }
                }
            }
        }

        // Gateway-scoped serving (single-hop invariant): a dedicated child serves
        // ONLY its gateway's-host routes; the edge drops routes whose host belongs
        // to a `Dedicated` gateway (those are served directly by the child).
        if self.serve_only_gateway.get().is_some() {
            table.routes.retain(|r| gateways.attach(&r.host).is_some());
        } else if let Some(dedicated) = self.dedicated_host_index() {
            table.routes.retain(|r| dedicated.attach(&r.host).is_none());
        }

        if let Err(e) = apply_to_router(&self.router, &table, &gateways) {
            tracing::error!(error = %e, "Failed to apply routing table to router");
        }
    }
}

/// Create a Kubernetes client and spawn watchers for HTTPRoute and the Octopus
/// CRDs, driving `reconciler`. An empty `namespaces` watches all namespaces
/// (requires cluster-scoped RBAC); otherwise watchers are spawned per namespace.
///
/// The watchers program **per-replica** state (the local in-memory router and
/// TLS resolver), so every replica runs them and serves traffic. When
/// `leader_election` is set, acquiring the operator [`Lease`](crate::leader)
/// runs in the background and does **not** block startup; only the lease holder
/// is handed the client, so only it writes `.status` back to the API server
/// (the reconciler no-ops status writes until a client is set).
pub async fn start(
    reconciler: Arc<RouteReconciler>,
    namespaces: Vec<String>,
    tls_acceptor: Option<SwappableTlsAcceptor>,
    leader_election: bool,
    dedicated_image: Option<String>,
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

    // Watchers program this replica's local router and TLS resolver, so every
    // replica must run them — otherwise a follower would serve traffic with an
    // empty router and no certs. These never block server startup.
    for scope in &scopes {
        spawn_watchers(client.clone(), Arc::clone(&reconciler), scope.clone());
        if let Some(ref tls) = tls {
            spawn_tls_watchers(client.clone(), Arc::clone(tls), scope.clone());
        }
    }

    // GatewayClass is cluster-scoped: watch it once (independent of namespace
    // scopes) and claim those that name Octopus as their controller.
    tokio::spawn({
        let api: Api<GatewayClass> = Api::all(client.clone());
        let rec = Arc::clone(&reconciler);
        async move { run_watcher(api, rec, "GatewayClass", on_gateway_class).await }
    });

    // Writing `.status` back to resources is a cluster-singleton side effect, so
    // for HA only the lease holder does it. Acquire leadership in the background
    // (never blocking the data plane) and hand the client to the reconciler only
    // once we're leader; the reconciler no-ops status writes until then.
    if leader_election {
        let ns = crate::leader::lease_namespace();
        let me = crate::leader::pod_identity();
        tracing::info!(identity = %me, namespace = %ns, "leader election enabled; acquiring operator lease in background");
        tokio::spawn(async move {
            crate::leader::acquire_leadership(&client, &ns, &me).await;
            tracing::info!("operator leadership acquired; enabling status writeback");
            reconciler.set_client(client.clone());
            if let Some(image) = dedicated_image {
                reconciler.set_dedicated_reconciler(Arc::new(DedicatedGatewayReconciler::new(
                    client.clone(),
                    image,
                )));
            }
            crate::leader::run_leader_loop(client, ns, me).await;
        });
    } else {
        // Single-instance (no leader election): this replica writes status and
        // renders dedicated gateway workloads.
        if let Some(image) = dedicated_image {
            reconciler.set_dedicated_reconciler(Arc::new(DedicatedGatewayReconciler::new(
                client.clone(),
                image,
            )));
        }
        reconciler.set_client(client);
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
            api::<OctopusGateway>(&client, &namespace),
            Arc::clone(&reconciler),
        );
        async move { run_watcher(api, rec, "OctopusGateway", on_octopus_gateway).await }
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

/// Build a FARP [`GatewayBinding`](octopus_farp::GatewayBinding) from an
/// `OctopusGateway` spec: its first hostname + id + inherited auth/rate-limit/timeout.
/// `None` when the gateway declares no hostname.
fn gateway_binding_from_spec(
    name: &str,
    spec: &OctopusGatewaySpec,
) -> Option<octopus_farp::GatewayBinding> {
    let host = spec.hostnames.first()?;
    let dp = spec.default_policy.as_ref();
    Some(
        octopus_farp::GatewayBinding::new(host)
            .with_gateway_id(Some(name.to_string()))
            .with_default_auth(
                dp.and_then(|p| p.auth_provider.clone())
                    .or_else(|| spec.default_auth_provider.clone()),
            )
            .with_rate_limit(dp.and_then(|p| {
                p.rate_limit.as_ref().map(|rl| {
                    (
                        rl.requests,
                        std::time::Duration::from_secs(rl.window_seconds),
                    )
                })
            }))
            .with_timeout(
                dp.and_then(|p| p.timeout_seconds)
                    .map(std::time::Duration::from_secs),
            ),
    )
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
                rec.set_gateway_route(&name, &ns, o.metadata.generation, o.spec.clone());
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
                let annotations = o.metadata.annotations.clone().unwrap_or_default();
                rec.upsert_httproute(&name, &ns, &o.spec, &annotations);
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

fn on_gateway_class(rec: &RouteReconciler, event: watcher::Event<GatewayClass>) {
    match event {
        watcher::Event::Apply(o) | watcher::Event::InitApply(o) => {
            // Only claim GatewayClasses that name us as their controller; leave
            // others untouched so we don't fight their owning controller.
            if gatewayclass_is_ours(&o.spec) {
                if let Some(name) = o.metadata.name.clone() {
                    rec.write_cluster_status::<GatewayClass>(
                        &name,
                        o.metadata.generation,
                        &ReconcileOutcome::Accepted,
                    );
                    // Claiming the class also programs status on its Gateways.
                    rec.claim_gateway_class(&name);
                }
            }
        }
        watcher::Event::Delete(o) => {
            if let Some(name) = o.metadata.name {
                rec.unclaim_gateway_class(&name);
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

fn on_octopus_gateway(rec: &RouteReconciler, event: watcher::Event<OctopusGateway>) {
    match event {
        watcher::Event::Apply(o) | watcher::Event::InitApply(o) => {
            if let Some((name, ns)) = ident(&o) {
                rec.upsert_virtual_gateway(&name, &ns, &o.spec);
                rec.reconcile_dedicated(&name, &ns, &o.spec);
                rec.reconcile_farp_binding(&name, &o.spec);
                rec.write_status::<OctopusGateway>(
                    &name,
                    &ns,
                    o.metadata.generation,
                    &ReconcileOutcome::Accepted,
                );
            }
        }
        watcher::Event::Delete(o) => {
            if let Some((name, ns)) = ident(&o) {
                rec.remove_virtual_gateway(&name, &ns);
                rec.delete_dedicated(&name, &ns);
                if o.spec.farp_binding {
                    rec.clear_farp_binding();
                }
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
            Some(1),
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
        rec.upsert_httproute("r", "default", &spec, &BTreeMap::new());

        // Only the host within the listener's wildcard attaches.
        assert!(router
            .match_route("api.acme.com", &Method::GET, "/")
            .is_ok());
        assert!(router.match_route("evil.com", &Method::GET, "/").is_err());
    }

    #[test]
    fn virtual_gateway_policy_inherited_by_host_scoped_route() {
        use crate::crds::GatewayDefaultPolicy;
        let router = Arc::new(Router::new());
        let rec = RouteReconciler::new(Arc::clone(&router));

        // A virtual gateway owning api.acme.com with a default auth provider.
        rec.upsert_virtual_gateway(
            "platform-api",
            "octopus-system",
            &OctopusGatewaySpec {
                listen: "0.0.0.0:8080".into(),
                gateway_class_name: None,
                default_auth_provider: None,
                hostnames: vec!["api.acme.com".into()],
                default_policy: Some(GatewayDefaultPolicy {
                    auth_provider: Some("jwt".into()),
                    timeout_seconds: None,
                    rate_limit: None,
                    cors: None,
                }),
                isolation: Default::default(),
                farp_binding: false,
            },
        );

        // A standard Gateway + HTTPRoute scoped to api.acme.com (no auth of its own).
        rec.set_gateway_route(
            "gw",
            "infra",
            Some(1),
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
        let mut spec = httproute_spec("/users", "svc", 80);
        spec.parent_refs = vec![ParentRef {
            name: "gw".into(),
            namespace: Some("infra".into()),
            section_name: None,
        }];
        spec.hostnames = vec!["api.acme.com".into()];
        rec.upsert_httproute("r", "default", &spec, &BTreeMap::new());

        // The route auto-attached to the gateway and inherited its auth provider.
        let m = router
            .match_route("api.acme.com", &Method::GET, "/users")
            .unwrap();
        assert_eq!(m.route.gateway_id.as_deref(), Some("platform-api"));
        assert_eq!(m.route.auth_provider.as_deref(), Some("jwt"));
    }

    #[test]
    fn farp_binding_gateway_drives_farp_handler() {
        let router = Arc::new(Router::new());
        let rec = RouteReconciler::new(Arc::clone(&router));
        let farp = Arc::new(octopus_farp::FarpApiHandler::new(Arc::new(
            octopus_farp::SchemaRegistry::with_cache_ttl(std::time::Duration::from_secs(300)),
        )));
        rec.set_farp_handler(Arc::clone(&farp));

        let mut spec = gw_spec(&["api.acme.com"], GatewayIsolation::Shared);
        spec.farp_binding = true;
        spec.default_auth_provider = Some("jwt".into());
        rec.reconcile_farp_binding("platform-api", &spec);

        let cell = farp.binding_handle();
        let guard = cell.load();
        let binding = (**guard).as_ref().expect("FARP binding set from the CRD");
        assert_eq!(
            binding.host,
            octopus_router::HostMatch::Exact("api.acme.com".into())
        );
        assert_eq!(binding.gateway_id.as_deref(), Some("platform-api"));
        assert_eq!(binding.default_auth_provider.as_deref(), Some("jwt"));
    }

    fn gw_spec(hostnames: &[&str], isolation: GatewayIsolation) -> OctopusGatewaySpec {
        OctopusGatewaySpec {
            listen: "0.0.0.0:8080".into(),
            gateway_class_name: None,
            default_auth_provider: None,
            hostnames: hostnames.iter().map(ToString::to_string).collect(),
            default_policy: None,
            isolation,
            farp_binding: false,
        }
    }

    fn host_route(host: &str) -> IntermediateRoute {
        IntermediateRoute::new(Method::GET, "/x", "up", RouteSource::Static)
            .with_host(octopus_router::HostMatch::Exact(host.into()))
    }

    #[test]
    fn edge_excludes_dedicated_gateways_and_drops_their_routes() {
        let router = Arc::new(Router::new());
        let rec = RouteReconciler::new(Arc::clone(&router)); // no serve_only → edge
        rec.upsert_virtual_gateway(
            "big",
            "ns",
            &gw_spec(&["big.acme.com"], GatewayIsolation::Dedicated),
        );
        rec.upsert_virtual_gateway(
            "platform-api",
            "ns",
            &gw_spec(&["api.acme.com"], GatewayIsolation::Shared),
        );
        rec.seed_static(
            vec![host_route("api.acme.com"), host_route("big.acme.com")],
            vec![],
        );

        assert!(
            router
                .match_route("api.acme.com", &Method::GET, "/x")
                .is_ok(),
            "edge serves a Shared gateway's host"
        );
        assert!(
            router
                .match_route("big.acme.com", &Method::GET, "/x")
                .is_err(),
            "edge does NOT serve a Dedicated gateway's host (the child does, directly)"
        );
    }

    #[test]
    fn child_serves_only_its_gateway_routes_with_policy() {
        use crate::crds::GatewayDefaultPolicy;
        let router = Arc::new(Router::new());
        let rec = RouteReconciler::new(Arc::clone(&router));
        rec.set_serve_only_gateway("big".to_string()); // this instance IS gateway "big"

        let mut big = gw_spec(&["big.acme.com"], GatewayIsolation::Dedicated);
        big.default_policy = Some(GatewayDefaultPolicy {
            auth_provider: Some("jwt".into()),
            timeout_seconds: None,
            rate_limit: None,
            cors: None,
        });
        rec.upsert_virtual_gateway("big", "ns", &big);
        rec.upsert_virtual_gateway(
            "platform-api",
            "ns",
            &gw_spec(&["api.acme.com"], GatewayIsolation::Shared),
        );
        rec.seed_static(
            vec![host_route("api.acme.com"), host_route("big.acme.com")],
            vec![],
        );

        // The child serves ONLY its own gateway's host...
        assert!(
            router
                .match_route("api.acme.com", &Method::GET, "/x")
                .is_err(),
            "child does not serve other gateways' hosts"
        );
        let m = router
            .match_route("big.acme.com", &Method::GET, "/x")
            .expect("child serves its own gateway's host");
        // ...and that route attaches to "big" and inherits its policy (treated local).
        assert_eq!(m.route.gateway_id.as_deref(), Some("big"));
        assert_eq!(m.route.auth_provider.as_deref(), Some("jwt"));
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
            route_rules: vec![],
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
            &BTreeMap::new(),
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
            &BTreeMap::new(),
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
