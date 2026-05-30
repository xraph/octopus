//! Lifecycle state for Kubernetes-style health probes.
//!
//! [`LifecycleState`] is the single, lock-free source of truth backing the
//! `/livez`, `/readyz`, and `/startupz` probe endpoints. It is cloned into the
//! [`Server`](crate::Server), the [`RequestHandler`](crate::RequestHandler),
//! and (via [`LifecycleState::discovery_synced_flag`]) the service-discovery
//! watcher, so every component observes the same lifecycle.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Shared, lock-free liveness/readiness/startup state.
///
/// Cloning is cheap and shares the underlying state, so all clones report the
/// same lifecycle.
#[derive(Clone, Debug)]
pub struct LifecycleState {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    /// Listener has bound — the startup probe passes once this is true.
    bind_complete: AtomicBool,
    /// Configuration has been fully loaded and applied.
    config_loaded: AtomicBool,
    /// The server has entered its accept loop.
    running: AtomicBool,
    /// Shutdown has begun; readiness reports NotReady immediately.
    draining: AtomicBool,
    /// The process is fully stopped; liveness reports dead.
    stopped: AtomicBool,
    /// The initial service-discovery sync has completed.
    ///
    /// Held as a separate `Arc` so it can be handed to the discovery watcher
    /// (in another crate) as a plain `Arc<AtomicBool>` without coupling crates.
    discovery_synced: Arc<AtomicBool>,
    /// Whether readiness must wait for discovery to sync.
    discovery_required: bool,
}

impl LifecycleState {
    /// Create a new lifecycle tracker.
    ///
    /// When `discovery_required` is `true`, readiness waits for the discovery
    /// watcher to complete its first sync before reporting ready. When `false`,
    /// discovery is treated as already synced.
    pub fn new(discovery_required: bool) -> Self {
        let inner = Inner {
            bind_complete: AtomicBool::new(false),
            config_loaded: AtomicBool::new(false),
            running: AtomicBool::new(false),
            draining: AtomicBool::new(false),
            stopped: AtomicBool::new(false),
            // Pre-set synced when discovery isn't required, so readiness only
            // depends on running + config.
            discovery_synced: Arc::new(AtomicBool::new(!discovery_required)),
            discovery_required,
        };
        Self {
            inner: Arc::new(inner),
        }
    }

    /// A shared flag the discovery watcher flips after its first sync pass.
    ///
    /// Returned as a plain `Arc<AtomicBool>` so it can cross crate boundaries
    /// (e.g. into the FARP discovery watcher) without a circular dependency.
    pub fn discovery_synced_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.inner.discovery_synced)
    }

    // ── transitions ──────────────────────────────────────────────────────

    /// Mark the listener as bound (startup complete).
    pub fn mark_bind_complete(&self) {
        self.inner.bind_complete.store(true, Ordering::Release);
    }

    /// Mark configuration as loaded and applied.
    pub fn mark_config_loaded(&self) {
        self.inner.config_loaded.store(true, Ordering::Release);
    }

    /// Mark the server as running (accept loop entered).
    pub fn mark_running(&self) {
        self.inner.running.store(true, Ordering::Release);
    }

    /// Mark the initial discovery sync as complete.
    pub fn mark_discovery_synced(&self) {
        self.inner.discovery_synced.store(true, Ordering::Release);
    }

    /// Begin draining: readiness flips to NotReady immediately.
    pub fn begin_draining(&self) {
        self.inner.draining.store(true, Ordering::Release);
    }

    /// Mark the process as fully stopped (liveness reports dead).
    pub fn mark_stopped(&self) {
        self.inner.stopped.store(true, Ordering::Release);
    }

    // ── evaluators ───────────────────────────────────────────────────────

    /// Liveness: the process is alive unless it has fully stopped.
    ///
    /// Liveness must NOT depend on readiness. A draining-but-healthy pod stays
    /// live so Kubernetes lets it finish in-flight requests instead of killing
    /// it mid-drain.
    pub fn is_live(&self) -> bool {
        !self.inner.stopped.load(Ordering::Acquire)
    }

    /// Readiness: ready to receive new traffic.
    pub fn is_ready(&self) -> bool {
        self.inner.running.load(Ordering::Acquire)
            && !self.inner.draining.load(Ordering::Acquire)
            && self.inner.config_loaded.load(Ordering::Acquire)
            && (!self.inner.discovery_required
                || self.inner.discovery_synced.load(Ordering::Acquire))
    }

    /// Startup: the listener has bound.
    pub fn is_started(&self) -> bool {
        self.inner.bind_complete.load(Ordering::Acquire)
    }

    /// Whether the server is currently draining.
    pub fn is_draining(&self) -> bool {
        self.inner.draining.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_ready_until_running_and_config_loaded() {
        let lc = LifecycleState::new(false);
        assert!(lc.is_live(), "live from creation");
        assert!(!lc.is_started(), "not started before bind");
        assert!(!lc.is_ready(), "not ready before running");

        lc.mark_bind_complete();
        assert!(lc.is_started());
        assert!(
            !lc.is_ready(),
            "started but config not loaded / not running"
        );

        lc.mark_config_loaded();
        lc.mark_running();
        assert!(lc.is_ready(), "ready once running + config loaded");
    }

    #[test]
    fn readiness_waits_for_discovery_when_required() {
        let lc = LifecycleState::new(true);
        lc.mark_bind_complete();
        lc.mark_config_loaded();
        lc.mark_running();
        assert!(!lc.is_ready(), "discovery required but not synced");

        lc.discovery_synced_flag().store(true, Ordering::Release);
        assert!(lc.is_ready(), "ready once discovery synced");
    }

    #[test]
    fn draining_flips_readiness_but_not_liveness() {
        let lc = LifecycleState::new(false);
        lc.mark_bind_complete();
        lc.mark_config_loaded();
        lc.mark_running();
        assert!(lc.is_ready());

        lc.begin_draining();
        assert!(!lc.is_ready(), "draining → NotReady immediately");
        assert!(lc.is_live(), "draining pod stays live");
        assert!(lc.is_started(), "still started while draining");
    }

    #[test]
    fn stopped_reports_dead() {
        let lc = LifecycleState::new(false);
        lc.mark_running();
        lc.mark_stopped();
        assert!(!lc.is_live(), "stopped → not live");
    }
}
