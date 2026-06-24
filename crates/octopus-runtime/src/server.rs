//! HTTP server implementation

use crate::lifecycle::LifecycleState;
use crate::shutdown::ShutdownSignal;
use crate::worker::{WorkerConfig, WorkerPool};
use crate::RuntimeState;
use octopus_config::{Config, ConfigWatcher};
use octopus_core::{Error, Result};
use octopus_farp::FarpApiHandler;
use octopus_plugin_runtime::PluginManager;
use octopus_protocols::{GrpcHandler, ProtocolHandler};
use octopus_proxy::{HttpClient, HttpProxy, ProxyConfig};
use octopus_router::Router;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

/// How a connection's transport is handled.
#[derive(Clone)]
enum TlsMode {
    /// Plain HTTP.
    Plain,
    /// File-based TLS from static configuration (hot-swappable; reloads on cert change).
    Static(octopus_tls::SwappableTlsAcceptor),
    /// Operator-managed, hot-swappable TLS (Gateway listener Secrets).
    Operator(octopus_tls::SwappableTlsAcceptor),
}

/// Serve a single connection (HTTP/1.1 or HTTP/2 auto-detected), injecting the
/// optional client-certificate CN (mTLS) into request extensions.
async fn serve_io<IO>(
    io: IO,
    handler: crate::RequestHandler,
    client_cn: Option<String>,
    sni: Option<String>,
    peer_addr: SocketAddr,
) where
    IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let service =
        hyper::service::service_fn(move |mut req: http::Request<hyper::body::Incoming>| {
            let handler = handler.clone();
            let cn = client_cn.clone();
            let sni = sni.clone();
            let addr = peer_addr;
            async move {
                req.extensions_mut().insert(octopus_tls::TlsClientCn(cn));
                req.extensions_mut().insert(octopus_tls::TlsSniName(sni));
                req.extensions_mut()
                    .insert(crate::handler::ClientAddr(addr));
                handler.handle(req).await.or_else(|e| {
                    tracing::error!("Request handler error: {}", e);
                    let status = e.to_status_code();
                    http::Response::builder()
                        .status(status)
                        .body(http_body_util::Either::Left(http_body_util::Full::new(
                            bytes::Bytes::from(format!("Error: {e}")),
                        )))
                        .map_err(|e| {
                            tracing::error!("Failed to build error response: {}", e);
                            e
                        })
                })
            }
        });
    let io = hyper_util::rt::TokioIo::new(io);
    if let Err(e) =
        hyper_util::server::conn::auto::Builder::new(hyper_util::rt::TokioExecutor::new())
            .serve_connection_with_upgrades(io, service)
            .await
    {
        tracing::error!("Connection error: {}", e);
    }
}

/// Spawn a background task that reloads the file-based TLS certificate when the
/// cert file's modification time changes, rebuilding the config (preserving mTLS
/// and ALPN) and swapping it into the live acceptor with no downtime.
fn spawn_cert_reload(
    acceptor: octopus_tls::SwappableTlsAcceptor,
    tls_cfg: octopus_tls::TlsConfig,
    interval: Duration,
) {
    tokio::spawn(async move {
        let cert_path = tls_cfg.cert_file.clone();
        let mut last = std::fs::metadata(&cert_path)
            .and_then(|m| m.modified())
            .ok();
        let mut ticker = tokio::time::interval(interval);
        ticker.tick().await; // consume the immediate first tick
        loop {
            ticker.tick().await;
            let current = std::fs::metadata(&cert_path)
                .and_then(|m| m.modified())
                .ok();
            if current != last {
                match octopus_tls::build_server_config(&tls_cfg) {
                    Ok(cfg) => {
                        acceptor.swap(Arc::new(cfg));
                        last = current;
                        tracing::info!(cert = ?cert_path, "Reloaded TLS certificate");
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "TLS certificate reload failed; keeping current");
                    }
                }
            }
        }
    });
}

/// Build a label-resolution [`Convention`](octopus_router::Convention) for a
/// convention auth provider. Only the resolved namespace matters for auth, so
/// port/script/route-rules get inert defaults.
fn convention_for_auth(
    cfg: &octopus_config::types::ConventionAuthProviderConfig,
) -> octopus_router::Convention {
    let roles = cfg
        .layout
        .iter()
        .map(|r| match r.as_str() {
            "service" => octopus_router::LabelRole::Service,
            "namespace" | "tenant" => octopus_router::LabelRole::Namespace,
            _ => octopus_router::LabelRole::Ignore,
        })
        .collect();
    octopus_router::Convention {
        base_suffix: format!(".{}", cfg.base_domain.trim_matches('.')),
        roles,
        default_service: None,
        port: 0,
        script: None,
        backend: octopus_router::BackendStrategy::default(),
        route_rules: Vec::new(),
    }
}

/// Shared, lock-free handle to the operator's virtual gateway index.
type GatewayIndexHandle = std::sync::Arc<arc_swap::ArcSwap<octopus_router::VirtualGatewayIndex>>;

/// HTTP server
pub struct Server {
    config: Config,
    router: Arc<Router>,
    proxy: Arc<HttpProxy>,
    state: Arc<RwLock<RuntimeState>>,
    shutdown: ShutdownSignal,
    worker_pool: Arc<WorkerPool>,
    request_count: Arc<AtomicUsize>,
    farp_handler: Option<Arc<FarpApiHandler>>,
    plugin_manager: Option<Arc<PluginManager>>,
    protocol_handlers: Vec<Arc<dyn ProtocolHandler>>,
    /// Optional config file paths for hot-reload support.
    config_paths: Option<Vec<std::path::PathBuf>>,
    /// Lifecycle state backing the health probes.
    lifecycle: LifecycleState,
    /// Operator-managed (hot-swappable) TLS acceptor, when the gateway listener
    /// terminates TLS from Gateway listener Secrets.
    operator_tls: Option<octopus_tls::SwappableTlsAcceptor>,
    /// Shared virtual gateway index from the operator, handed to the request
    /// handler so it can resolve a request's gateway by host. `None` without k8s.
    gateway_index: Option<GatewayIndexHandle>,
}

