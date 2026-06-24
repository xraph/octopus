//! Connection pool for managing upstream connections with real HTTP connection pooling

use bytes::Bytes;
use dashmap::DashMap;
use http_body_util::Full;
use hyper::client::conn::http1;
use hyper_util::rt::TokioIo;
use octopus_core::{Error, Result, UpstreamInstance};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, Semaphore};
use tokio::time::timeout;
use tracing::{debug, info, trace, warn};

use crate::tls::TlsConfig;

/// Cached verify-on TLS config. Building the root store is expensive, so we do
/// it once and clone the cheap `Arc`-backed handle per connection.
static TLS_VERIFY: OnceLock<TlsConfig> = OnceLock::new();

/// Cached verify-off (insecure) TLS config for upstreams with `tls_verify=false`.
static TLS_INSECURE: OnceLock<TlsConfig> = OnceLock::new();

/// Return a shared [`TlsConfig`], building it once per verify mode.
fn shared_tls_config(verify: bool) -> Result<TlsConfig> {
    let cell = if verify { &TLS_VERIFY } else { &TLS_INSECURE };
    if let Some(cfg) = cell.get() {
        return Ok(cfg.clone());
    }
    let built = if verify {
        TlsConfig::new()?
    } else {
        TlsConfig::insecure()?
    };
    // Another thread may have raced us; either way we end up with a usable handle.
    Ok(cell.get_or_init(|| built).clone())
}

/// Drive a freshly handshaked HTTP/1.1 connection in the background. Shared by
/// the plain and TLS paths so both return the same `SendRequest` type.
async fn spawn_http1_handshake<I>(io: TokioIo<I>) -> Result<http1::SendRequest<Full<Bytes>>>
where
    I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Unpin + 'static,
{
    let (sender, conn) = http1::Builder::new()
        .handshake::<_, Full<Bytes>>(io)
        .await
        .map_err(|e| Error::UpstreamConnection(format!("HTTP handshake failed: {e}")))?;

    tokio::spawn(async move {
        if let Err(e) = conn.await {
            debug!("Connection error: {}", e);
        }
    });

    Ok(sender)
}

/// Plain (non-TLS) HTTP/1.1 handshake over a raw TCP stream.
async fn handshake_plain(stream: TcpStream) -> Result<http1::SendRequest<Full<Bytes>>> {
    spawn_http1_handshake(TokioIo::new(stream)).await
}

/// TLS-wrapped HTTP/1.1 handshake. Performs the rustls handshake against
/// `domain` first, then the HTTP/1.1 handshake over the encrypted stream.
async fn handshake_tls(
    stream: TcpStream,
    domain: &str,
    verify: bool,
) -> Result<http1::SendRequest<Full<Bytes>>> {
    let tls_config = shared_tls_config(verify)?;
    let tls_stream = tls_config.connect(stream, domain).await?;
    spawn_http1_handshake(TokioIo::new(tls_stream)).await
}

/// Connection pool configuration
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// Maximum idle connections per upstream
    pub max_idle_per_upstream: usize,

    /// Maximum connections per upstream (includes idle + active)
    pub max_per_upstream: usize,

    /// Idle connection timeout
    pub idle_timeout: Duration,

    /// Connection timeout
    pub connect_timeout: Duration,

    /// Maximum connection lifetime (retire connections after this)
    pub max_connection_lifetime: Duration,

    /// Maximum uses per connection before retirement
    pub max_connection_uses: u32,

    /// Enable connection health checks before reuse
    pub enable_health_check: bool,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_idle_per_upstream: 32,
            max_per_upstream: 128,
            idle_timeout: Duration::from_secs(90),
            connect_timeout: Duration::from_secs(5),
            max_connection_lifetime: Duration::from_secs(300), // 5 minutes
            max_connection_uses: 100,
            enable_health_check: true,
        }
    }
}

