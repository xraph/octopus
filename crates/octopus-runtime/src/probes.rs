//! Kubernetes-style health probe endpoints.
//!
//! Probes are served directly on the gateway listener (the port kubelet
//! targets) and handled *before* request accounting, so a readiness poll during
//! drain never inflates the in-flight counter or blocks graceful shutdown.
//!
//! - `/livez` — liveness; 200 while the process is alive (even while draining).
//! - `/readyz` — readiness; 200 only when ready to receive new traffic.
//! - `/startupz` — startup; 200 once the listener has bound.
//!
//! `/healthz` and `/health` are accepted as back-compat aliases for readiness.

use crate::lifecycle::LifecycleState;
use bytes::Bytes;
use http::{Response, StatusCode};
use http_body_util::Full;

/// Default liveness probe path.
pub const DEFAULT_LIVENESS_PATH: &str = "/livez";
/// Default readiness probe path.
pub const DEFAULT_READINESS_PATH: &str = "/readyz";
/// Default startup probe path.
pub const DEFAULT_STARTUP_PATH: &str = "/startupz";

/// Resolved probe paths (from configuration), plus an enable switch.
#[derive(Clone, Debug)]
pub struct ProbeRoutes {
    /// Whether probe endpoints are served at all.
    pub enabled: bool,
    /// Path for the liveness probe.
    pub liveness: String,
    /// Path for the readiness probe.
    pub readiness: String,
    /// Path for the startup probe.
    pub startup: String,
}

impl Default for ProbeRoutes {
    fn default() -> Self {
        Self {
            enabled: true,
            liveness: DEFAULT_LIVENESS_PATH.to_string(),
            readiness: DEFAULT_READINESS_PATH.to_string(),
            startup: DEFAULT_STARTUP_PATH.to_string(),
        }
    }
}

enum ProbeKind {
    Liveness,
    Readiness,
    Startup,
}

/// Handle a probe request when `path` matches a configured probe endpoint.
///
/// Returns `Some(response)` for liveness/readiness/startup paths (and the
/// `/healthz` / `/health` readiness aliases), and `None` otherwise so the
/// caller continues normal request dispatch.
pub fn handle_probe(
    lifecycle: &LifecycleState,
    routes: &ProbeRoutes,
    path: &str,
) -> Option<Response<Full<Bytes>>> {
    if !routes.enabled {
        return None;
    }

    let kind = if path == routes.liveness {
        ProbeKind::Liveness
    } else if path == routes.readiness || path == "/healthz" || path == "/health" {
        ProbeKind::Readiness
    } else if path == routes.startup {
        ProbeKind::Startup
    } else {
        return None;
    };

    let (ok, name) = match kind {
        ProbeKind::Liveness => (lifecycle.is_live(), "live"),
        ProbeKind::Readiness => (lifecycle.is_ready(), "ready"),
        ProbeKind::Startup => (lifecycle.is_started(), "started"),
    };

    Some(build_response(ok, name))
}

fn build_response(ok: bool, name: &str) -> Response<Full<Bytes>> {
    let (status, status_str) = if ok {
        (StatusCode::OK, "ok")
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "unavailable")
    };
    let body = format!("{{\"status\":\"{status_str}\",\"probe\":\"{name}\"}}");
    Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .header("Cache-Control", "no-store")
        .body(Full::new(Bytes::from(body)))
        // The builder only fails on invalid header/status, none of which are
        // possible here, so a fallback empty body is unreachable in practice.
        .unwrap_or_else(|_| Response::new(Full::new(Bytes::new())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready_lifecycle() -> LifecycleState {
        let lc = LifecycleState::new(false);
        lc.mark_bind_complete();
        lc.mark_config_loaded();
        lc.mark_running();
        lc
    }

    #[test]
    fn unknown_path_returns_none() {
        let lc = LifecycleState::new(false);
        assert!(handle_probe(&lc, &ProbeRoutes::default(), "/api/users").is_none());
    }

    #[test]
    fn disabled_returns_none_even_for_probe_paths() {
        let lc = ready_lifecycle();
        let routes = ProbeRoutes {
            enabled: false,
            ..Default::default()
        };
        assert!(handle_probe(&lc, &routes, "/livez").is_none());
    }

    #[test]
    fn liveness_ok_while_draining_readiness_503() {
        let lc = ready_lifecycle();
        let routes = ProbeRoutes::default();
        lc.begin_draining();

        let live = handle_probe(&lc, &routes, "/livez").unwrap();
        assert_eq!(live.status(), StatusCode::OK);

        let ready = handle_probe(&lc, &routes, "/readyz").unwrap();
        assert_eq!(ready.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn healthz_alias_maps_to_readiness() {
        let lc = ready_lifecycle();
        let routes = ProbeRoutes::default();
        assert_eq!(
            handle_probe(&lc, &routes, "/healthz").unwrap().status(),
            StatusCode::OK
        );
    }

    #[test]
    fn startup_503_before_bind() {
        let lc = LifecycleState::new(false);
        let routes = ProbeRoutes::default();
        assert_eq!(
            handle_probe(&lc, &routes, "/startupz").unwrap().status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        lc.mark_bind_complete();
        assert_eq!(
            handle_probe(&lc, &routes, "/startupz").unwrap().status(),
            StatusCode::OK
        );
    }
}