impl std::fmt::Debug for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Server")
            .field("config", &self.config)
            .field("request_count", &self.request_count)
            .field("protocol_handlers_count", &self.protocol_handlers.len())
            .finish()
    }
}

impl Server {
    /// Create a new server builder
    pub fn builder() -> ServerBuilder {
        ServerBuilder::new()
    }

    /// Get the current state
    pub async fn state(&self) -> RuntimeState {
        *self.state.read().await
    }

    /// Get listen address
    pub fn listen_addr(&self) -> SocketAddr {
        self.config.gateway.listen
    }

    /// Get router
    pub fn router(&self) -> &Router {
        &self.router
    }

    /// Get request count
    pub fn request_count(&self) -> usize {
        self.request_count.load(Ordering::Relaxed)
    }

    /// Get shutdown signal
    pub fn shutdown_signal(&self) -> ShutdownSignal {
        self.shutdown.clone()
    }

    /// Get the lifecycle state (liveness/readiness/startup).
    pub fn lifecycle(&self) -> LifecycleState {
        self.lifecycle.clone()
    }

    /// Run the server
    pub async fn run(&self) -> Result<()> {
        // Set state to running
        {
            let mut state = self.state.write().await;
            *state = RuntimeState::Running;
        }
        self.lifecycle.mark_running();

        tracing::info!(
            listen = %self.listen_addr(),
            workers = self.worker_pool.worker_count(),
            "Server starting"
        );

        // Create TCP listener
        let listener = tokio::net::TcpListener::bind(self.listen_addr())
            .await
            .map_err(|e| {
                Error::Runtime(format!("Failed to bind to {}: {}", self.listen_addr(), e))
            })?;
        // Listener is bound — startup probe can now pass.
        self.lifecycle.mark_bind_complete();

        // Create TLS acceptor if configured
        let tls_acceptor = if let Some(ref tls_config) = self.config.gateway.tls {
            let tls_cfg = octopus_tls::TlsConfig {
                cert_file: tls_config.cert_file.clone().into(),
                key_file: tls_config.key_file.clone().into(),
                client_ca_file: tls_config.client_ca_file.as_ref().map(|s| s.clone().into()),
                require_client_cert: tls_config.require_client_cert,
                min_tls_version: tls_config.min_tls_version.clone(),
                enable_cert_reload: tls_config.enable_cert_reload,
                reload_interval_secs: tls_config.reload_interval_secs,
            };

            match octopus_tls::build_server_config(&tls_cfg) {
                Ok(server_config) => {
                    let acceptor = octopus_tls::SwappableTlsAcceptor::new(Arc::new(server_config));
                    if tls_config.enable_cert_reload {
                        spawn_cert_reload(
                            acceptor.clone(),
                            tls_cfg.clone(),
                            Duration::from_secs(tls_config.reload_interval_secs),
                        );
                    }
                    tracing::info!(
                        cert = %tls_config.cert_file,
                        tls_version = %tls_config.min_tls_version,
                        reload = tls_config.enable_cert_reload,
                        "HTTPS enabled"
                    );
                    Some(acceptor)
                }
                Err(e) => {
                    return Err(Error::Runtime(format!("Failed to initialize TLS: {e}")));
                }
            }
        } else {
            None
        };

        // How each connection is served: static file-based TLS takes precedence,
        // then operator-managed (hot-swappable) TLS, otherwise plain HTTP.
        let tls_mode = if let Some(acceptor) = tls_acceptor {
            TlsMode::Static(acceptor)
        } else if let Some(swappable) = self.operator_tls.clone() {
            TlsMode::Operator(swappable)
        } else {
            tracing::info!("Server listening on {} (HTTP only)", self.listen_addr());
            TlsMode::Plain
        };

        // Build the pre-auth request middleware (compression, CORS) from config.
        let mut middlewares: Vec<Arc<dyn octopus_core::middleware::Middleware>> =
            crate::chain::build_request_middleware(
                &self.config.gateway.compression,
                self.config.cors.as_ref(),
                &self.config.gateway.security_headers,
            );
        tracing::info!(
            compression = self.config.gateway.compression.enabled,
            cors = self.config.cors.is_some(),
            "Request middleware chain built"
        );

        // Add the route-aware rate limiter when any route declares a `rate_limit`.
        // It reads the per-route `MatchedRouteRateLimit` extension injected by the
        // handler and enforces a fixed window. Uses an in-process state backend;
        // swap for a shared backend (e.g. Redis) for cross-replica limits.
        if self.config.routes.iter().any(|r| r.rate_limit.is_some()) {
            let backend = octopus_state::InMemoryBackend::new();
            middlewares.push(Arc::new(octopus_middleware::RouteRateLimiter::new(backend))
                as Arc<dyn octopus_core::middleware::Middleware>);
            tracing::info!("Per-route rate limiting enabled");
        }

        // Load plugin middleware (script plugins) from `config.plugins`.
        middlewares.extend(crate::chain::build_plugin_middleware(&self.config.plugins));

        // Initialize auth providers from config and add auth middleware
        let mut auth_registry: Option<Arc<octopus_auth::AuthProviderRegistry>> = None;
        if !self.config.auth_providers.is_empty() || self.config.auth.global_enforce {
            let registry = Arc::new(octopus_auth::AuthProviderRegistry::new(
                self.config.auth.default_provider.clone(),
                self.config.auth.token_cache_ttl,
            ));

            // Instantiate each configured provider
            for (name, provider_config) in &self.config.auth_providers {
                match provider_config {
                    octopus_config::types::AuthProviderConfig::Jwt(cfg) => {
                        match octopus_auth::JwtProvider::from_config(name, cfg) {
                            Ok(p) => {
                                registry.register(name, Arc::new(p));
                                tracing::info!(name = %name, "JWT auth provider registered");
                            }
                            Err(e) => {
                                tracing::error!(name = %name, error = %e, "Failed to create JWT provider");
                            }
                        }
                    }
                    octopus_config::types::AuthProviderConfig::Oidc(cfg) => {
                        match octopus_auth::OidcProvider::from_config(name, cfg).await {
                            Ok(p) => {
                                registry.register(name, Arc::new(p));
                                tracing::info!(name = %name, issuer = %cfg.issuer_url, "OIDC auth provider registered");
                            }
                            Err(e) => {
                                tracing::error!(name = %name, error = %e, "Failed to create OIDC provider");
                            }
                        }
                    }
                    octopus_config::types::AuthProviderConfig::ApiKey(cfg) => {
                        let p = octopus_auth::ApiKeyProvider::from_config(name, cfg);
                        registry.register(name, Arc::new(p));
                        tracing::info!(name = %name, keys = cfg.keys.len(), "API key auth provider registered");
                    }
                    octopus_config::types::AuthProviderConfig::ForwardAuth(cfg) => {
                        match octopus_auth::ForwardAuthProvider::from_config(name, cfg) {
                            Ok(p) => {
                                registry.register(name, Arc::new(p));
                                tracing::info!(name = %name, endpoint = %cfg.endpoint, "Forward auth provider registered");
                            }
                            Err(e) => {
                                tracing::error!(name = %name, error = %e, "Failed to create forward auth provider");
                            }
                        }
                    }
                    octopus_config::types::AuthProviderConfig::Mtls(cfg) => {
                        let p = octopus_auth::MtlsProvider::from_config(name, cfg);
                        registry.register(name, Arc::new(p));
                        tracing::info!(name = %name, "mTLS auth provider registered");
                    }
                    octopus_config::types::AuthProviderConfig::Introspection(cfg) => {
                        match octopus_auth::IntrospectionProvider::from_config(name, cfg) {
                            Ok(p) => {
                                registry.register(name, Arc::new(p));
                                tracing::info!(name = %name, endpoint = %cfg.endpoint, "Introspection auth provider registered");
                            }
                            Err(e) => {
                                tracing::error!(name = %name, error = %e, "Failed to create introspection provider");
                            }
                        }
                    }
                    octopus_config::types::AuthProviderConfig::ConventionAuth(cfg) => {
                        let convention = convention_for_auth(cfg);
                        let base = octopus_config::types::IntrospectionProviderConfig {
                            endpoint: String::new(), // substituted per namespace
                            header_name: cfg.header_name.clone(),
                            token_prefix: cfg.token_prefix.clone(),
                            client_id: cfg.client_id.clone(),
                            client_secret: cfg.client_secret.clone(),
                            subject_field: cfg.subject_field.clone(),
                            roles_field: cfg.roles_field.clone(),
                            scope_field: cfg.scope_field.clone(),
                            timeout: cfg.timeout,
                        };
                        match octopus_auth::ConventionAuthProvider::new(
                            name,
                            convention,
                            &cfg.endpoint_template,
                            base,
                        ) {
                            Ok(p) => {
                                registry.register(name, Arc::new(p));
                                tracing::info!(name = %name, base_domain = %cfg.base_domain, "Convention auth provider registered");
                            }
                            Err(e) => {
                                tracing::error!(name = %name, error = %e, "Failed to create convention auth provider");
                            }
                        }
                    }
                }
            }

            // Build authz engine
            let authz = Arc::new(
                octopus_auth::AuthzEvaluator::from_config(&self.config.auth.authz)
                    .unwrap_or_else(|e| {
                        tracing::error!(error = %e, "Failed to create authz evaluator, using default");
                        octopus_auth::AuthzEvaluator::from_config(&octopus_config::types::AuthzConfig::default()).unwrap()
                    }),
            );

            // Spawn cache cleanup task
            let registry_clone = Arc::clone(&registry);
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(120)).await;
                    registry_clone.cleanup_cache();
                }
            });

            // Add auth gateway middleware
            let auth_middleware = Arc::new(octopus_middleware::AuthGatewayMiddleware::new(
                Arc::clone(&registry),
                authz,
                self.config.auth.clone(),
            )) as Arc<dyn octopus_core::middleware::Middleware>;
            middlewares.push(auth_middleware);

            tracing::info!(
                providers = self.config.auth_providers.len(),
                global_enforce = self.config.auth.global_enforce,
                default_provider = ?self.config.auth.default_provider,
                "Auth gateway middleware enabled"
            );

            auth_registry = Some(registry);
        }

        // GraphQL-aware layer runs last (after auth/rate-limit), then delegates
        // to the proxy for valid operations.
        if self.config.graphql.enabled {
            middlewares.push(Arc::new(octopus_graphql::GraphQlMiddleware::from_config(
                &self.config.graphql,
            ))
                as Arc<dyn octopus_core::middleware::Middleware>);
            tracing::info!(
                endpoint = %self.config.graphql.endpoint,
                "GraphQL gateway layer enabled"
            );
        }

        let middleware_chain: Arc<[Arc<dyn octopus_core::middleware::Middleware>]> =
            Arc::from(middlewares);

        let protocol_handlers: Arc<[Arc<dyn ProtocolHandler>]> = Arc::from(
            self.protocol_handlers
                .iter()
                .map(Arc::clone)
                .collect::<Vec<_>>(),
        );

        // Create metrics collector and activity log
        let metrics_collector = Arc::new(octopus_metrics::MetricsCollector::new());
        let activity_log = Arc::new(octopus_metrics::ActivityLog::default());

        // Create health tracker and circuit breaker for monitoring
        let health_tracker = Arc::new(octopus_health::HealthTracker::default_config());
        let circuit_breaker = Arc::new(octopus_health::CircuitBreaker::default_config());

        let mut handler = crate::RequestHandler::with_all_features(
            Arc::clone(&self.router),
            Arc::clone(&self.proxy),
            Arc::clone(&self.request_count),
            middleware_chain,
            self.farp_handler.clone(),
            protocol_handlers,
            Some(health_tracker),
            Some(circuit_breaker),
            self.plugin_manager.clone(),
            metrics_collector,
            activity_log,
            Some(Arc::new(self.config.clone())),
        );

        // Wire the admin IP allowlist (independent of admin auth).
        handler.set_admin_allowed_ips(&self.config.admin.allowed_ips);

        // Wire admin auth if configured
        if let Some(ref registry) = auth_registry {
            handler.set_admin_auth(
                Arc::clone(registry),
                self.config.admin.auth_provider.clone(),
            );
        }

        // Anti host-spoofing (Host == TLS SNI), gated by config.
        handler.set_enforce_sni_check(self.config.gateway.enforce_sni_check);

        // Share the operator's virtual gateway index so the handler can resolve a
        // request's gateway by host (e.g. gateway-level CORS preflight).
        if let Some(ref gateway_index) = self.gateway_index {
            handler.set_gateway_index(Arc::clone(gateway_index));
        }

        // Wire health probes (/livez, /readyz, /startupz).
        {
            let probes_cfg = &self.config.gateway.probes;
            let probe_routes = crate::probes::ProbeRoutes {
                enabled: probes_cfg.enabled,
                liveness: probes_cfg.liveness_path.clone(),
                readiness: probes_cfg.readiness_path.clone(),
                startup: probes_cfg.startup_path.clone(),
            };
            handler.set_lifecycle(self.lifecycle.clone(), probe_routes);
        }

        // EndpointSlice-backed convention upstreams need a live pod watcher.
        #[cfg(feature = "kubernetes")]
        if self.config.kubernetes.enabled {
            match octopus_k8s::EndpointWatchManager::connect(Arc::clone(&self.router)).await {
                Ok(mgr) => {
                    handler.set_backend_watcher(mgr);
                    tracing::info!("EndpointSlice backend watcher enabled for convention routes");
                }
                Err(e) => {
                    tracing::error!(error = %e, "failed to start EndpointSlice backend watcher")
                }
            }
        }

        let mut shutdown_rx = self.shutdown.subscribe();

        // Optionally start the config file watcher for hot-reload.
        let mut config_reload_rx: Option<tokio::sync::mpsc::Receiver<Config>> =
            if let Some(ref paths) = self.config_paths {
                if !paths.is_empty() {
                    let watcher = ConfigWatcher::new(paths.clone(), Duration::from_secs(5));
                    tracing::info!(
                        paths = ?paths,
                        "Config hot-reload enabled (polling every 5s)"
                    );
                    Some(watcher.watch().await)
                } else {
                    None
                }
            } else {
                None
            };

        // A pinned timer for the graceful-drain window. It starts far in the
        // future and is only armed (reset) once shutdown begins; the `draining`
        // guard keeps it inert until then.
        let drain_deadline = tokio::time::sleep(Duration::from_secs(31_536_000));
        tokio::pin!(drain_deadline);
        let mut draining = false;

        loop {
            tokio::select! {
                // Accept new connections
                result = listener.accept() => {
                    match result {
                        Ok((stream, addr)) => {
                            tracing::trace!("Accepted connection from {}", addr);

                            let handler = handler.clone();
                            let tls_mode = tls_mode.clone();

                            // Spawn a task to handle this connection
                            tokio::spawn(async move {
                                match tls_mode {
                                    TlsMode::Plain => serve_io(stream, handler, None, None, addr).await,
                                    TlsMode::Static(acceptor) => match acceptor.accept(stream).await {
                                        Ok(tls_stream) => {
                                            let cn = octopus_tls::extract_client_cn(&tls_stream);
                                            let sni = octopus_tls::extract_server_name(&tls_stream);
                                            serve_io(tls_stream, handler, cn, sni, addr).await;
                                        }
                                        Err(e) => tracing::error!("TLS handshake failed: {}", e),
                                    },
                                    TlsMode::Operator(acceptor) => match acceptor.accept(stream).await {
                                        Ok(tls_stream) => {
                                            let cn = octopus_tls::extract_client_cn(&tls_stream);
                                            let sni = octopus_tls::extract_server_name(&tls_stream);
                                            serve_io(tls_stream, handler, cn, sni, addr).await;
                                        }
                                        Err(e) => tracing::error!("TLS handshake failed: {}", e),
                                    },
                                }
                            });
                        }
                        Err(e) => {
                            tracing::error!("Failed to accept connection: {}", e);
                        }
                    }
                }

                // Handle config hot-reload
                Some(new_config) = async {
                    match config_reload_rx.as_mut() {
                        Some(rx) => rx.recv().await,
                        None => std::future::pending().await,
                    }
                } => {
                    tracing::info!("Applying hot-reloaded configuration");

                    // 1. Clear existing routes and re-register from new config
                    self.router.clear();

                    for route_config in &new_config.routes {
                        for method_str in &route_config.methods {
                            let method: http::Method = match method_str.parse() {
                                Ok(m) => m,
                                Err(_) => {
                                    tracing::error!(method = %method_str, "Invalid HTTP method in reloaded config, skipping");
                                    continue;
                                }
                            };

                            let mut builder = octopus_router::RouteBuilder::new()
                                .path(&route_config.path)
                                .method(method)
                                .upstream_name(&route_config.upstream)
                                .priority(route_config.priority)
                                .auth_provider(route_config.auth_provider.as_deref())
                                .skip_auth(route_config.skip_auth)
                                .require_roles(&route_config.require_roles)
                                .require_scopes(&route_config.require_scopes)
                                .authz_rule(route_config.authz_rule.as_deref());

                            if let Some(ref pfx) = route_config.strip_prefix {
                                builder = builder.strip_prefix(pfx);
                            }
                            if let Some(ref pfx) = route_config.add_prefix {
                                builder = builder.add_prefix(pfx);
                            }
                            if let Some(ref rl) = route_config.rate_limit {
                                builder = builder.rate_limit(rl.requests_per_window, rl.window_size);
                            }
                            if let Some(timeout) = route_config.timeout {
                                builder = builder.timeout(Some(timeout));
                            }
                            if let Some(ref cors_cfg) = route_config.cors {
                                builder = builder.cors(Some(octopus_router::RouteCorsOverride {
                                    allowed_origins: cors_cfg.allowed_origins.clone(),
                                    allowed_methods: cors_cfg.allowed_methods.clone(),
                                    allowed_headers: cors_cfg.allowed_headers.clone(),
                                    allow_credentials: cors_cfg.allow_credentials,
                                    max_age: cors_cfg.max_age,
                                }));
                            }
                            if let Some(spec) = route_config.proxy_spec() {
                                builder = builder.proxy(Some(spec));
                            }

                            match builder.build() {
                                Ok(route) => {
                                    if let Err(e) = self.router.add_route(route) {
                                        tracing::error!(error = %e, "Failed to add route during reload");
                                    }
                                }
                                Err(e) => {
                                    tracing::error!(error = %e, "Failed to build route during reload");
                                }
                            }
                        }
                    }

                    // 2. Re-register upstreams
                    for upstream_config in &new_config.upstreams {
                        let mut cluster = octopus_core::UpstreamCluster::new(&upstream_config.name);
                        for instance_config in &upstream_config.instances {
                            let instance = octopus_core::UpstreamInstance::new(
                                &instance_config.id,
                                &instance_config.host,
                                instance_config.port,
                            );
                            cluster.add_instance(instance);
                        }
                        self.router.register_upstream(cluster);
                    }

                    tracing::info!(
                        routes = new_config.routes.len(),
                        upstreams = new_config.upstreams.len(),
                        "Configuration reloaded successfully"
                    );
                }

                // Handle shutdown signal — begin draining but KEEP accepting for
                // pre_stop_delay so connections that arrive before kube-proxy /
                // the EndpointSlice controller deregister this pod still succeed,
                // and the readiness probe can observe 503 before we halt.
                _ = shutdown_rx.recv(), if !draining => {
                    tracing::info!("Shutdown signal received");
                    // Readiness → NotReady immediately.
                    self.lifecycle.begin_draining();
                    {
                        let mut state = self.state.write().await;
                        *state = RuntimeState::ShuttingDown;
                    }
                    let pre_stop_delay = self.config.gateway.pre_stop_delay;
                    if pre_stop_delay.is_zero() {
                        break;
                    }
                    tracing::info!(
                        delay_secs = pre_stop_delay.as_secs(),
                        "Draining: readiness now NotReady; serving during pre-stop delay"
                    );
                    draining = true;
                    drain_deadline
                        .as_mut()
                        .reset(tokio::time::Instant::now() + pre_stop_delay);
                }

                // Pre-stop delay elapsed: stop accepting and drain in-flight.
                _ = &mut drain_deadline, if draining => {
                    tracing::info!("Pre-stop delay elapsed; halting accept loop");
                    break;
                }
            }
        }

        // Readiness is NotReady, state is ShuttingDown, and we have stopped
        // accepting (the accept loop exited after the pre-stop drain window).
        // Now wait for in-flight requests to drain.
        tracing::info!("Server shutting down gracefully");

        let shutdown_timeout = self.config.gateway.shutdown_timeout;
        let start = std::time::Instant::now();

        tracing::info!(
            timeout_secs = shutdown_timeout.as_secs(),
            "Waiting for in-flight requests to complete"
        );

        // Poll active connections count until zero or timeout
        loop {
            let active = self
                .request_count
                .load(std::sync::atomic::Ordering::Relaxed);

            if active == 0 {
                tracing::info!("All requests completed, shutting down cleanly");
                break;
            }

            if start.elapsed() >= shutdown_timeout {
                tracing::warn!(
                    active_requests = active,
                    "Shutdown timeout reached, forcing shutdown"
                );
                break;
            }

            tracing::debug!(
                active_requests = active,
                elapsed_ms = start.elapsed().as_millis(),
                "Waiting for active requests to complete"
            );

            // Sleep briefly before checking again
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        // Set state to stopped
        {
            let mut state = self.state.write().await;
            *state = RuntimeState::Stopped;
        }
        self.lifecycle.mark_stopped();

        tracing::info!(
            shutdown_duration_ms = start.elapsed().as_millis(),
            "Server stopped"
        );

        Ok(())
    }
}