/// HTTP/1.1 connection wrapper
#[derive(Debug)]
pub struct PooledConnection {
    sender: http1::SendRequest<Full<Bytes>>,
    created_at: Instant,
    last_used: Instant,
    total_uses: u32,
    upstream_key: UpstreamKey,
}

impl PooledConnection {
    /// Create a new pooled connection
    pub fn new(sender: http1::SendRequest<Full<Bytes>>, upstream_key: UpstreamKey) -> Self {
        let now = Instant::now();
        Self {
            sender,
            created_at: now,
            last_used: now,
            total_uses: 0,
            upstream_key,
        }
    }

    /// Check if connection is still healthy
    pub fn is_healthy(&self, config: &PoolConfig) -> bool {
        // Check if connection is idle too long
        if self.last_used.elapsed() > config.idle_timeout {
            return false;
        }

        // Check if connection exceeded max lifetime
        if self.created_at.elapsed() > config.max_connection_lifetime {
            return false;
        }

        // Check if connection exceeded max uses
        if self.total_uses >= config.max_connection_uses {
            return false;
        }

        // Check if underlying connection is ready
        self.sender.is_ready()
    }

    /// Mark connection as used
    pub fn mark_used(&mut self) {
        self.last_used = Instant::now();
        self.total_uses += 1;
    }

    /// Get the sender
    pub fn sender(&mut self) -> &mut http1::SendRequest<Full<Bytes>> {
        &mut self.sender
    }

    /// Get connection age
    pub fn age(&self) -> Duration {
        self.created_at.elapsed()
    }

    /// Get idle time
    pub fn idle_time(&self) -> Duration {
        self.last_used.elapsed()
    }
}

/// Key for identifying unique upstream targets
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct UpstreamKey {
    /// Upstream host address
    pub host: String,
    /// Upstream port number
    pub port: u16,
    /// Whether the connection uses TLS (https). Keeps http and https pools
    /// distinct so a plain connection is never reused for a secure target.
    pub tls: bool,
}

impl UpstreamKey {
    /// Create an upstream key from a service instance
    pub fn from_instance(instance: &UpstreamInstance) -> Self {
        Self {
            host: instance.address.clone(),
            port: instance.port,
            tls: instance.is_tls(),
        }
    }
}

/// Per-upstream connection pool
struct UpstreamPool {
    /// Idle connections ready for reuse
    idle_connections: Arc<Mutex<VecDeque<PooledConnection>>>,

    /// Number of active connections
    active_count: AtomicU32,

    /// Semaphore to limit max connections
    connection_limit: Arc<Semaphore>,

    /// Pool metrics
    metrics: PoolMetrics,

    /// Pool configuration
    #[allow(dead_code)]
    config: PoolConfig,
}

impl UpstreamPool {
    fn new(config: PoolConfig) -> Self {
        Self {
            idle_connections: Arc::new(Mutex::new(VecDeque::new())),
            active_count: AtomicU32::new(0),
            connection_limit: Arc::new(Semaphore::new(config.max_per_upstream)),
            metrics: PoolMetrics::default(),
            config,
        }
    }

    /// Get connection count (idle + active)
    #[allow(dead_code)]
    fn total_connections(&self) -> u32 {
        let idle = self
            .idle_connections
            .try_lock()
            .map(|idle| idle.len() as u32)
            .unwrap_or(0);
        let active = self.active_count.load(Ordering::Relaxed);
        idle + active
    }

    /// Get idle connection count
    #[allow(dead_code)]
    async fn idle_count(&self) -> usize {
        self.idle_connections.lock().await.len()
    }

    /// Get active connection count
    #[allow(dead_code)]
    fn active_count(&self) -> u32 {
        self.active_count.load(Ordering::Relaxed)
    }
}

/// Pool metrics for observability
#[derive(Debug, Default)]
struct PoolMetrics {
    /// Total connections created
    connections_created: AtomicU64,

    /// Total connections reused
    connections_reused: AtomicU64,

    /// Total connections retired
    connections_retired: AtomicU64,

