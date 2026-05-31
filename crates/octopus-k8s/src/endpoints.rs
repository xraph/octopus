//! Real-time EndpointSlice watching for convention `EndpointSlice` backends.
//!
//! [`EndpointWatchManager`] implements [`octopus_core::BackendWatcher`]: on first
//! request for a `(namespace, service)` it spawns a `discovery.k8s.io/v1`
//! EndpointSlice watch that keeps an [`UpstreamCluster`]'s pod instances in sync
//! with scale events. Idle watches (no request within a TTL) are reaped.

use dashmap::DashMap;
use futures::TryStreamExt;
use k8s_openapi::api::discovery::v1::EndpointSlice;
use kube::{
    api::Api,
    runtime::{watcher, watcher::Config as WatcherConfig},
    Client,
};
use octopus_core::{BackendWatcher, UpstreamCluster, UpstreamInstance};
use octopus_router::Router;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Label Kubernetes sets on every EndpointSlice naming its owning Service.
const SERVICE_NAME_LABEL: &str = "kubernetes.io/service-name";

/// Build `UpstreamInstance`s from a service's EndpointSlices.
///
/// Honors endpoint `ready` (unknown/`false` excluded unless `include_not_ready`),
/// skips `FQDN` slices, and uses the slice ports (falling back to `fallback_port`
/// when a slice declares none).
fn endpoints_to_instances(
    slices: &[EndpointSlice],
    fallback_port: u16,
    include_not_ready: bool,
) -> Vec<UpstreamInstance> {
    let mut out = Vec::new();
    for slice in slices {
        // FQDN slices carry hostnames rather than routable pod IPs.
        if slice.address_type == "FQDN" {
            continue;
        }
        let mut ports: Vec<u16> = slice
            .ports
            .as_ref()
            .map(|ps| ps.iter().filter_map(|p| p.port).map(|p| p as u16).collect())
            .unwrap_or_default();
        if ports.is_empty() {
            ports.push(fallback_port);
        }
        for endpoint in &slice.endpoints {
            let ready = endpoint.conditions.as_ref().and_then(|c| c.ready);
            if ready != Some(true) && !include_not_ready {
                continue;
            }
            for ip in &endpoint.addresses {
                for &port in &ports {
                    out.push(UpstreamInstance::new(
                        format!("{ip}:{port}"),
                        ip.clone(),
                        port,
                    ));
                }
            }
        }
    }
    out
}

/// One active per-`(namespace, service)` watch.
#[derive(Debug)]
struct WatchEntry {
    handle: tokio::task::JoinHandle<()>,
    last_touch: Instant,
}

impl Drop for WatchEntry {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

/// Keeps EndpointSlice-backed convention upstreams' pod instances live.
pub struct EndpointWatchManager {
    client: Client,
    router: Arc<Router>,
    active: DashMap<(String, String), WatchEntry>,
    include_not_ready: bool,
    idle_ttl: Duration,
}

impl std::fmt::Debug for EndpointWatchManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EndpointWatchManager")
            .field("active_watches", &self.active.len())
            .field("idle_ttl", &self.idle_ttl)
            .finish()
    }
}

impl EndpointWatchManager {
    /// Create a manager driving `router` from `client`. Watches idle for longer
    /// than `idle_ttl` are reaped (see [`Self::spawn_reaper`]).
    pub fn new(client: Client, router: Arc<Router>) -> Self {
        Self {
            client,
            router,
            active: DashMap::new(),
            include_not_ready: false,
            idle_ttl: Duration::from_secs(300),
        }
    }

    /// Connect a default kube client, build the manager for `router`, and spawn
    /// its idle-watch reaper. Returned as a shared handle for the data plane.
    pub async fn connect(router: Arc<Router>) -> crate::Result<Arc<Self>> {
        octopus_tls::ensure_crypto_provider();
        let client = Client::try_default().await?;
        let mgr = Arc::new(Self::new(client, router));
        Arc::clone(&mgr).spawn_reaper();
        Ok(mgr)
    }

    /// Spawn the background reaper that aborts idle watches and drops their
    /// upstreams. Call once after construction.
    pub fn spawn_reaper(self: Arc<Self>) {
        tokio::spawn(async move {
            let tick = (self.idle_ttl / 2).max(Duration::from_secs(5));
            loop {
                tokio::time::sleep(tick).await;
                let now = Instant::now();
                let stale: Vec<(String, String)> = self
                    .active
                    .iter()
                    .filter(|e| now.duration_since(e.value().last_touch) > self.idle_ttl)
                    .map(|e| e.key().clone())
                    .collect();
                for id in stale {
                    if self.active.remove(&id).is_some() {
                        // WatchEntry::drop aborts the task.
                        let key = format!("{}.{}.svc", id.1, id.0);
                        self.router.remove_upstream(&key);
                        tracing::debug!(namespace = %id.0, service = %id.1, "reaped idle EndpointSlice watch");
                    }
                }
            }
        });
    }

