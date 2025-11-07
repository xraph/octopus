//! HTTP request handler

use crate::admin::AdminHandler;
use bytes::Bytes;
use http::{Request, Response, StatusCode};
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use octopus_core::{middleware::Middleware, Error, Result};
use octopus_farp::FarpApiHandler;
use octopus_health::{CircuitBreaker, HealthTracker};
use octopus_metrics::{ActivityLog, MetricsCollector, RequestOutcome};
use octopus_plugin_runtime::PluginManager;
use octopus_protocols::ProtocolHandler;
use octopus_proxy::HttpProxy;
use octopus_router::Router;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, error, info, warn};

/// Body type alias
pub type Body = Full<Bytes>;

/// HTTP request handler
#[derive(Clone)]
pub struct RequestHandler {
    router: Arc<Router>,
    proxy: Arc<HttpProxy>,
    request_count: Arc<AtomicUsize>,
    admin_handler: AdminHandler,
    middleware_chain: Arc<[Arc<dyn Middleware>]>,
    farp_handler: Option<Arc<FarpApiHandler>>,
    protocol_handlers: Arc<[Arc<dyn ProtocolHandler>]>,
    metrics_collector: Arc<MetricsCollector>,
    activity_log: Arc<ActivityLog>,
}

impl std::fmt::Debug for RequestHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RequestHandler")
            .field("request_count", &self.request_count)
            .field("middleware_count", &self.middleware_chain.len())
            .field("protocol_handlers_count", &self.protocol_handlers.len())
            .finish()
    }
}

impl RequestHandler {
    /// Create a new request handler
    pub fn new(
        router: Arc<Router>,
        proxy: Arc<HttpProxy>,
        request_count: Arc<AtomicUsize>,
    ) -> Self {
        let metrics_collector = Arc::new(MetricsCollector::new());
        let activity_log = Arc::new(ActivityLog::default());

        let admin_handler = AdminHandler::new(Arc::clone(&router), Arc::clone(&request_count));

        Self {
            router,
            proxy,
            request_count,
            admin_handler,
            middleware_chain: Arc::new([]), // Empty chain by default
            farp_handler: None,
            protocol_handlers: Arc::new([]),
            metrics_collector,
            activity_log,
        }
    }

    /// Create a new request handler with all features
    pub fn with_features(
        router: Arc<Router>,
        proxy: Arc<HttpProxy>,
        request_count: Arc<AtomicUsize>,
        middleware_chain: Arc<[Arc<dyn Middleware>]>,
        farp_handler: Option<Arc<FarpApiHandler>>,
        protocol_handlers: Arc<[Arc<dyn ProtocolHandler>]>,
    ) -> Self {
        let metrics_collector = Arc::new(MetricsCollector::new());
        let activity_log = Arc::new(ActivityLog::default());

        // For now, create AdminHandler without health tracker and plugin manager
        // These will be None until we properly wire them from the proxy/health system
        let admin_handler = AdminHandler::with_all(
            Arc::clone(&router),
            Arc::clone(&request_count),
            None, // health_tracker - Get from proxy if available
            None, // circuit_breaker - Get from proxy if available
            None, // plugin_manager - Pass from server
            Some(Arc::clone(&metrics_collector)),
            Some(Arc::clone(&activity_log)),
        );

        Self {
            router,
            proxy,
            request_count,
            admin_handler,
            middleware_chain,
            farp_handler,
            protocol_handlers,
            metrics_collector,
            activity_log,
        }
    }

    /// Create a new request handler with all features including plugin manager
    pub fn with_all_features(
        router: Arc<Router>,
        proxy: Arc<HttpProxy>,
        request_count: Arc<AtomicUsize>,
        middleware_chain: Arc<[Arc<dyn Middleware>]>,
        farp_handler: Option<Arc<FarpApiHandler>>,
        protocol_handlers: Arc<[Arc<dyn ProtocolHandler>]>,
        health_tracker: Option<Arc<HealthTracker>>,
        circuit_breaker: Option<Arc<CircuitBreaker>>,
        plugin_manager: Option<Arc<PluginManager>>,
        metrics_collector: Arc<MetricsCollector>,
        activity_log: Arc<ActivityLog>,
    ) -> Self {
        let admin_handler = AdminHandler::with_all(
            Arc::clone(&router),
            Arc::clone(&request_count),
            health_tracker,
            circuit_breaker,
            plugin_manager,
            Some(Arc::clone(&metrics_collector)),
            Some(Arc::clone(&activity_log)),
        );

        Self {
            router,
            proxy,
            request_count,
            admin_handler,
            middleware_chain,
            farp_handler,
            protocol_handlers,
            metrics_collector,
            activity_log,
        }
    }