    /// Total connection errors
    connection_errors: AtomicU64,
}

impl PoolMetrics {
    fn record_created(&self) {
        self.connections_created.fetch_add(1, Ordering::Relaxed);
    }

    fn record_reused(&self) {
        self.connections_reused.fetch_add(1, Ordering::Relaxed);
    }

    fn record_retired(&self) {
        self.connections_retired.fetch_add(1, Ordering::Relaxed);
    }

    fn record_error(&self) {
        self.connection_errors.fetch_add(1, Ordering::Relaxed);
    }
}

/// Connection pool for managing upstream connections
pub struct ConnectionPool {
    config: PoolConfig,
    pools: Arc<DashMap<UpstreamKey, Arc<UpstreamPool>>>,
    accepting: Arc<AtomicBool>,
}

impl std::fmt::Debug for ConnectionPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectionPool")
            .field("config", &self.config)
            .field("accepting", &self.accepting)
            .finish()
    }
}

impl ConnectionPool {
    /// Create a new connection pool
    pub fn new(config: PoolConfig) -> Self {
        let pool = Self {
            config,
            pools: Arc::new(DashMap::new()),
            accepting: Arc::new(AtomicBool::new(true)),
        };

        // Start background cleanup task
        pool.start_cleanup_task();

        pool
    }

    /// Get or create a connection for the upstream
    pub async fn get_connection(&self, instance: &UpstreamInstance) -> Result<PooledConnection> {
        if !self.accepting.load(Ordering::Relaxed) {
            return Err(Error::UpstreamConnection(
                "Pool is shutting down".to_string(),
            ));
        }

        let key = UpstreamKey::from_instance(instance);
        let pool = self.get_or_create_pool(&key);

        // Try to acquire a connection slot (respects max_per_upstream limit)
        let _permit = pool
            .connection_limit
            .clone()
            .acquire_owned()
            .await
            .map_err(|e| Error::UpstreamConnection(format!("Failed to acquire permit: {e}")))?;

        // Try to get an idle connection first
        if let Some(mut conn) = self.pop_idle_connection(&pool, &key).await {
            if conn.is_healthy(&self.config) {
                conn.mark_used();
                pool.active_count.fetch_add(1, Ordering::Relaxed);
                pool.metrics.record_reused();

                trace!(
                    upstream = %key.host,
                    port = key.port,
                    age_secs = conn.age().as_secs(),
                    uses = conn.total_uses,
                    "Reusing pooled connection"
                );

                return Ok(conn);
            } else {
                pool.metrics.record_retired();
                debug!(
                    upstream = %key.host,
                    port = key.port,
                    "Connection retired (unhealthy)"
                );
            }
        }

        // No healthy idle connection, create a new one
        let conn = self.create_connection(instance, &key, &pool).await?;
        pool.active_count.fetch_add(1, Ordering::Relaxed);

        Ok(conn)
    }

    /// Return a connection to the pool
    pub async fn return_connection(&self, mut conn: PooledConnection) {
        let key = conn.upstream_key.clone();

        if let Some(pool) = self.pools.get(&key) {
            pool.active_count.fetch_sub(1, Ordering::Relaxed);

            // Check if connection is still healthy and pool has space
            if conn.is_healthy(&self.config) && self.accepting.load(Ordering::Relaxed) {
                let mut idle = pool.idle_connections.lock().await;

                if idle.len() < self.config.max_idle_per_upstream {
                    conn.last_used = Instant::now();
                    idle.push_back(conn);

                    trace!(
                        upstream = %key.host,
                        port = key.port,
                        idle_count = idle.len(),
                        "Returned connection to pool"
                    );
                } else {
                    pool.metrics.record_retired();
                    debug!(
                        upstream = %key.host,
                        port = key.port,
                        "Connection retired (pool full)"
                    );
                }
            } else {
                pool.metrics.record_retired();
                debug!(
                    upstream = %key.host,
                    port = key.port,
                    "Connection retired (unhealthy)"
                );
            }
        }
    }