    /// Background task: watch one service's EndpointSlices and keep the cluster
    /// `key`'s instances in sync.
    async fn watch_service(
        client: Client,
        router: Arc<Router>,
        namespace: String,
        service: String,
        port: u16,
        key: String,
        include_not_ready: bool,
    ) {
        let api: Api<EndpointSlice> = Api::namespaced(client, &namespace);
        let cfg = WatcherConfig::default().labels(&format!("{SERVICE_NAME_LABEL}={service}"));
        let mut by_name: HashMap<String, EndpointSlice> = HashMap::new();
        let mut stream = std::pin::pin!(watcher(api, cfg));

        loop {
            match stream.try_next().await {
                Ok(Some(event)) => {
                    match event {
                        watcher::Event::Apply(s) | watcher::Event::InitApply(s) => {
                            if let Some(n) = s.metadata.name.clone() {
                                by_name.insert(n, s);
                            }
                        }
                        watcher::Event::Delete(s) => {
                            if let Some(n) = &s.metadata.name {
                                by_name.remove(n);
                            }
                        }
                        watcher::Event::Init => by_name.clear(),
                        watcher::Event::InitDone => {}
                    }
                    let slices: Vec<EndpointSlice> = by_name.values().cloned().collect();
                    let instances = endpoints_to_instances(&slices, port, include_not_ready);
                    let mut cluster = UpstreamCluster::new(&key);
                    for inst in instances {
                        cluster.add_instance(inst);
                    }
                    router.register_upstream(cluster);
                }
                Ok(None) => break,
                Err(e) => {
                    tracing::error!(service = %service, error = %e, "EndpointSlice watch error")
                }
            }
        }
    }
}

impl BackendWatcher for EndpointWatchManager {
    fn ensure(&self, namespace: &str, service: &str, port: u16, key: &str) {
        let id = (namespace.to_string(), service.to_string());
        if let Some(mut entry) = self.active.get_mut(&id) {
            entry.last_touch = Instant::now(); // keep-alive
            return;
        }
        let handle = tokio::spawn(Self::watch_service(
            self.client.clone(),
            Arc::clone(&self.router),
            namespace.to_string(),
            service.to_string(),
            port,
            key.to_string(),
            self.include_not_ready,
        ));
        self.active.insert(
            id,
            WatchEntry {
                handle,
                last_touch: Instant::now(),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::discovery::v1::{Endpoint, EndpointConditions, EndpointPort};

    fn endpoint(ip: &str, ready: Option<bool>) -> Endpoint {
        Endpoint {
            addresses: vec![ip.to_string()],
            conditions: Some(EndpointConditions {
                ready,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn slice(endpoints: Vec<Endpoint>, port: Option<i32>) -> EndpointSlice {
        EndpointSlice {
            address_type: "IPv4".to_string(),
            endpoints,
            ports: port.map(|p| {
                vec![EndpointPort {
                    port: Some(p),
                    ..Default::default()
                }]
            }),
            ..Default::default()
        }
    }

    #[test]
    fn maps_ready_endpoints_to_instances() {
        let s = slice(
            vec![
                endpoint("10.0.0.1", Some(true)),
                endpoint("10.0.0.2", Some(false)),
            ],
            Some(8080),
        );
        let got = endpoints_to_instances(&[s], 80, false);
        assert_eq!(got.len(), 1, "only the ready endpoint");
        assert_eq!(got[0].address, "10.0.0.1");
        assert_eq!(got[0].port, 8080);
    }

    #[test]
    fn include_not_ready_keeps_all() {
        let s = slice(
            vec![
                endpoint("10.0.0.1", Some(true)),
                endpoint("10.0.0.2", Some(false)),
            ],
            Some(8080),
        );
        assert_eq!(endpoints_to_instances(&[s], 80, true).len(), 2);
    }

    #[test]
    fn falls_back_to_port_when_slice_has_none() {
        let s = slice(vec![endpoint("10.0.0.1", Some(true))], None);
        let got = endpoints_to_instances(&[s], 9000, false);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].port, 9000);
    }
}