/// Server builder
#[derive(Debug)]
pub struct ServerBuilder {
    config: Option<Config>,
    worker_config: WorkerConfig,
    enable_farp: bool,
    enable_plugins: bool,
    enable_protocols: bool,
    config_paths: Option<Vec<std::path::PathBuf>>,
}

impl ServerBuilder {
    /// Create a new server builder
    pub fn new() -> Self {
        Self {
            config: None,
            worker_config: WorkerConfig::default(),
            enable_farp: true,
            enable_plugins: true,
            enable_protocols: true,
            config_paths: None,
        }
    }

    /// Set configuration
    pub fn config(mut self, config: Config) -> Self {
        self.config = Some(config);
        self
    }

    /// Set worker configuration
    pub fn worker_config(mut self, config: WorkerConfig) -> Self {
        self.worker_config = config;
        self
    }

    /// Enable/disable FARP
    pub fn enable_farp(mut self, enable: bool) -> Self {
        self.enable_farp = enable;
        self
    }

    /// Enable/disable plugins
    pub fn enable_plugins(mut self, enable: bool) -> Self {
        self.enable_plugins = enable;
        self
    }

    /// Enable/disable protocol handlers
    pub fn enable_protocols(mut self, enable: bool) -> Self {
        self.enable_protocols = enable;
        self
    }