    /// Create a new connection to the upstream
    async fn create_connection(
        &self,
        instance: &UpstreamInstance,
        key: &UpstreamKey,
        pool: &UpstreamPool,
    ) -> Result<PooledConnection> {
        let addr = format!("{}:{}", instance.address, instance.port);

        debug!(upstream = %addr, "Creating new connection");

        // Connect with timeout
        let stream = timeout(self.config.connect_timeout, TcpStream::connect(&addr))
            .await
            .map_err(|_| Error::UpstreamTimeout)?
            .map_err(|e| {
                pool.metrics.record_error();
                Error::UpstreamConnection(format!("Failed to connect: {e}"))
            })?;

        // Configure TCP stream
        if let Err(e) = stream.set_nodelay(true) {
            warn!("Failed to set TCP_NODELAY: {}", e);
        }

        // Wrap in TLS for https upstreams; otherwise hand the raw stream to the
        // HTTP/1.1 handshake exactly as before. Both helpers return the same
        // `SendRequest` type so the rest of the path stays single-typed.
        let sender = if instance.is_tls() {
            let domain = instance
                .sni
                .clone()
                .unwrap_or_else(|| instance.address.clone());
            handshake_tls(stream, &domain, instance.tls_verify)
                .await
                .map_err(|e| {
                    pool.metrics.record_error();
                    e
                })?
        } else {
            handshake_plain(stream).await.map_err(|e| {
                pool.metrics.record_error();
                e
            })?
        };

        pool.metrics.record_created();

        info!(
            upstream = %addr,
            total_created = pool.metrics.connections_created.load(Ordering::Relaxed),
            "Created new connection"
        );

        Ok(PooledConnection::new(sender, key.clone()))
    }

    /// Pop an idle connection if available
    async fn pop_idle_connection(
        &self,
        pool: &UpstreamPool,
        key: &UpstreamKey,
    ) -> Option<PooledConnection> {
        let mut idle = pool.idle_connections.lock().await;

        // Try to find a healthy connection
        while let Some(conn) = idle.pop_front() {
            if conn.is_healthy(&self.config) {
                return Some(conn);
            } else {
                pool.metrics.record_retired();
                debug!(
                    upstream = %key.host,
                    port = key.port,
                    "Discarded unhealthy idle connection"
                );
            }
        }

        None
    }

    /// Get or create pool for upstream
    fn get_or_create_pool(&self, key: &UpstreamKey) -> Arc<UpstreamPool> {
        self.pools
            .entry(key.clone())
            .or_insert_with(|| Arc::new(UpstreamPool::new(self.config.clone())))
            .clone()
    }

