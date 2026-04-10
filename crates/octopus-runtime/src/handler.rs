//! HTTP request handler

use crate::admin::AdminHandler;
use bytes::Bytes;
use http::{Request, Response, StatusCode};
use http_body_util::{BodyExt, Either, Full};
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

/// Body type — Left for buffered, Right for streaming (SSE / chunked)
pub type Body = Either<Full<Bytes>, Incoming>;

/// Create a buffered body from data
fn buffered(data: impl Into<Bytes>) -> Body {
    Either::Left(Full::new(data.into()))
}

/// Create a streaming body from an Incoming response
fn streaming(incoming: Incoming) -> Body {
    Either::Right(incoming)
}

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
    /// Active WebSocket connection count for graceful shutdown coordination
    ws_active_count: Arc<AtomicUsize>,
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
            ws_active_count: Arc::new(AtomicUsize::new(0)),
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
            None, // farp_registry
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
            ws_active_count: Arc::new(AtomicUsize::new(0)),
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
        let farp_registry = farp_handler.as_ref().map(|h| Arc::clone(h.registry()));
        let admin_handler = AdminHandler::with_all(
            Arc::clone(&router),
            Arc::clone(&request_count),
            health_tracker,
            circuit_breaker,
            plugin_manager,
            Some(Arc::clone(&metrics_collector)),
            Some(Arc::clone(&activity_log)),
            farp_registry,
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
            ws_active_count: Arc::new(AtomicUsize::new(0)),
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
            ws_active_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Get the number of active WebSocket connections
    pub fn active_ws_connections(&self) -> usize {
        self.ws_active_count.load(Ordering::Relaxed)
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
            let internal_path = path.replacen("/__admin", "/admin", 1);
            return self
                .admin_handler
                .handle(&method, &internal_path)
                .await
                .map(|r| r.map(Either::Left));
        }

        // Also support legacy /admin paths for backwards compatibility
        if path.starts_with("/admin") {
            return self
                .admin_handler
                .handle(&method, &path)
                .await
                .map(|r| r.map(Either::Left));
        }

        // ── WebSocket upgrade ─────────────────────────────────────────
        // Must intercept BEFORE body buffering so the hyper OnUpgrade
        // extension is still in the request.
        if octopus_protocols::is_websocket_upgrade(&req) {
            return self.handle_websocket_upgrade(req).await;
        }

        // ── SSE streaming proxy ──────────────────────────────────────
        // Must intercept BEFORE body buffering so we can stream the response.
        if req
            .headers()
            .get(http::header::ACCEPT)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.contains("text/event-stream"))
        {
            return self.handle_sse_proxy(req).await;
        }

        // Convert Incoming body to Full<Bytes>
        let (parts, body) = req.into_parts();
        let body_bytes = body
            .collect()
            .await
            .map_err(|e| Error::InvalidRequest(format!("Failed to read request body: {}", e)))?
            .to_bytes();
        let req = Request::from_parts(parts, Full::new(body_bytes));

        // Handle FARP v1 push protocol routes (/_farp/v1/*)
        // Per FARP spec: /_farp/v1/register, /_farp/v1/heartbeat/{id}, etc.
        if path.starts_with("/_farp/v1") {
            if let Some(farp_handler) = &self.farp_handler {
                debug!("Routing to FARP handler (v1 push protocol)");
                let internal_path = path.replacen("/_farp/v1", "/farp", 1);
                let (parts, body) = req.into_parts();
                let mut builder = http::Request::builder()
                    .method(parts.method)
                    .version(parts.version);

                for (key, value) in parts.headers.iter() {
                    builder = builder.header(key, value);
                }

                let internal_req = builder.uri(internal_path).body(body).map_err(|e| {
                    Error::InvalidRequest(format!("Failed to build request: {}", e))
                })?;

                return farp_handler
                    .handle(internal_req)
                    .await
                    .map(|r| r.map(Either::Left));
            }
        }

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

                return farp_handler
                    .handle(internal_req)
                    .await
                    .map(|r| r.map(Either::Left));
            }
        }

        // Also support legacy /farp paths for backwards compatibility
        if path.starts_with("/farp") {
            if let Some(farp_handler) = &self.farp_handler {
                debug!("Routing to FARP handler");
                return farp_handler
                    .handle(req)
                    .await
                    .map(|r| r.map(Either::Left));
            }
        }

        // Root-level documentation routes (/swagger, /docs, /redoc)
        if path == "/swagger" || path == "/docs" || path == "/redoc" {
            if let Some(farp_handler) = &self.farp_handler {
                debug!("Routing to FARP docs handler");
                let internal_path = if path == "/redoc" {
                    "/farp/redoc".to_string()
                } else {
                    "/farp/docs".to_string()
                };
                let (parts, body) = req.into_parts();
                let mut builder = http::Request::builder()
                    .method(parts.method)
                    .version(parts.version);

                for (key, value) in parts.headers.iter() {
                    builder = builder.header(key, value);
                }

                let internal_req =
                    builder.uri(internal_path).body(body).map_err(|e| {
                        Error::InvalidRequest(format!("Failed to build request: {}", e))
                    })?;

                return farp_handler
                    .handle(internal_req)
                    .await
                    .map(|r| r.map(Either::Left));
            }
        }

        // Check protocol handlers (WebSocket, gRPC, GraphQL)
        for handler in self.protocol_handlers.iter() {
            if handler.can_handle(&req) {
                debug!(
                    protocol = %handler.protocol_type(),
                    "Routing to protocol handler"
                );
                return handler
                    .handle(req)
                    .await
                    .map(|r| r.map(Either::Left));
            }
        }

        // Execute middleware chain if configured
        if !self.middleware_chain.is_empty() {
            debug!(
                middleware_count = self.middleware_chain.len(),
                "Executing middleware chain"
            );

            // Create final handler closure
            // Middleware chain operates on Full<Bytes> bodies (octopus_core::middleware::Body)
            let handler = self.clone();
            let final_handler =
                Box::new(move |req: Request<octopus_core::middleware::Body>| {
                    let handler = handler.clone();
                    Box::pin(async move { handler.handle_proxy_request(req).await })
                        as std::pin::Pin<
                            Box<
                                dyn std::future::Future<
                                        Output = Result<
                                            Response<octopus_core::middleware::Body>,
                                        >,
                                    > + Send,
                            >,
                        >
                });

            // Execute middleware chain with final handler
            let next = octopus_core::middleware::Next::with_handler(
                Arc::clone(&self.middleware_chain),
                final_handler,
            );
            return next.run(req).await.map(|r| r.map(Either::Left));
        }

        // No middleware, handle directly
        self.handle_proxy_request(req).await.map(|r| r.map(Either::Left))
    }

    /// Handle WebSocket upgrade requests.
    ///
    /// Called BEFORE body buffering so hyper's `OnUpgrade` extension is preserved.
    ///
    /// Flow:
    /// 1. Route match → select upstream instance
    /// 2. Build forwarded headers (X-Forwarded-For, Origin, Cookie, etc.)
    /// 3. **Connect to upstream WS first** (with timeout) — fail fast with 502 if unreachable
    /// 4. Only on success → build 101 response, extract OnUpgrade
    /// 5. Spawn background proxy task with already-connected upstream
    /// 6. Return 101 to client
    async fn handle_websocket_upgrade(
        &self,
        mut req: Request<Incoming>,
    ) -> Result<Response<Body>> {
        let method = req.method().clone();
        let path = req.uri().path().to_string();

        tracing::info!(path = %path, "WebSocket upgrade request");

        // 1. Route match
        let route = self.router.find_route(&method, &path).map_err(|e| {
            tracing::warn!(path = %path, error = %e, "No route for WebSocket");
            Error::RouteNotFound(format!("No route for WebSocket path: {path}"))
        })?;

        // Select upstream instance
        let instance = self.router.select_instance(&route.upstream_name).map_err(|e| {
            tracing::error!(upstream = %route.upstream_name, error = %e, "No upstream for WebSocket");
            Error::NoHealthyUpstream
        })?;

        // Build upstream WebSocket URL with path rewriting
        let upstream_base = instance.base_url();
        let mut upstream_path = path.clone();
        if let Some(ref prefix) = route.strip_prefix {
            if let Some(stripped) = upstream_path.strip_prefix(prefix.as_str()) {
                upstream_path = stripped.to_string();
            }
        }
        if let Some(ref prefix) = route.add_prefix {
            upstream_path = format!("{prefix}{upstream_path}");
        }
        let upstream_ws_url = upstream_base
            .replace("http://", "ws://")
            .replace("https://", "wss://");
        let upstream_url = format!("{upstream_ws_url}{upstream_path}");

        // 2. Build forwarded headers from client request
        let forwarded_headers = octopus_protocols::build_forwarded_headers(&req);

        // 3. Connect to upstream FIRST — fail fast with 502 if unreachable
        let config = octopus_protocols::WebSocketConfig::default();
        let upstream_stream = octopus_protocols::connect_upstream(
            &upstream_url,
            &forwarded_headers,
            &config,
        )
        .await
        .map_err(|e| {
            tracing::error!(upstream = %upstream_url, error = %e, "Upstream WebSocket connect failed");
            Error::UpstreamConnection(e)
        })?;

        // 4. Upstream connected — now build 101 response (validates handshake)
        let response = octopus_protocols::build_upgrade_response(&req)
            .map_err(|e| Error::InvalidRequest(e))?;

        // Extract the upgrade future BEFORE returning the response
        let on_upgrade = hyper::upgrade::on(&mut req);

        // Track connection
        instance.increment_connections();
        let instance_for_cleanup = instance.clone();
        let metrics = self.metrics_collector.clone();
        let route_key = format!("WS {path}");
        let ws_count = self.ws_active_count.clone();
        ws_count.fetch_add(1, Ordering::Relaxed);

        // 5. Spawn proxy task with already-connected upstream
        let ws_config = config.to_tungstenite_config();
        tokio::spawn(async move {
            match on_upgrade.await {
                Ok(upgraded) => {
                    tracing::debug!(upstream = %upstream_url, "WebSocket upgrade complete");

                    // Wrap upgraded client connection with size limits
                    let io = hyper_util::rt::TokioIo::new(upgraded);
                    let client_ws = tokio_tungstenite::WebSocketStream::from_raw_socket(
                        io,
                        tokio_tungstenite::tungstenite::protocol::Role::Server,
                        Some(ws_config),
                    )
                    .await;

                    // Run bidirectional proxy
                    match octopus_protocols::proxy_websocket_connected(
                        client_ws,
                        upstream_stream,
                        &config,
                    )
                    .await
                    {
                        Ok(stats) => {
                            tracing::info!(
                                c2u = stats.client_to_upstream,
                                u2c = stats.upstream_to_client,
                                bytes = stats.bytes_transferred,
                                ms = stats.duration.as_millis() as u64,
                                "WebSocket proxy completed"
                            );
                            metrics.record_request(&route_key, stats.duration, RequestOutcome::Success);
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "WebSocket proxy error");
                            metrics.record_request(&route_key, std::time::Duration::ZERO, RequestOutcome::Error);
                        }
                    }
                }
                Err(e) => {
                    tracing::error!(error = %e, "WebSocket upgrade failed");
                }
            }
            // Cleanup
            instance_for_cleanup.decrement_connections();
            ws_count.fetch_sub(1, Ordering::Relaxed);
        });

        // 6. Return 101 Switching Protocols
        Ok(response.map(|_| buffered(Bytes::new())))
    }

    /// Handle SSE (Server-Sent Events) proxy requests.
    ///
    /// Called BEFORE body buffering so the response body can be streamed
    /// back to the client without being fully collected first.
    async fn handle_sse_proxy(&self, req: Request<Incoming>) -> Result<Response<Body>> {
        let method = req.method().clone();
        let path = req.uri().path().to_string();

        tracing::info!(path = %path, "SSE streaming proxy request");

        // Route match
        let route = self.router.find_route(&method, &path).map_err(|e| {
            tracing::warn!(path = %path, error = %e, "No route for SSE");
            Error::RouteNotFound(format!("No route for SSE path: {path}"))
        })?;

        // Select upstream instance
        let instance = self.router.select_instance(&route.upstream_name).map_err(|e| {
            tracing::error!(upstream = %route.upstream_name, error = %e, "No upstream for SSE");
            Error::NoHealthyUpstream
        })?;

        // Build upstream URL with path rewriting
        let mut upstream_path = path.clone();
        if let Some(ref prefix) = route.strip_prefix {
            if let Some(stripped) = upstream_path.strip_prefix(prefix.as_str()) {
                upstream_path = stripped.to_string();
            }
        }
        if let Some(ref prefix) = route.add_prefix {
            upstream_path = format!("{prefix}{upstream_path}");
        }
        let upstream_url = format!("{}{}", instance.base_url(), upstream_path);

        // Build upstream request, forwarding relevant headers
        let (parts, _body) = req.into_parts();
        let mut upstream_builder = http::Request::builder()
            .method(&parts.method)
            .uri(&upstream_url);

        // Forward headers: Accept, Last-Event-ID, Authorization, Cookie, X-Forwarded-*
        for name in &[
            "accept",
            "last-event-id",
            "authorization",
            "cookie",
            "x-forwarded-for",
            "x-real-ip",
            "content-type",
        ] {
            if let Some(val) = parts.headers.get(*name) {
                upstream_builder = upstream_builder.header(*name, val);
            }
        }

        let upstream_req = upstream_builder
            .body(Full::new(Bytes::new()))
            .map_err(|e| Error::Internal(format!("Failed to build SSE upstream request: {e}")))?;

        // Use hyper client to send request and get streaming response
        use hyper_util::client::legacy::Client;
        use hyper_util::rt::TokioExecutor;
        let client = Client::builder(TokioExecutor::new()).build_http::<Full<Bytes>>();

        let upstream_resp = client.request(upstream_req).await.map_err(|e| {
            Error::UpstreamConnection(format!("SSE upstream failed: {e}"))
        })?;

        // Return the streaming response directly -- no body buffering
        let (resp_parts, body) = upstream_resp.into_parts();
        Ok(Response::from_parts(resp_parts, streaming(body)))
    }

    /// Handle the actual proxying logic (called after middleware)
    ///
    /// Uses `Full<Bytes>` explicitly because this is always a buffered path —
    /// streaming (SSE) is handled separately before reaching here.
    async fn handle_proxy_request(
        &self,
        req: Request<Full<Bytes>>,
    ) -> Result<Response<Full<Bytes>>> {
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

                // Convert Response<Incoming> to Response<Body>
                let (parts, body) = response.into_parts();
                let body_bytes = body.collect().await?.to_bytes();
                let full_body = Full::new(body_bytes);
                Ok(Response::from_parts(parts, full_body))
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

    /// Create a buffered error response
    fn error_response(
        &self,
        status: StatusCode,
        message: &str,
    ) -> Result<Response<Full<Bytes>>> {
        Response::builder()
            .status(status)
            .header("content-type", "text/plain")
            .body(Full::new(Bytes::from(message.to_string())))
            .map_err(|e| Error::Internal(format!("Failed to build error response: {}", e)))
    }

    /// Create a streaming-typed error response (for use in contexts returning `Body`)
    #[allow(dead_code)]
    fn error_body_response(
        &self,
        status: StatusCode,
        message: &str,
    ) -> Result<Response<Body>> {
        Response::builder()
            .status(status)
            .header("content-type", "text/plain")
            .body(buffered(message.to_string()))
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