    /// Set config file paths to enable hot-reload support.
    ///
    /// When set, the server will poll these files for changes and
    /// automatically reload routes and upstreams when the configuration
    /// is modified.
    pub fn config_paths(mut self, paths: Vec<std::path::PathBuf>) -> Self {
        self.config_paths = Some(paths);
        self
    }

    /// Build the server
    pub async fn build(self) -> Result<Server> {
        let config = self
            .config
            .ok_or_else(|| Error::Config("config is required".to_string()))?;

        // Create worker pool (only if not in test mode)
        let worker_pool = Arc::new(WorkerPool::new(self.worker_config)?);

        // Lifecycle state backing the health probes. Readiness waits for the
        // first discovery sync only when discovery is configured and the
        // operator asked for it.
        let discovery_required = config.farp.enabled
            && self.enable_farp
            && config.farp.discovery.is_some()
            && config.gateway.probes.require_discovery_sync;
        let lifecycle = LifecycleState::new(discovery_required);

        // Create router
        let router = Arc::new(Router::new());

        // Register upstreams
        for upstream_config in &config.upstreams {
            let mut cluster = octopus_core::UpstreamCluster::new(&upstream_config.name);

            for instance_config in &upstream_config.instances {
                let instance = octopus_core::UpstreamInstance::new(
                    &instance_config.id,
                    &instance_config.host,
                    instance_config.port,
                );
                cluster.add_instance(instance);
            }

            router.register_upstream(cluster);
        }

        // Register routes (with auth config)
        for route_config in &config.routes {
            for method_str in &route_config.methods {
                let method = method_str
                    .parse()
                    .map_err(|_| Error::Config(format!("Invalid HTTP method: {method_str}")))?;

                let mut builder = octopus_router::RouteBuilder::new()
                    .path(&route_config.path)
                    .method(method)
                    .upstream_name(&route_config.upstream)
                    .priority(route_config.priority)
                    .auth_provider(route_config.auth_provider.as_deref())
                    .skip_auth(route_config.skip_auth)
                    .require_roles(&route_config.require_roles)
                    .require_scopes(&route_config.require_scopes)
                    .authz_rule(route_config.authz_rule.as_deref());

                if let Some(ref pfx) = route_config.strip_prefix {
                    builder = builder.strip_prefix(pfx);
                }
                if let Some(ref pfx) = route_config.add_prefix {
                    builder = builder.add_prefix(pfx);
                }
                if let Some(ref rl) = route_config.rate_limit {
                    builder = builder.rate_limit(rl.requests_per_window, rl.window_size);
                }
                if let Some(timeout) = route_config.timeout {
                    builder = builder.timeout(Some(timeout));
                }
                if let Some(ref cors_cfg) = route_config.cors {
                    builder = builder.cors(Some(octopus_router::RouteCorsOverride {
                        allowed_origins: cors_cfg.allowed_origins.clone(),
                        allowed_methods: cors_cfg.allowed_methods.clone(),
                        allowed_headers: cors_cfg.allowed_headers.clone(),
                        allow_credentials: cors_cfg.allow_credentials,
                        max_age: cors_cfg.max_age,
                    }));
                }
                if let Some(spec) = route_config.proxy_spec() {
                    builder = builder.proxy(Some(spec));
                }

                router.add_route(builder.build()?)?;
            }
        }

        // Create HTTP client (connection pool is managed internally)
        let client = HttpClient::with_timeout(config.gateway.request_timeout);

        // Create proxy
        let proxy = Arc::new(HttpProxy::new(client, ProxyConfig::default()));

        // Initialize FARP (if enabled in config AND builder)
        let farp_enabled = config.farp.enabled && self.enable_farp;
        let farp_handler = if farp_enabled {
            tracing::info!("Initializing FARP handler");
            let registry = Arc::new(octopus_farp::SchemaRegistry::with_cache_ttl(
                config.farp.schema_cache_ttl,
            ));
            let federation = Arc::new(octopus_farp::SchemaFederation::new());

            // Optional virtual-gateway binding: scope all FARP routes under one
            // hostname (e.g. api.twinos.cloud) and attach them for policy.
            let farp_binding = config.farp.gateway.as_ref().map(|g| {
                octopus_farp::GatewayBinding::new(&g.hostname)
                    .with_gateway_id(g.gateway_id.clone())
                    .with_default_auth(g.default_auth_provider.clone())
                    .with_rate_limit(
                        g.default_rate_limit_per_minute
                            .map(|rpm| (rpm, std::time::Duration::from_secs(60))),
                    )
                    .with_timeout(g.default_timeout)
            });

            // Build the handler first so its (hot-swappable) binding cell can be
            // shared with the discovery watcher — a CRD-driven binding update via
            // the controller then reaches both the push and discovery paths.
            let mut handler =
                FarpApiHandler::with_federation(Arc::clone(&registry), Arc::clone(&federation))
                    .with_router(Arc::clone(&router));
            if let Some(binding) = farp_binding {
                handler = handler.with_binding(binding);
            }
            let binding_cell = handler.binding_handle();

            // Initialize discovery watcher if discovery is configured.
            if let Some(ref discovery_config) = config.farp.discovery {
                Self::initialize_farp_discovery(
                    registry,
                    federation,
                    Arc::clone(&router),
                    discovery_config,
                    config.farp.watch_interval,
                    lifecycle.discovery_synced_flag(),
                    binding_cell,
                )
                .await;
            }

            Some(Arc::new(handler))
        } else {
            if !config.farp.enabled {
                tracing::info!("FARP disabled in configuration");
            }
            None
        };

        // Initialize Plugin Manager (if enabled)
        let plugin_manager = if self.enable_plugins {
            tracing::info!("Initializing plugin manager");
            // TODO: Load plugins from config.plugins
            Some(Arc::new(PluginManager::new()))
        } else {
            None
        };

        // Initialize Protocol Handlers (if enabled)
        let protocol_handlers: Vec<Arc<dyn ProtocolHandler>> = if self.enable_protocols {
            tracing::info!("Initializing protocol handlers");
            // Note: WebSocket is handled via HTTP upgrade in handler.rs,
            // not through the ProtocolHandler trait (which requires Full<Bytes>)
            vec![Arc::new(GrpcHandler::new())]
        } else {
            Vec::new()
        };

        tracing::info!(
            upstreams = config.upstreams.len(),
            routes = config.routes.len(),
            farp_enabled = farp_handler.is_some(),
            plugins_enabled = plugin_manager.is_some(),
            protocol_handlers = protocol_handlers.len(),
            "Server components initialized"
        );

        // Start the Kubernetes operator (Gateway API + Octopus CRDs) if enabled.
        #[cfg(feature = "kubernetes")]
        let (operator_tls, gateway_index) = if config.kubernetes.enabled {
            Self::initialize_k8s_operator(&config, Arc::clone(&router), farp_handler.clone()).await
        } else {
            (None, None)
        };
        #[cfg(not(feature = "kubernetes"))]
        let (operator_tls, gateway_index): (
            Option<octopus_tls::SwappableTlsAcceptor>,
            Option<GatewayIndexHandle>,
        ) = (None, None);

        // Configuration is fully loaded and applied.
        lifecycle.mark_config_loaded();

        Ok(Server {
            config,
            router,
            proxy,
            state: Arc::new(RwLock::new(RuntimeState::Initializing)),
            shutdown: ShutdownSignal::new(),
            worker_pool,
            request_count: Arc::new(AtomicUsize::new(0)),
            farp_handler,
            plugin_manager,
            protocol_handlers,
            config_paths: self.config_paths,
            lifecycle,
            operator_tls,
            gateway_index,
        })
    }