    /// Start background task to clean up stale connections
    fn start_cleanup_task(&self) {
        let pools = self.pools.clone();
        let config = self.config.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));

            loop {
                interval.tick().await;

                for entry in pools.iter() {
                    let key = entry.key();
                    let pool = entry.value();

                    let mut idle = pool.idle_connections.lock().await;
                    let original_len = idle.len();

                    // Remove unhealthy connections
                    idle.retain(|conn| {
                        let healthy = conn.is_healthy(&config);
                        if !healthy {
                            pool.metrics.record_retired();
                        }
                        healthy
                    });

                    let removed = original_len - idle.len();
                    if removed > 0 {
                        debug!(
                            upstream = %key.host,
                            port = key.port,
                            removed = removed,
                            remaining = idle.len(),
                            "Cleaned up stale connections"
                        );
                    }
                }
            }
        });
    }

    /// Get pool statistics
    pub fn get_pool_stats(&self, key: &UpstreamKey) -> Option<PoolStats> {
        self.pools.get(key).map(|pool| {
            let idle = pool
                .idle_connections
                .try_lock()
                .map(|idle| idle.len())
                .unwrap_or(0);

            PoolStats {
                idle_connections: idle,
                active_connections: pool.active_count.load(Ordering::Relaxed) as usize,
                total_created: pool.metrics.connections_created.load(Ordering::Relaxed),
                total_reused: pool.metrics.connections_reused.load(Ordering::Relaxed),
                total_retired: pool.metrics.connections_retired.load(Ordering::Relaxed),
                connection_errors: pool.metrics.connection_errors.load(Ordering::Relaxed),
            }
        })
    }

    /// Get total pool count
    pub fn pool_count(&self) -> usize {
        self.pools.len()
    }

    /// Get configuration
    pub fn config(&self) -> &PoolConfig {
        &self.config
    }

    /// Graceful shutdown - drain all connections
    pub async fn graceful_shutdown(&self, timeout_duration: Duration) {
        info!("Starting connection pool graceful shutdown");

        // Stop accepting new connections
        self.accepting.store(false, Ordering::SeqCst);

        let deadline = Instant::now() + timeout_duration;

        // Wait for active connections to finish
        while Instant::now() < deadline {
            let total_active: u32 = self
                .pools
                .iter()
                .map(|entry| entry.value().active_count.load(Ordering::Relaxed))
                .sum();

            if total_active == 0 {
                info!("All connections drained gracefully");
                break;
            }

            debug!(
                active_connections = total_active,
                "Waiting for connections to drain"
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        // Close all idle connections
        let mut total_closed = 0;
        for entry in self.pools.iter() {
            let mut idle = entry.value().idle_connections.lock().await;
            total_closed += idle.len();
            idle.clear();
        }

        if total_closed > 0 {
            info!(closed_connections = total_closed, "Closed idle connections");
        }

        // Clear all pools
        self.pools.clear();

        info!("Connection pool shutdown complete");
    }
}

impl Default for ConnectionPool {
    fn default() -> Self {
        Self::new(PoolConfig::default())
    }
}

/// Pool statistics
#[derive(Debug, Clone)]
pub struct PoolStats {
    /// Number of idle connections available for reuse
    pub idle_connections: usize,
    /// Number of active connections currently in use
    pub active_connections: usize,
    /// Total connections created over the pool lifetime
    pub total_created: u64,
    /// Total connections reused from the pool
    pub total_reused: u64,
    /// Total connections retired due to age or errors
    pub total_retired: u64,
    /// Total connection errors encountered
    pub connection_errors: u64,
}

// ============================================================================
// HTTP/2 Connection Pool (for gRPC and HTTP/2 upstreams)
// ============================================================================

/// HTTP/2 connection wrapper — sender is Clone-able for multiplexed streams
#[derive(Clone)]
pub struct Http2Sender {
    sender: hyper::client::conn::http2::SendRequest<Full<Bytes>>,
    created_at: Instant,
}

impl std::fmt::Debug for Http2Sender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Http2Sender")
            .field("age_secs", &self.created_at.elapsed().as_secs())
            .finish()
    }
}

/// HTTP/2 connection pool — one multiplexed connection per upstream
pub struct Http2Pool {
    connections: Arc<DashMap<UpstreamKey, Http2Sender>>,
    config: PoolConfig,
}

impl Http2Pool {
    /// Create a new HTTP/2 pool
    pub fn new(config: PoolConfig) -> Self {
        Self {
            connections: Arc::new(DashMap::new()),
            config,
        }
    }

