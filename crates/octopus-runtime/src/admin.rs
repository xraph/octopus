//! Admin dashboard integration
//!
//! Integrates the Askama-based admin dashboard into the Octopus runtime.
//! Now uses the Axum-based DashboardRouter directly instead of manual routing.

use axum::body::Body;
use axum::Router as AxumRouter;
use bytes::Bytes;
use http::{Method, Request, Response, StatusCode};
use http_body_util::Full;
use octopus_admin::{AppState, DashboardRouter};
use octopus_core::{Error, Result};
use octopus_health::{CircuitBreaker, HealthTracker};
use octopus_metrics::{prometheus::PrometheusExporter, ActivityLog, MetricsCollector};
use octopus_plugin_runtime::PluginManager;
use octopus_router::Router;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tower::ServiceExt;
use tracing::debug;

/// Admin dashboard handler
///
/// This handler wraps the Axum-based DashboardRouter from octopus-admin,
/// providing a bridge between Hyper's Request/Response types and Axum's router.
#[derive(Clone)]
pub struct AdminHandler {
    // The Octopus router (for getting route data)
    router: Arc<Router>,
    request_count: Arc<AtomicUsize>,

    // The Axum router for admin dashboard (handles all /admin routes)
    admin_router: AxumRouter,

    // State needed for metrics display
    #[allow(dead_code)]
    app_state: Arc<AppState>,
    #[allow(dead_code)]
    health_tracker: Option<Arc<HealthTracker>>,
    #[allow(dead_code)]
    circuit_breaker: Option<Arc<CircuitBreaker>>,
    #[allow(dead_code)]
    plugin_manager: Option<Arc<PluginManager>>,
    metrics_collector: Option<Arc<MetricsCollector>>,
    #[allow(dead_code)]
    activity_log: Option<Arc<ActivityLog>>,
}

impl std::fmt::Debug for AdminHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdminHandler")
            .field("request_count", &self.request_count)
            .finish()
    }
}

impl AdminHandler {
    /// Build an AppState populated with real data sources
    fn build_app_state(
        router: &Arc<Router>,
        metrics_collector: &Option<Arc<MetricsCollector>>,
        activity_log: &Option<Arc<ActivityLog>>,
        health_tracker: &Option<Arc<HealthTracker>>,
        circuit_breaker: &Option<Arc<CircuitBreaker>>,
        plugin_manager: &Option<Arc<PluginManager>>,
        farp_registry: &Option<Arc<octopus_farp::SchemaRegistry>>,
        farp_federation: &Option<Arc<octopus_farp::SchemaFederation>>,
        config: &Option<Arc<octopus_config::Config>>,
    ) -> Arc<AppState> {
        let mut state = AppState::new();
        state.router = Some(Arc::clone(router));
        state.metrics = metrics_collector.clone();
        state.activity_log = activity_log.clone();
        state.health_tracker = health_tracker.clone();
        state.circuit_breaker = circuit_breaker.clone();
        state.plugin_manager = plugin_manager.clone();
        state.farp_registry = farp_registry.clone();
        state.farp_federation = farp_federation.clone();
        state.config = config.clone();
        Arc::new(state)
    }

    /// Create a new admin handler
    pub fn new(router: Arc<Router>, request_count: Arc<AtomicUsize>) -> Self {
        let app_state = Self::build_app_state(
            &router, &None, &None, &None, &None, &None, &None, &None, &None,
        );
        let admin_router = DashboardRouter::build(Arc::clone(&app_state));

        Self {
            router,
            request_count,
            admin_router,
            app_state,
            health_tracker: None,
            circuit_breaker: None,
            plugin_manager: None,
            metrics_collector: None,
            activity_log: None,
        }
    }