    /// Initialize FARP discovery providers
    async fn initialize_farp_discovery(
        registry: Arc<octopus_farp::SchemaRegistry>,
        federation: Arc<octopus_farp::SchemaFederation>,
        router: Arc<octopus_router::Router>,
        discovery_config: &octopus_config::types::FarpDiscoveryConfig,
        watch_interval: std::time::Duration,
        discovery_synced: Arc<AtomicBool>,
        binding_cell: octopus_farp::BindingCell,
    ) {
        use octopus_config::types::DiscoveryBackendConfig;

        tracing::info!(
            backends = discovery_config.backends.len(),
            "Initializing FARP discovery watcher"
        );

        let mut watcher = octopus_farp::DiscoveryWatcher::with_federation(
            registry,
            watch_interval,
            3, // max_missed_discoveries
            federation,
        )
        .with_router(router)
        .with_readiness_flag(Arc::clone(&discovery_synced))
        .with_binding_cell(binding_cell);

        let mut enabled_backends = 0;

        for backend in &discovery_config.backends {
            match backend {
                DiscoveryBackendConfig::Mdns { enabled, config } if *enabled => {
                    #[cfg(feature = "mdns")]
                    {
                        use octopus_discovery::mdns::{MdnsConfig, MdnsDiscovery};

                        tracing::info!(
                            service_type = %config.service_type,
                            domain = %config.domain,
                            "Enabling mDNS discovery backend"
                        );

                        let mdns_config = MdnsConfig {
                            service_type: config.service_type.clone(),
                            domain: config.domain.clone(),
                            watch_interval: config.watch_interval,
                            query_timeout: config.query_timeout,
                            enable_ipv6: config.enable_ipv6,
                        };

                        let discovery = MdnsDiscovery::new(mdns_config);
                        watcher.add_provider(Arc::new(discovery));
                        enabled_backends += 1;
                    }

                    #[cfg(not(feature = "mdns"))]
                    {
                        tracing::warn!(
                            "mDNS discovery is configured but the 'mdns' feature is not enabled. \
                             Rebuild with --features mdns to enable mDNS support."
                        );
                    }
                }
                DiscoveryBackendConfig::Dns { enabled, config } if *enabled => {
                    #[cfg(feature = "dns")]
                    {
                        use octopus_discovery::dns::{DnsConfig, DnsDiscovery};

                        tracing::info!(
                            domain = %config.domain,
                            "Enabling DNS discovery backend"
                        );

                        let dns_config = DnsConfig {
                            default_port: 80,
                            watch_interval: config.watch_interval,
                            resolver_config: None,
                        };

                        match DnsDiscovery::new(dns_config).await {
                            Ok(discovery) => {
                                watcher.add_provider(Arc::new(discovery));
                                enabled_backends += 1;
                            }
                            Err(e) => {
                                tracing::error!(error = %e, "Failed to initialize DNS discovery")
                            }
                        }
                    }

                    #[cfg(not(feature = "dns"))]
                    {
                        let _ = config;
                        tracing::warn!("DNS discovery configured but 'dns' feature not enabled");
                    }
                }
                DiscoveryBackendConfig::Consul { enabled, config } if *enabled => {
                    #[cfg(feature = "consul")]
                    {
                        use octopus_discovery::consul::{ConsulConfig, ConsulDiscovery};

                        tracing::info!(
                            address = %config.address,
                            datacenter = %config.datacenter,
                            "Enabling Consul discovery backend"
                        );

                        let consul_config = ConsulConfig {
                            address: config.address.clone(),
                            datacenter: if config.datacenter.is_empty() {
                                None
                            } else {
                                Some(config.datacenter.clone())
                            },
                            token: config.token.clone(),
                            watch_interval: config.watch_interval,
                        };

                        let discovery = ConsulDiscovery::new(consul_config);
                        watcher.add_provider(Arc::new(discovery));
                        enabled_backends += 1;
                    }

                    #[cfg(not(feature = "consul"))]
                    {
                        let _ = config;
                        tracing::warn!(
                            "Consul discovery configured but 'consul' feature not enabled"
                        );
                    }
                }
                DiscoveryBackendConfig::Kubernetes { enabled, config } if *enabled => {
                    #[cfg(feature = "kubernetes")]
                    {
                        use octopus_discovery::kubernetes::{K8sConfig, K8sDiscovery};

                        tracing::info!(
                            namespace = %config.namespace,
                            "Enabling Kubernetes discovery backend"
                        );

                        let k8s_config = K8sConfig {
                            namespace: if config.namespace.is_empty() {
                                None
                            } else {
                                Some(config.namespace.clone())
                            },
                            label_selector: config.label_selector.clone(),
                            use_endpoint_slices: config.use_endpoint_slices,
                            include_not_ready: config.include_not_ready,
                        };

                        match K8sDiscovery::new(k8s_config).await {
                            Ok(discovery) => {
                                watcher.add_provider(Arc::new(discovery));
                                enabled_backends += 1;
                            }
                            Err(e) => {
                                tracing::error!(error = %e, "Failed to initialize Kubernetes discovery")
                            }
                        }
                    }

                    #[cfg(not(feature = "kubernetes"))]
                    {
                        let _ = config;
                        tracing::warn!(
                            "Kubernetes discovery configured but 'kubernetes' feature not enabled"
                        );
                    }
                }
                _ => {
                    // Backend is disabled, skip
                }
            }
        }

        if enabled_backends > 0 {
            tracing::info!(
                enabled_backends,
                "Starting FARP discovery watcher with {} enabled backend(s)",
                enabled_backends
            );

            // Spawn the watcher as a background task
            let watcher = Arc::new(watcher);
            tokio::spawn(async move {
                if let Err(e) = watcher.watch().await {
                    tracing::error!(error = %e, "FARP discovery watcher terminated with error");
                }
            });
        } else {
            tracing::warn!("No discovery backends enabled, FARP discovery watcher will not start");
            // No watcher will run to flip the readiness flag, so mark discovery
            // as synced now — otherwise readiness would be blocked forever.
            discovery_synced.store(true, Ordering::Release);
        }
    }