    /// Get or create an HTTP/2 connection for the upstream
    pub async fn get_sender(
        &self,
        instance: &UpstreamInstance,
    ) -> Result<hyper::client::conn::http2::SendRequest<Full<Bytes>>> {
        let key = UpstreamKey::from_instance(instance);

        // Check for existing healthy connection
        if let Some(entry) = self.connections.get(&key) {
            let sender = &entry.value().sender;
            if !sender.is_closed()
                && entry.value().created_at.elapsed() < self.config.max_connection_lifetime
            {
                return Ok(sender.clone());
            }
            // Connection is dead or expired, remove it
            drop(entry);
            self.connections.remove(&key);
        }

        // Create new HTTP/2 connection.
        //
        // NOTE: This path is plain TCP only. HTTP/2 over TLS (h2c excepted)
        // requires ALPN negotiation of the `h2` protocol, which the current
        // `TlsConfig` does not configure. Wiring a TLS handshake here without
        // ALPN would fail at runtime, so TLS h2 upstreams are intentionally not
        // supported yet. The HTTP/1.1 pool above handles external https origins.
        let addr = format!("{}:{}", instance.address, instance.port);
        debug!(upstream = %addr, "Creating new HTTP/2 connection");

        let stream = timeout(self.config.connect_timeout, TcpStream::connect(&addr))
            .await
            .map_err(|_| Error::UpstreamTimeout)?
            .map_err(|e| Error::UpstreamConnection(format!("Failed to connect: {e}")))?;

        if let Err(e) = stream.set_nodelay(true) {
            warn!("Failed to set TCP_NODELAY: {}", e);
        }

        let io = TokioIo::new(stream);
        let (sender, conn) =
            hyper::client::conn::http2::Builder::new(hyper_util::rt::TokioExecutor::new())
                .handshake(io)
                .await
                .map_err(|e| Error::UpstreamConnection(format!("HTTP/2 handshake failed: {e}")))?;

        // Spawn connection driver task
        tokio::spawn(async move {
            if let Err(e) = conn.await {
                debug!("HTTP/2 connection error: {}", e);
            }
        });

        let h2_sender = Http2Sender {
            sender: sender.clone(),
            created_at: Instant::now(),
        };

        self.connections.insert(key, h2_sender);
        info!(upstream = %addr, "Created new HTTP/2 connection");

        Ok(sender)
    }

    /// Number of active HTTP/2 connections
    pub fn connection_count(&self) -> usize {
        self.connections.len()
    }
}

impl Default for Http2Pool {
    fn default() -> Self {
        Self::new(PoolConfig::default())
    }
}

impl std::fmt::Debug for Http2Pool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Http2Pool")
            .field("connections", &self.connections.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_config() {
        let config = PoolConfig::default();
        assert_eq!(config.max_idle_per_upstream, 32);
        assert_eq!(config.max_per_upstream, 128);
        assert_eq!(config.idle_timeout, Duration::from_secs(90));
    }

    #[test]
    fn test_upstream_key() {
        let instance = UpstreamInstance::new("test-1", "localhost", 8080);
        let key = UpstreamKey::from_instance(&instance);

        assert_eq!(key.host, "localhost");
        assert_eq!(key.port, 8080);
    }

    #[test]
    fn upstream_key_distinguishes_tls() {
        let mut plain = UpstreamInstance::new("a", "h", 443);
        let mut secure = UpstreamInstance::new("a", "h", 443);
        secure.set_tls(true, None, true);
        let kp = UpstreamKey::from_instance(&plain);
        let ks = UpstreamKey::from_instance(&secure);
        assert_ne!(kp, ks);
        let _ = &mut plain;
    }

    // Requires network access to a real https origin — run manually with
    // `cargo test -p octopus-proxy --ignored tls_handshake_real_origin`.
    #[tokio::test]
    #[ignore]
    async fn tls_handshake_real_origin() {
        let mut instance = UpstreamInstance::new("example", "example.com", 443);
        instance.set_tls(true, None, true);

        let pool = ConnectionPool::default();
        let conn = pool
            .get_connection(&instance)
            .await
            .expect("TLS connection to example.com should succeed");
        assert!(conn.sender.is_ready() || !conn.sender.is_closed());
    }

    #[tokio::test]
    async fn test_connection_pool_creation() {
        let pool = ConnectionPool::default();
        assert_eq!(pool.pool_count(), 0);
    }

    #[tokio::test]
    async fn test_graceful_shutdown() {
        let pool = ConnectionPool::default();
        pool.graceful_shutdown(Duration::from_secs(5)).await;
        assert!(!pool.accepting.load(Ordering::Relaxed));
    }
}