    /// Create a new request handler with middleware chain
    pub fn with_middleware(
        router: Arc<Router>,
        proxy: Arc<HttpProxy>,
        request_count: Arc<AtomicUsize>,
        middleware_chain: Arc<[Arc<dyn Middleware>]>,
    ) -> Self {
        let metrics_collector = Arc::new(MetricsCollector::new());
        let activity_log = Arc::new(ActivityLog::default());

        let admin_handler = AdminHandler::new(Arc::clone(&router), Arc::clone(&request_count));

        Self {
            router,
            proxy,
            request_count,
            admin_handler,
            middleware_chain,
            farp_handler: None,
            protocol_handlers: Arc::new([]),
            metrics_collector,
            activity_log,
        }
    }

    /// Get the metrics collector
    pub fn metrics_collector(&self) -> Arc<MetricsCollector> {
        Arc::clone(&self.metrics_collector)
    }

    /// Get the activity log
    pub fn activity_log(&self) -> Arc<ActivityLog> {
        Arc::clone(&self.activity_log)
    }

    /// Handle an incoming HTTP request (from Hyper with Incoming body)
    pub async fn handle(&self, req: Request<Incoming>) -> Result<Response<Body>> {
        // Increment request counter
        self.request_count.fetch_add(1, Ordering::Relaxed);

        let method = req.method().clone();
        let path = req.uri().path().to_string();

        debug!(
            method = %method,
            path = %path,
            "Handling request"
        );

        // Handle internal API routes (built-in, not proxied)
        // Internal routes use __ prefix by default
        if path.starts_with("/__admin") {
            // Remove __ prefix before passing to admin handler
            let internal_path = path.replacen("/__admin", "/admin", 1);
            return self.admin_handler.handle(&method, &internal_path);
        }

        // Also support legacy /admin paths for backwards compatibility
        if path.starts_with("/admin") {
            return self.admin_handler.handle(&method, &path);
        }

        // Convert Incoming body to Full<Bytes>
        let (parts, body) = req.into_parts();
        let body_bytes = body
            .collect()
            .await
            .map_err(|e| Error::InvalidRequest(format!("Failed to read request body: {}", e)))?
            .to_bytes();
        let req = Request::from_parts(parts, Full::new(body_bytes));

        // Handle internal FARP API routes (with __ prefix)
        // Support both /__farp and /__/farp patterns
        if path.starts_with("/__/farp") || path.starts_with("/__farp") {
            if let Some(farp_handler) = &self.farp_handler {
                debug!("Routing to FARP handler (internal)");
                // Remove __ prefix before passing to FARP handler
                let internal_path = if path.starts_with("/__/farp") {
                    path.replacen("/__/farp", "/farp", 1)
                } else {
                    path.replacen("/__farp", "/farp", 1)
                };
                let (parts, body) = req.into_parts();
                let mut builder = http::Request::builder()
                    .method(parts.method)
                    .version(parts.version);

                // Copy headers
                for (key, value) in parts.headers.iter() {
                    builder = builder.header(key, value);
                }

                let internal_req = builder.uri(internal_path).body(body).map_err(|e| {
                    Error::InvalidRequest(format!("Failed to build request: {}", e))
                })?;

                return farp_handler.handle(internal_req).await;
            }
        }

        // Also support legacy /farp paths for backwards compatibility
        if path.starts_with("/farp") {
            if let Some(farp_handler) = &self.farp_handler {
                debug!("Routing to FARP handler");
                return farp_handler.handle(req).await;
            }
        }

        // Check protocol handlers (WebSocket, gRPC, GraphQL)
        for handler in self.protocol_handlers.iter() {
            if handler.can_handle(&req) {
                debug!(
                    protocol = %handler.protocol_type(),
                    "Routing to protocol handler"
                );
                return handler.handle(req).await;
            }
        }

        // Execute middleware chain if configured
        if !self.middleware_chain.is_empty() {
            debug!(
                middleware_count = self.middleware_chain.len(),
                "Executing middleware chain"
            );

            // Create final handler closure
            let handler = self.clone();
            let final_handler = Box::new(move |req: Request<Body>| {
                let handler = handler.clone();
                Box::pin(async move { handler.handle_proxy_request(req).await })
                    as std::pin::Pin<
                        Box<dyn std::future::Future<Output = Result<Response<Body>>> + Send>,
                    >
            });

            // Execute middleware chain with final handler
            let next = octopus_core::middleware::Next::with_handler(
                Arc::clone(&self.middleware_chain),
                final_handler,
            );
            return next.run(req).await;
        }

        // No middleware, handle directly
        self.handle_proxy_request(req).await
    }