    /// Convert static config routes/upstreams into the operator's intermediate
    /// representation so they survive merges once the operator owns the router.
    #[cfg(feature = "kubernetes")]
    fn config_to_ir(
        config: &Config,
    ) -> (
        Vec<octopus_k8s::ir::IntermediateRoute>,
        Vec<octopus_core::UpstreamCluster>,
    ) {
        use octopus_k8s::ir::{IntermediateRoute, RateLimit, RouteSource};

        let mut routes = Vec::new();
        for rc in &config.routes {
            for method_str in &rc.methods {
                let Ok(method) = method_str.parse::<http::Method>() else {
                    continue;
                };
                let mut r =
                    IntermediateRoute::new(method, &rc.path, &rc.upstream, RouteSource::Static);
                r.priority = rc.priority;
                r.strip_prefix = rc.strip_prefix.clone();
                r.add_prefix = rc.add_prefix.clone();
                r.auth_provider = rc.auth_provider.clone();
                r.skip_auth = rc.skip_auth;
                r.require_roles = rc.require_roles.clone();
                r.require_scopes = rc.require_scopes.clone();
                r.authz_rule = rc.authz_rule.clone();
                r.timeout = rc.timeout;
                if let Some(rl) = &rc.rate_limit {
                    r.rate_limit = Some(RateLimit {
                        requests: rl.requests_per_window,
                        window: rl.window_size,
                    });
                }
                routes.push(r);
            }
        }

        let mut upstreams = Vec::new();
        for uc in &config.upstreams {
            let mut cluster = octopus_core::UpstreamCluster::new(&uc.name);
            for ic in &uc.instances {
                cluster.add_instance(octopus_core::UpstreamInstance::new(
                    &ic.id, &ic.host, ic.port,
                ));
            }
            upstreams.push(cluster);
        }

        (routes, upstreams)
    }

