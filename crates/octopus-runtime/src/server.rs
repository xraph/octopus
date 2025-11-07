//! HTTP server implementation

use crate::shutdown::ShutdownSignal;
use crate::worker::{WorkerConfig, WorkerPool};
use crate::RuntimeState;
use octopus_config::Config;
use octopus_core::{Error, Result};
use octopus_farp::FarpApiHandler;
use octopus_plugin_runtime::PluginManager;
use octopus_protocols::{GraphQLHandler, GrpcHandler, ProtocolHandler, WebSocketHandler};
use octopus_proxy::{ConnectionPool, HttpClient, HttpProxy, PoolConfig, ProxyConfig};
use octopus_router::Router;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

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

    /// Run the server
    pub async fn run(&self) -> Result<()> {
        // Set state to running
        {
            let mut state = self.state.write().await;
            *state = RuntimeState::Running;
        }

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

            match octopus_tls::TlsAcceptor::new(&tls_cfg) {
                Ok(acceptor) => {
                    tracing::info!(
                        cert = %tls_config.cert_file,
                        tls_version = %tls_config.min_tls_version,
                        "HTTPS enabled"
                    );
                    Some(acceptor)
                }
                Err(e) => {
                    return Err(Error::Runtime(format!("Failed to initialize TLS: {}", e)));
                }
            }
        } else {
            tracing::info!("Server listening on {} (HTTP only)", self.listen_addr());
            None
        };

        // Build middleware chain
        let mut middlewares: Vec<Arc<dyn octopus_core::middleware::Middleware>> = Vec::new();

        // Add compression middleware if enabled
        if self.config.gateway.compression.enabled {
            let compression_config = octopus_compression::CompressionConfig {
                enabled: self.config.gateway.compression.enabled,
                level: self.config.gateway.compression.level,
                min_size: self.config.gateway.compression.min_size,
                algorithms: self.config.gateway.compression.algorithms.clone(),
            };
            let compression_middleware = Arc::new(octopus_compression::CompressionMiddleware::new(
                compression_config,
            ))
                as Arc<dyn octopus_core::middleware::Middleware>;
            middlewares.push(compression_middleware);
            tracing::info!(
                level = self.config.gateway.compression.level,
                algorithms = ?self.config.gateway.compression.algorithms,
                "Compression middleware enabled"
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

        // Try to get health tracker and circuit breaker from proxy if available
        // For now, pass plugin manager directly
        let handler = crate::RequestHandler::with_all_features(
            Arc::clone(&self.router),
            Arc::clone(&self.proxy),
            Arc::clone(&self.request_count),
            middleware_chain,
            self.farp_handler.clone(),
            protocol_handlers,
            None, // health_tracker - Extract from proxy if available
            None, // circuit_breaker - Extract from proxy if available
            self.plugin_manager.clone(),
            metrics_collector,
            activity_log,
        );

        let mut shutdown_rx = self.shutdown.subscribe();

        loop {
            tokio::select! {
                // Accept new connections
                result = listener.accept() => {
                    match result {
                        Ok((stream, addr)) => {
                            tracing::trace!("Accepted connection from {}", addr);

                            let handler = handler.clone();
                            let tls_acceptor_clone = tls_acceptor.clone();

                            // Spawn a task to handle this connection
                            tokio::spawn(async move {
                                let service = hyper::service::service_fn(move |req| {
                                    let handler = handler.clone();
                                    async move {
                                        handler.handle(req).await
                                            .or_else(|e| {
                                                tracing::error!("Request handler error: {}", e);
                                                let status = e.to_status_code();
                                                http::Response::builder()
                                                    .status(status)
                                                    .body(http_body_util::Full::new(bytes::Bytes::from(
                                                        format!("Error: {}", e)
                                                    )))
                                                    .map_err(|e| {
                                                        tracing::error!("Failed to build error response: {}", e);
                                                        e
                                                    })
                                            })
                                    }
                                });

                                // Handle TLS if configured
                                if let Some(acceptor) = tls_acceptor_clone {
                                    // Perform TLS handshake
                                    match acceptor.accept(stream).await {
                                        Ok(tls_stream) => {
                                            let io = hyper_util::rt::TokioIo::new(tls_stream);
                                            if let Err(e) = hyper::server::conn::http1::Builder::new()
                                                .serve_connection(io, service)
                                                .await
                                            {
                                                tracing::error!("HTTPS connection error: {}", e);
                                            }
                                        }
                                        Err(e) => {
                                            tracing::error!("TLS handshake failed: {}", e);
                                        }
                                    }
                                } else {
                                    // Plain HTTP
                                    let io = hyper_util::rt::TokioIo::new(stream);
                                if let Err(e) = hyper::server::conn::http1::Builder::new()
                                    .serve_connection(io, service)
                                    .await
                                {
                                        tracing::error!("HTTP connection error: {}", e);
                                    }
                                }
                            });
                        }
                        Err(e) => {
                            tracing::error!("Failed to accept connection: {}", e);
                        }
                    }
                }

                // Handle shutdown signal
                _ = shutdown_rx.recv() => {
                    tracing::info!("Shutdown signal received");
                    break;
                }
            }
        }

        // Set state to shutting down
        {
            let mut state = self.state.write().await;
            *state = RuntimeState::ShuttingDown;
        }

        tracing::info!("Server shutting down gracefully");

        // Graceful shutdown implementation:
        // 1. Stop accepting new connections (already done by breaking the loop)
        // 2. Wait for active requests to complete with timeout

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

    /// Build the server
    pub fn build(self) -> Result<Server> {
        let config = self
            .config
            .ok_or_else(|| Error::Config("config is required".to_string()))?;

        // Create worker pool (only if not in test mode)
        let worker_pool = Arc::new(WorkerPool::new(self.worker_config)?);

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

        // Register routes
        for route_config in &config.routes {
            for method_str in &route_config.methods {
                let method = method_str
                    .parse()
                    .map_err(|_| Error::Config(format!("Invalid HTTP method: {}", method_str)))?;

                let route = octopus_router::RouteBuilder::new()
                    .path(&route_config.path)
                    .method(method)
                    .upstream_name(&route_config.upstream)
                    .priority(route_config.priority)
                    .build()?;

                router.add_route(route)?;
            }
        }

        // Create connection pool
        let pool = Arc::new(ConnectionPool::new(PoolConfig::default()));

        // Create HTTP client
        let client = HttpClient::with_timeout(config.gateway.request_timeout);

        // Create proxy
        let proxy = Arc::new(HttpProxy::new(client, pool, ProxyConfig::default()));

        // Initialize FARP (if enabled in config AND builder)
        let farp_enabled = config.farp.enabled && self.enable_farp;
        let farp_handler = if farp_enabled {
            tracing::info!("Initializing FARP handler");
            let registry = Arc::new(octopus_farp::SchemaRegistry::new());
            let federation = Arc::new(octopus_farp::SchemaFederation::new());

            // Initialize discovery watcher if discovery is configured
            if let Some(ref discovery_config) = config.farp.discovery {
                Self::initialize_farp_discovery(
                    Arc::clone(&registry),
                    Arc::clone(&federation),
                    Arc::clone(&router),
                    discovery_config,
                    config.farp.watch_interval,
                );
            }

            Some(Arc::new(FarpApiHandler::with_federation(
                registry, federation,
            )))
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
            vec![
                Arc::new(WebSocketHandler::new()),
                Arc::new(GrpcHandler::new()),
                Arc::new(GraphQLHandler::new("/graphql")),
            ]
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
        })
    }

    /// Initialize FARP discovery providers
    fn initialize_farp_discovery(
        registry: Arc<octopus_farp::SchemaRegistry>,
        federation: Arc<octopus_farp::SchemaFederation>,
        router: Arc<octopus_router::Router>,
        discovery_config: &octopus_config::types::FarpDiscoveryConfig,
        watch_interval: std::time::Duration,
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
        .with_router(router);

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
                DiscoveryBackendConfig::Dns { enabled, .. } if *enabled => {
                    tracing::warn!("DNS discovery backend is not yet fully implemented for FARP");
                }
                DiscoveryBackendConfig::Consul { enabled, .. } if *enabled => {
                    tracing::warn!("Consul discovery backend is not yet implemented");
                }
                DiscoveryBackendConfig::Kubernetes { enabled, .. } if *enabled => {
                    tracing::warn!("Kubernetes discovery backend is not yet implemented");
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
        }
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
        use crate::types::CompressionConfig;
        ConfigBuilder::new()
            .gateway(GatewayConfig {
                listen: "127.0.0.1:8080".parse().unwrap(),
                workers: 4,
                request_timeout: Duration::from_secs(30),
                shutdown_timeout: Duration::from_secs(30),
                max_body_size: 10 * 1024 * 1024,
                tls: None,
                compression: CompressionConfig::default(),
                internal_route_prefix: Some("__".to_string()),
            })
            .build()
            .unwrap()
    }

    #[test]
    fn test_server_builder() {
        let config = test_config();
        let server = ServerBuilder::new().config(config).build().unwrap();

        assert_eq!(server.listen_addr(), "127.0.0.1:8080".parse().unwrap());
        assert_eq!(server.request_count(), 0);
    }

    // Note: test_server_state removed due to runtime-in-runtime complications
    // The server state is tested via integration tests

    #[test]
    fn test_server_builder_no_config() {
        let result = ServerBuilder::new().build();
        assert!(result.is_err());
    }
}
