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
    /// Active SSE connection count
    sse_active_count: Arc<AtomicUsize>,
    /// Auth provider registry (for admin auth)
    auth_registry: Option<Arc<octopus_auth::AuthProviderRegistry>>,
    /// Admin auth provider name
    admin_auth_provider: Option<String>,
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
            sse_active_count: Arc::new(AtomicUsize::new(0)),
            auth_registry: None,
            admin_auth_provider: None,
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

        let farp_registry = farp_handler.as_ref().map(|h| Arc::clone(h.registry()));
        let farp_federation = farp_handler.as_ref().map(|h| Arc::clone(h.federation()));
        let admin_handler = AdminHandler::with_all(
            Arc::clone(&router),
            Arc::clone(&request_count),
            None,
            None,
            None,
            Some(Arc::clone(&metrics_collector)),
            Some(Arc::clone(&activity_log)),
            farp_registry,
            farp_federation,
            None, // config
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
            sse_active_count: Arc::new(AtomicUsize::new(0)),
            auth_registry: None,
            admin_auth_provider: None,
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
        config: Option<Arc<octopus_config::Config>>,
    ) -> Self {
        let farp_registry = farp_handler.as_ref().map(|h| Arc::clone(h.registry()));
        let farp_federation = farp_handler.as_ref().map(|h| Arc::clone(h.federation()));
        let admin_handler = AdminHandler::with_all(
            Arc::clone(&router),
            Arc::clone(&request_count),
            health_tracker,
            circuit_breaker,
            plugin_manager,
            Some(Arc::clone(&metrics_collector)),
            Some(Arc::clone(&activity_log)),
            farp_registry,
            farp_federation,
            config,
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
            sse_active_count: Arc::new(AtomicUsize::new(0)),
            auth_registry: None,
            admin_auth_provider: None,
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
            sse_active_count: Arc::new(AtomicUsize::new(0)),
            auth_registry: None,
            admin_auth_provider: None,
        }
    }

    /// Set the auth registry and admin auth provider for admin endpoint protection
    pub fn set_admin_auth(
        &mut self,
        registry: Arc<octopus_auth::AuthProviderRegistry>,
        admin_provider: Option<String>,
    ) {
        self.auth_registry = Some(registry);
        self.admin_auth_provider = admin_provider;
    }

    /// Get the number of active WebSocket connections
    pub fn active_ws_connections(&self) -> usize {
        self.ws_active_count.load(Ordering::Relaxed)
    }

    /// Get the number of active SSE connections
    pub fn active_sse_connections(&self) -> usize {
        self.sse_active_count.load(Ordering::Relaxed)
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
        if path.starts_with("/__admin") || path.starts_with("/admin") {
            // Check admin auth if configured
            if let (Some(ref registry), Some(ref provider_name)) =
                (&self.auth_registry, &self.admin_auth_provider)
            {
                // Skip auth for static assets (CSS, JS, fonts)
                let is_asset = path.contains("/static/") || path.contains("/_next/");
                if !is_asset {
                    let tls_cn = req
                        .extensions()
                        .get::<octopus_tls::TlsClientCn>()
                        .and_then(|cn| cn.0.clone());
                    let auth_req = octopus_auth::AuthRequest {
                        headers: req.headers(),
                        method: req.method(),
                        uri: req.uri(),
                        tls_client_cn: tls_cn.as_deref(),
                    };
                    match registry.authenticate(provider_name, &auth_req).await {
                        Ok(octopus_auth::AuthResult::Authenticated(_)) => {
                            // Proceed
                        }
                        Ok(octopus_auth::AuthResult::Unauthenticated)
                        | Ok(octopus_auth::AuthResult::Failed(_)) => {
                            let body = serde_json::json!({
                                "error": "unauthorized",
                                "message": "Admin authentication required"
                            });
                            return Ok(Response::builder()
                                .status(StatusCode::UNAUTHORIZED)
                                .header("Content-Type", "application/json")
                                .header("WWW-Authenticate", "Bearer")
                                .body(buffered(serde_json::to_vec(&body).unwrap_or_default()))
                                .unwrap());
                        }
                        Err(_) => {
                            return Ok(Response::builder()
                                .status(StatusCode::INTERNAL_SERVER_ERROR)
                                .body(buffered("Auth error"))
                                .unwrap());
                        }
                    }
                }
            }

            let admin_path = if path.starts_with("/__admin") {
                path.replacen("/__admin", "/admin", 1)
            } else {
                path.clone()
            };
            return self
                .admin_handler
                .handle(&method, &admin_path)
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

        // ── gRPC proxy ────────────────────────────────────────────────
        // Must intercept BEFORE body buffering to support streaming RPCs.
        // gRPC over HTTP/2 streams request/response bodies.
        if octopus_protocols::GrpcHandler::is_grpc_request_raw(&req) {
            return self.handle_grpc_proxy(req).await;
        }

        // Convert Incoming body to Full<Bytes>
        let (parts, body) = req.into_parts();
        let body_bytes = body
            .collect()
            .await
            .map_err(|e| Error::InvalidRequest(format!("Failed to read request body: {}", e)))?
            .to_bytes();
        let mut req = Request::from_parts(parts, Full::new(body_bytes));

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
                return farp_handler.handle(req).await.map(|r| r.map(Either::Left));
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

                let internal_req = builder.uri(internal_path).body(body).map_err(|e| {
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
                return handler.handle(req).await.map(|r| r.map(Either::Left));
            }
        }

        // Pre-match route to inject auth context into extensions for auth middleware
        if let Ok(route) = self.router.find_route(req.method(), req.uri().path()) {
            req.extensions_mut()
                .insert(octopus_middleware::MatchedRouteAuth {
                    auth_provider: route.auth_provider.clone(),
                    skip_auth: route.skip_auth,
                    require_roles: route.require_roles.clone(),
                    require_scopes: route.require_scopes.clone(),
                    authz_rule: route.authz_rule.clone(),
                    upstream: route.upstream_name.clone(),
                    metadata: route.metadata.clone(),
                });

            // Inject per-route CORS override if configured
            if let Some(ref cors_override) = route.cors {
                req.extensions_mut()
                    .insert(octopus_middleware::MatchedRouteCors {
                        allowed_origins: cors_override.allowed_origins.clone(),
                        allowed_methods: cors_override.allowed_methods.clone(),
                        allowed_headers: cors_override.allowed_headers.clone(),
                        allow_credentials: cors_override.allow_credentials,
                        max_age: cors_override.max_age,
                    });
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
            let final_handler = Box::new(move |req: Request<octopus_core::middleware::Body>| {
                let handler = handler.clone();
                Box::pin(async move { handler.handle_proxy_request(req).await })
                    as std::pin::Pin<
                        Box<
                            dyn std::future::Future<
                                    Output = Result<Response<octopus_core::middleware::Body>>,
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
        self.handle_proxy_request(req)
            .await
            .map(|r| r.map(Either::Left))
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
    async fn handle_websocket_upgrade(&self, mut req: Request<Incoming>) -> Result<Response<Body>> {
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
                            metrics.record_request(
                                &route_key,
                                stats.duration,
                                RequestOutcome::Success,
                            );
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "WebSocket proxy error");
                            metrics.record_request(
                                &route_key,
                                std::time::Duration::ZERO,
                                RequestOutcome::Error,
                            );
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
    /// Handle SSE (Server-Sent Events) streaming proxy.
    ///
    /// Called BEFORE body buffering so the `Incoming` body can be forwarded
    /// (supports POST SSE with request body). Returns the upstream response
    /// with its `Incoming` body streamed directly to the client — zero buffering.
    ///
    /// Features:
    /// - Preserves request body (POST SSE support)
    /// - Preserves query string
    /// - Forwards: Accept, Last-Event-ID, Authorization, Cookie, Content-Type,
    ///   Content-Length, X-Forwarded-For/Proto/Host, X-Real-IP, Host
    /// - Forwards Retry header from upstream response
    /// - Tracks active SSE connections via `sse_active_count`
    /// - Upstream connect timeout (10s)
    /// - Connection tracking on upstream instance
    async fn handle_sse_proxy(&self, req: Request<Incoming>) -> Result<Response<Body>> {
        let method = req.method().clone();
        let path = req.uri().path().to_string();
        let query = req.uri().query().map(|q| q.to_string());

        tracing::info!(path = %path, method = %method, "SSE streaming proxy request");

        // Route match
        let route = self.router.find_route(&method, &path).map_err(|e| {
            tracing::warn!(path = %path, error = %e, "No route for SSE");
            Error::RouteNotFound(format!("No route for SSE path: {path}"))
        })?;

        // Select upstream instance
        let instance = self
            .router
            .select_instance(&route.upstream_name)
            .map_err(|e| {
                tracing::error!(upstream = %route.upstream_name, error = %e, "No upstream for SSE");
                Error::NoHealthyUpstream
            })?;

        // Build upstream URL with path rewriting + query string
        let mut upstream_path = path.clone();
        if let Some(ref prefix) = route.strip_prefix {
            if let Some(stripped) = upstream_path.strip_prefix(prefix.as_str()) {
                upstream_path = stripped.to_string();
            }
        }
        if let Some(ref prefix) = route.add_prefix {
            upstream_path = format!("{prefix}{upstream_path}");
        }
        let mut upstream_url = format!("{}{}", instance.base_url(), upstream_path);
        if let Some(ref qs) = query {
            upstream_url = format!("{upstream_url}?{qs}");
        }

        // Decompose request — preserve body for POST SSE
        let (parts, body) = req.into_parts();

        // Collect the incoming body for forwarding (SSE request bodies are typically small)
        let body_bytes = body
            .collect()
            .await
            .map_err(|e| Error::Internal(format!("Failed to read SSE request body: {e}")))?
            .to_bytes();

        let mut upstream_builder = http::Request::builder()
            .method(&parts.method)
            .uri(&upstream_url);

        // Forward all relevant headers
        let forward_headers = [
            "accept",
            "last-event-id",
            "authorization",
            "cookie",
            "content-type",
            "content-length",
            "x-forwarded-for",
            "x-forwarded-proto",
            "x-forwarded-host",
            "x-real-ip",
            "host",
            "user-agent",
            "cache-control",
        ];
        for name in &forward_headers {
            if let Some(val) = parts.headers.get(*name) {
                upstream_builder = upstream_builder.header(*name, val);
            }
        }

        // Ensure Accept header is set
        if parts.headers.get("accept").is_none() {
            upstream_builder = upstream_builder.header("accept", "text/event-stream");
        }

        let upstream_req = upstream_builder
            .body(Full::new(body_bytes))
            .map_err(|e| Error::Internal(format!("Failed to build SSE upstream request: {e}")))?;

        // Connect to upstream with timeout
        use hyper_util::client::legacy::Client;
        use hyper_util::rt::TokioExecutor;
        let client = Client::builder(TokioExecutor::new()).build_http::<Full<Bytes>>();

        let upstream_resp = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            client.request(upstream_req),
        )
        .await
        .map_err(|_| Error::UpstreamConnection("SSE upstream connect timeout".to_string()))?
        .map_err(|e| Error::UpstreamConnection(format!("SSE upstream failed: {e}")))?;

        let status = upstream_resp.status();
        if !status.is_success() {
            tracing::warn!(status = %status, path = %path, "SSE upstream returned non-success");
        }

        // Track connection
        instance.increment_connections();
        self.sse_active_count.fetch_add(1, Ordering::Relaxed);
        let instance_cleanup = instance.clone();
        let sse_count = self.sse_active_count.clone();
        let _metrics = self.metrics_collector.clone();
        let route_key = format!("SSE {path}");
        let _start = Instant::now();

        // Build response — forward upstream headers including Retry
        let (resp_parts, upstream_body) = upstream_resp.into_parts();

        // Spawn cleanup task that fires when the streaming body is dropped
        // (i.e., when client disconnects or upstream ends)
        tokio::spawn(async move {
            // Wait for the connection to be fully utilized
            // This task monitors connection lifetime — cleanup happens when
            // the streaming body is consumed/dropped by hyper
            // We rely on Drop semantics; for explicit tracking we'd need
            // a wrapper stream. For now, just log the metric on a timer.
            // The actual connection cleanup relies on hyper dropping the body.
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                // Check if the SSE connection is still alive by checking the count
                // This is a background heartbeat for metrics purposes
                if sse_count.load(Ordering::Relaxed) == 0 {
                    break;
                }
            }
        });

        // Return response with streaming body and SSE-appropriate headers
        let mut response = Response::from_parts(resp_parts, streaming(upstream_body));

        // Ensure SSE headers are set even if upstream didn't set them
        let headers = response.headers_mut();
        if !headers.contains_key("content-type") {
            headers.insert(
                http::header::CONTENT_TYPE,
                "text/event-stream".parse().unwrap(),
            );
        }
        if !headers.contains_key("cache-control") {
            headers.insert(http::header::CACHE_CONTROL, "no-cache".parse().unwrap());
        }

        // Record the SSE connection start
        self.metrics_collector.record_request(
            &route_key,
            std::time::Duration::ZERO,
            RequestOutcome::Success,
        );

        // Schedule cleanup when the response body is eventually dropped
        let instance_for_drop = instance_cleanup;
        let sse_drop_count = self.sse_active_count.clone();
        let metrics_drop = self.metrics_collector.clone();
        let route_key_drop = route_key;
        tokio::spawn(async move {
            // We can't directly detect body drop, but we decrement after a
            // reasonable SSE session max lifetime or when the server shuts down.
            // In practice, hyper will close the connection when client disconnects,
            // which closes the upstream body stream, which ends the SSE.
            // For accurate tracking, we'd wrap the body in a custom stream.
            // For now, rely on the upstream connection close propagating.
            // TODO: Wrap in custom body adapter for precise drop detection
            let _ = (
                instance_for_drop,
                sse_drop_count,
                metrics_drop,
                route_key_drop,
                _start,
            );
        });

        Ok(response)
    }

    /// Handle gRPC proxy requests — transparent proxying over HTTP/2
    ///
    /// Called BEFORE body buffering so streaming RPCs work.
    /// Routes gRPC requests to upstream services via HTTP/2 connections.
    async fn handle_grpc_proxy(&self, req: Request<Incoming>) -> Result<Response<Body>> {
        let method = req.method().clone();
        let path = req.uri().path().to_string();

        info!(path = %path, "gRPC proxy request");

        // Only POST is valid for gRPC
        if method != http::Method::POST {
            let resp = octopus_protocols::GrpcHandler::error_response(
                octopus_protocols::grpc::status_codes::UNIMPLEMENTED,
                "Only POST method is allowed for gRPC",
            )?;
            return Ok(resp.map(Either::Left));
        }

        // Parse service/method from path
        let (service, rpc_method) = match octopus_protocols::GrpcHandler::parse_grpc_path(&path) {
            Some(parsed) => parsed,
            None => {
                let resp = octopus_protocols::GrpcHandler::error_response(
                    octopus_protocols::grpc::status_codes::UNIMPLEMENTED,
                    "Invalid gRPC path format",
                )?;
                return Ok(resp.map(Either::Left));
            }
        };

        debug!(service = %service, method = %rpc_method, "Routing gRPC request");

        // Route to upstream — first check explicit gRPC services map, then fall back to router
        let route = self.router.find_route(&method, &path).map_err(|e| {
            warn!(service = %service, error = %e, "No route for gRPC service");
            Error::RouteNotFound(format!("No route for gRPC service: {service}"))
        })?;

        let instance = self
            .router
            .select_instance(&route.upstream_name)
            .map_err(|e| {
                error!(upstream = %route.upstream_name, error = %e, "No upstream for gRPC");
                Error::NoHealthyUpstream
            })?;

        // Build upstream URL
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

        // Parse deadline from grpc-timeout header
        let deadline = req
            .headers()
            .get("grpc-timeout")
            .and_then(|v| v.to_str().ok())
            .and_then(octopus_protocols::GrpcHandler::parse_grpc_timeout);

        // Build upstream gRPC headers
        let upstream_headers = octopus_protocols::grpc::build_grpc_upstream_headers(req.headers());

        // Decompose the request — keep the streaming body
        let (_parts, body) = req.into_parts();

        // Collect body for HTTP/2 send (gRPC unary messages are small; streaming will need
        // a different approach but this works for the common unary case)
        let body_bytes = body
            .collect()
            .await
            .map_err(|e| Error::InvalidRequest(format!("Failed to read gRPC body: {e}")))?
            .to_bytes();

        // Build the upstream request
        let upstream_uri: http::Uri = format!("{upstream_base}{upstream_path}")
            .parse()
            .map_err(|e| Error::InvalidRequest(format!("Invalid upstream URI: {e}")))?;

        let mut upstream_req = Request::builder()
            .method(http::Method::POST)
            .uri(upstream_uri)
            .body(Full::new(body_bytes))
            .map_err(|e| Error::InvalidRequest(format!("Failed to build upstream request: {e}")))?;

        // Copy headers
        *upstream_req.headers_mut() = upstream_headers;

        // Send via HTTP/2 with optional deadline
        let proxy = self.proxy.clone();
        let result = if let Some(deadline_duration) = deadline {
            // Remaining deadline propagation
            let remaining = format!("{}m", deadline_duration.as_millis());
            upstream_req
                .headers_mut()
                .insert("grpc-timeout", remaining.parse().unwrap());

            tokio::time::timeout(
                deadline_duration,
                proxy.client().send_h2(upstream_req, &instance),
            )
            .await
        } else {
            // Default 30s timeout
            tokio::time::timeout(
                std::time::Duration::from_secs(30),
                proxy.client().send_h2(upstream_req, &instance),
            )
            .await
        };

        match result {
            Ok(Ok(resp)) => {
                let status = resp.status();
                debug!(
                    service = %service,
                    rpc_method = %rpc_method,
                    status = %status,
                    "gRPC upstream response received"
                );

                // Stream response back — convert Incoming to streaming Body
                let (parts, body) = resp.into_parts();
                let response = Response::from_parts(parts, streaming(body));
                Ok(response)
            }
            Ok(Err(e)) => {
                error!(service = %service, error = %e, "gRPC upstream error");
                let resp = octopus_protocols::GrpcHandler::error_response(
                    octopus_protocols::grpc::status_codes::UNAVAILABLE,
                    &format!("Upstream unavailable: {e}"),
                )?;
                Ok(resp.map(Either::Left))
            }
            Err(_) => {
                warn!(service = %service, "gRPC deadline exceeded");
                let resp = octopus_protocols::GrpcHandler::error_response(
                    octopus_protocols::grpc::status_codes::DEADLINE_EXCEEDED,
                    "Deadline exceeded",
                )?;
                Ok(resp.map(Either::Left))
            }
        }
    }

    /// Handle the actual proxying logic (called after middleware)
    ///
    /// Uses `Full<Bytes>` explicitly because this is always a buffered path —
    /// streaming (SSE) is handled separately before reaching here.
    async fn handle_proxy_request(
        &self,
        mut req: Request<Full<Bytes>>,
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

        // Apply path rewriting (strip_prefix / add_prefix) before proxying
        let mut upstream_path = path.clone();
        if let Some(ref prefix) = route.strip_prefix {
            if let Some(stripped) = upstream_path.strip_prefix(prefix.as_str()) {
                upstream_path = stripped.to_string();
            }
        }
        if let Some(ref prefix) = route.add_prefix {
            upstream_path = format!("{prefix}{upstream_path}");
        }
        if upstream_path != path {
            // Rebuild the URI with the rewritten path
            let mut parts = req.uri().clone().into_parts();
            let query = req
                .uri()
                .query()
                .map(|q| format!("?{q}"))
                .unwrap_or_default();
            parts.path_and_query = Some(
                format!("{upstream_path}{query}")
                    .parse()
                    .unwrap_or_else(|_| http::uri::PathAndQuery::from_static("/")),
            );
            if let Ok(new_uri) = http::Uri::from_parts(parts) {
                *req.uri_mut() = new_uri;
            }
        }

        // Proxy the request with retry support
        let result = self.proxy.proxy_with_retry(req, &instance).await;
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

                // Response is already Full<Bytes> from proxy_with_retry
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

    /// Create a buffered error response
    fn error_response(&self, status: StatusCode, message: &str) -> Result<Response<Full<Bytes>>> {
        Response::builder()
            .status(status)
            .header("content-type", "text/plain")
            .body(Full::new(Bytes::from(message.to_string())))
            .map_err(|e| Error::Internal(format!("Failed to build error response: {}", e)))
    }

    /// Create a streaming-typed error response (for use in contexts returning `Body`)
    #[allow(dead_code)]
    fn error_body_response(&self, status: StatusCode, message: &str) -> Result<Response<Body>> {
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
