//! Abstraction for keeping a convention-derived upstream's endpoints live.

/// Ensures (and keeps fresh) a load-balanced upstream for a
/// `<service>.<namespace>` target — e.g. by watching Kubernetes EndpointSlices
/// and re-registering the cluster's pod instances on scale events.
///
/// The data plane holds only a `dyn BackendWatcher`, so it stays
/// Kubernetes-agnostic; the concrete implementation (with the kube client and
/// watch tasks) lives in the operator crate.
pub trait BackendWatcher: Send + Sync + std::fmt::Debug {
    /// Ensure an upstream cluster named `key` exists and tracks the live pod
    /// endpoints of `service` in `namespace` on `port`.
    ///
    /// Idempotent: a repeat call for the same `(namespace, service)` only
    /// refreshes liveness (keep-alive), so the data plane can call it on every
    /// request without spawning duplicate watches.
    fn ensure(&self, namespace: &str, service: &str, port: u16, key: &str);
}