    /// Handle the actual proxying logic (called after middleware)
    async fn handle_proxy_request(&self, req: Request<Body>) -> Result<Response<Body>> {
        let start_time = Instant::now();
        let method = req.method().clone();
        let path = req.uri().path().to_string();

        // Track active connections
        self.metrics_collector.increment_active_connections();

        // Find matching route
        let route = match self.router.find_route(&method, &path) {
            Ok(route) => route,
            Err(e) => {
                let latency = start_time.elapsed();
                warn!(
                    method = %method,
                    path = %path,
                    error = %e,
                    "No route found"
                );

                // Record failed request
                self.metrics_collector
                    .record_request(&path, latency, RequestOutcome::Error);
                self.activity_log.record(
                    method.clone(),
                    path.clone(),
                    StatusCode::NOT_FOUND,
                    latency,
                    "none".to_string(),
                );
                self.metrics_collector.decrement_active_connections();

                return self.error_response(StatusCode::NOT_FOUND, "Route not found");
            }
        };

        debug!(
            upstream = %route.upstream_name,
            "Route matched"
        );

        // Get upstream instance
        let instance = match self.router.select_instance(&route.upstream_name) {
            Ok(instance) => instance,
            Err(e) => {
                let latency = start_time.elapsed();
                error!(
                    upstream = %route.upstream_name,
                    error = %e,
                    "Failed to select upstream instance"
                );

                // Record failed request
                self.metrics_collector
                    .record_request(&path, latency, RequestOutcome::Error);
                self.activity_log.record(
                    method.clone(),
                    path.clone(),
                    StatusCode::SERVICE_UNAVAILABLE,
                    latency,
                    route.upstream_name.clone(),
                );
                self.metrics_collector.decrement_active_connections();

                return self.error_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "No healthy upstream available",
                );
            }
        };

        debug!(
            instance_id = %instance.id,
            address = %instance.address,
            port = instance.port,
            "Upstream instance selected"
        );

        // Proxy the request
        let result = self.proxy.proxy(req, &instance).await;
        let latency = start_time.elapsed();

        // Decrement active connections
        self.metrics_collector.decrement_active_connections();

        match result {
            Ok(response) => {
                let status = response.status();
                let outcome = if status.is_success() {
                    RequestOutcome::Success
                } else {
                    RequestOutcome::Error
                };

                // Record successful request
                self.metrics_collector
                    .record_request(&path, latency, outcome);
                self.activity_log.record(
                    method.clone(),
                    path.clone(),
                    status,
                    latency,
                    route.upstream_name.clone(),
                );

                info!(
                    method = %method,
                    path = %path,
                    status = status.as_u16(),
                    latency_ms = %latency.as_millis(),
                    "Request completed"
                );
                Ok(response)
            }
            Err(e) => {
                // Record failed request
                self.metrics_collector
                    .record_request(&path, latency, RequestOutcome::Error);
                self.activity_log.record(
                    method.clone(),
                    path.clone(),
                    StatusCode::BAD_GATEWAY,
                    latency,
                    route.upstream_name.clone(),
                );

                error!(
                    method = %method,
                    path = %path,
                    error = %e,
                    latency_ms = %latency.as_millis(),
                    "Proxy error"
                );
                self.error_response(StatusCode::BAD_GATEWAY, "Upstream error")
            }
        }
    }

    /// Create an error response
    fn error_response(&self, status: StatusCode, message: &str) -> Result<Response<Body>> {
        Response::builder()
            .status(status)
            .header("content-type", "text/plain")
            .body(Full::new(Bytes::from(message.to_string())))
            .map_err(|e| Error::Internal(format!("Failed to build error response: {}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use octopus_proxy::{ConnectionPool, HttpClient, PoolConfig, ProxyConfig};
    use std::time::Duration;

    fn create_test_handler() -> RequestHandler {
        let router = Arc::new(Router::new());
        let pool = Arc::new(ConnectionPool::new(PoolConfig::default()));
        let client = HttpClient::with_timeout(Duration::from_secs(30));
        let proxy = Arc::new(HttpProxy::new(client, pool, ProxyConfig::default()));
        let request_count = Arc::new(AtomicUsize::new(0));

        RequestHandler::new(router, proxy, request_count)
    }

    #[test]
    fn test_handler_creation() {
        let handler = create_test_handler();
        assert_eq!(handler.request_count.load(Ordering::Relaxed), 0);
    }
}