    /// Create a new admin handler with health monitoring
    pub fn with_health(
        router: Arc<Router>,
        request_count: Arc<AtomicUsize>,
        health_tracker: Arc<HealthTracker>,
        circuit_breaker: Arc<CircuitBreaker>,
    ) -> Self {
        let ht = Some(health_tracker.clone());
        let cb = Some(circuit_breaker.clone());
        let app_state =
            Self::build_app_state(&router, &None, &None, &ht, &cb, &None, &None, &None, &None);
        let admin_router = DashboardRouter::build(Arc::clone(&app_state));

        Self {
            router,
            request_count,
            admin_router,
            app_state,
            health_tracker: Some(health_tracker),
            circuit_breaker: Some(circuit_breaker),
            plugin_manager: None,
            metrics_collector: None,
            activity_log: None,
        }
    }

    /// Create a new admin handler with all features
    pub fn with_all(
        router: Arc<Router>,
        request_count: Arc<AtomicUsize>,
        health_tracker: Option<Arc<HealthTracker>>,
        circuit_breaker: Option<Arc<CircuitBreaker>>,
        plugin_manager: Option<Arc<PluginManager>>,
        metrics_collector: Option<Arc<MetricsCollector>>,
        activity_log: Option<Arc<ActivityLog>>,
        farp_registry: Option<Arc<octopus_farp::SchemaRegistry>>,
        farp_federation: Option<Arc<octopus_farp::SchemaFederation>>,
        config: Option<Arc<octopus_config::Config>>,
    ) -> Self {
        let app_state = Self::build_app_state(
            &router,
            &metrics_collector,
            &activity_log,
            &health_tracker,
            &circuit_breaker,
            &plugin_manager,
            &farp_registry,
            &farp_federation,
            &config,
        );
        let admin_router = DashboardRouter::build(Arc::clone(&app_state));

        Self {
            router,
            request_count,
            admin_router,
            app_state,
            health_tracker,
            circuit_breaker,
            plugin_manager,
            metrics_collector,
            activity_log,
        }
    }

    /// Handle admin routes using the Axum router
    ///
    /// This method now delegates to the DashboardRouter from octopus-admin,
    /// which handles all /admin routes properly. Special routes like /metrics
    /// are handled separately.
    pub async fn handle(&self, method: &Method, path: &str) -> Result<Response<Full<Bytes>>> {
        debug!(method = %method, path = %path, "Handling admin route via Axum router");

        // Handle Prometheus metrics endpoint separately (not part of Axum router)
        if path == "/metrics" || path == "/__metrics" {
            return self.metrics_endpoint();
        }

        // Build a Request<Body> for Axum
        let req_builder = Request::builder().method(method.clone()).uri(path);

        let req = req_builder
            .body(Body::empty())
            .map_err(|e| Error::InvalidRequest(format!("Failed to build request: {}", e)))?;

        // Call the Axum router
        let router = self.admin_router.clone();
        let response = router
            .oneshot(req)
            .await
            .map_err(|e| Error::InvalidRequest(format!("Admin router error: {}", e)))?;

        // Convert Axum Response<Body> to Response<Full<Bytes>>
        let (parts, body) = response.into_parts();

        // Collect the body
        let body_bytes = axum::body::to_bytes(body, usize::MAX)
            .await
            .map_err(|e| Error::InvalidRequest(format!("Failed to read response body: {}", e)))?;

        let response = Response::from_parts(parts, Full::new(body_bytes));
        Ok(response)
    }

    /// Serve Prometheus metrics endpoint
    fn metrics_endpoint(&self) -> Result<Response<Full<Bytes>>> {
        let metrics_text = if let Some(metrics) = &self.metrics_collector {
            PrometheusExporter::export(metrics)
        } else {
            // Fallback: basic metrics
            format!(
                "# HELP octopus_requests_total Total number of requests\n\
                 # TYPE octopus_requests_total counter\n\
                 octopus_requests_total {}\n\
                 # HELP octopus_routes_total Total number of configured routes\n\
                 # TYPE octopus_routes_total gauge\n\
                 octopus_routes_total {}\n",
                self.request_count.load(Ordering::Relaxed),
                self.router.total_route_count()
            )
        };

        Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "text/plain; version=0.0.4; charset=utf-8")
            .body(Full::new(Bytes::from(metrics_text)))
            .map_err(|e| Error::InvalidRequest(format!("Failed to build response: {}", e)))
    }
}