    /// Initialize and spawn the in-process Kubernetes operator.
    #[cfg(feature = "kubernetes")]
    async fn initialize_k8s_operator(
        config: &Config,
        router: Arc<octopus_router::Router>,
        farp_handler: Option<Arc<FarpApiHandler>>,
    ) -> (
        Option<octopus_tls::SwappableTlsAcceptor>,
        Option<GatewayIndexHandle>,
    ) {
        use octopus_k8s::controller::RouteReconciler;

        tracing::info!(
            gateway_class = %config.kubernetes.gateway_class,
            "Starting Kubernetes operator (Gateway API + Octopus CRDs)"
        );

        // The kube client (and any operator TLS) uses rustls; make sure a
        // CryptoProvider is installed even when static TLS isn't configured.
        octopus_tls::ensure_crypto_provider();

        let reconciler = Arc::new(RouteReconciler::new(router));
        // When this instance is a dedicated child, serve only its gateway's routes.
        if let Some(gateway) = config.kubernetes.serve_only_gateway.clone() {
            reconciler.set_serve_only_gateway(gateway);
        }
        // Let `OctopusGateway{farp_binding: true}` drive the FARP federation binding.
        if let Some(farp) = farp_handler {
            reconciler.set_farp_handler(farp);
        }
        // Seed static config routes so they survive merges with reconciled sources.
        let (routes, upstreams) = Self::config_to_ir(config);
        reconciler.seed_static(routes, upstreams);

        // Terminate TLS on the listener from Gateway Secrets only when opted in
        // and no static TLS is configured (static TLS takes precedence).
        let tls_acceptor = if config.kubernetes.terminate_tls && config.gateway.tls.is_none() {
            let initial = Arc::new(octopus_tls::SniCertResolver::new().into_server_config());
            tracing::info!("Operator-managed TLS termination enabled (Gateway listener Secrets)");
            Some(octopus_tls::SwappableTlsAcceptor::new(initial))
        } else {
            None
        };

        // Shared handle to the gateway index for the data-plane handler (grab it
        // before `reconciler` is moved into the operator task).
        let gateway_index = reconciler.gateway_index_handle();

        // A dedicated child never renders further dedicated children (no recursion).
        let dedicated_image = if config.kubernetes.serve_only_gateway.is_some() {
            None
        } else {
            config.kubernetes.dedicated_gateway_image.clone()
        };

        if let Err(e) = octopus_k8s::controller::start(
            reconciler,
            config.kubernetes.watch_namespaces.clone(),
            tls_acceptor.clone(),
            config.kubernetes.leader_election,
            dedicated_image,
        )
        .await
        {
            tracing::error!(error = %e, "Failed to start Kubernetes operator");
        }

        (tls_acceptor, Some(gateway_index))
    }
}

impl Default for ServerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use octopus_config::{ConfigBuilder, GatewayConfig};
    use std::time::Duration;

    fn test_config() -> Config {
        use octopus_config::types::{CompressionConfig, ProbeConfig};
        ConfigBuilder::new()
            .gateway(GatewayConfig {
                listen: "127.0.0.1:8080".parse().unwrap(),
                workers: 4,
                request_timeout: Duration::from_secs(30),
                shutdown_timeout: Duration::from_secs(30),
                pre_stop_delay: Duration::from_secs(5),
                max_body_size: 10 * 1024 * 1024,
                tls: None,
                compression: CompressionConfig::default(),
                internal_route_prefix: Some("__".to_string()),
                probes: ProbeConfig::default(),
                enforce_sni_check: true,
                security_headers: Default::default(),
            })
            .build()
            .unwrap()
    }

    #[tokio::test]
    async fn test_server_builder() {
        let config = test_config();
        let server = ServerBuilder::new().config(config).build().await.unwrap();

        assert_eq!(server.listen_addr(), "127.0.0.1:8080".parse().unwrap());
        assert_eq!(server.request_count(), 0);
    }

    // Note: test_server_state removed due to runtime-in-runtime complications
    // The server state is tested via integration tests

    #[tokio::test]
    async fn test_server_builder_no_config() {
        let result = ServerBuilder::new().build().await;
        assert!(result.is_err());
    }
}
