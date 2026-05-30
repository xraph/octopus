//! Graceful shutdown with connection draining

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, Notify};
use tracing::{debug, info, warn};

/// Shutdown configuration
#[derive(Debug, Clone)]
pub struct ShutdownConfig {
    /// Grace period to wait for connections to drain (default: 30s)
    pub grace_period: Duration,

    /// Maximum time to wait for graceful shutdown before forcing (default: 60s)
    pub force_timeout: Duration,

    /// Whether to reject new connections during shutdown (default: true)
    pub reject_new_connections: bool,
}

impl Default for ShutdownConfig {
    fn default() -> Self {
        Self {
            grace_period: Duration::from_secs(30),
            force_timeout: Duration::from_secs(60),
            reject_new_connections: true,
        }
    }
}

/// Shutdown handle for coordinating graceful shutdown
#[derive(Clone)]
pub struct ShutdownHandle {
    /// Shutdown initiated flag
    shutdown_initiated: Arc<AtomicBool>,

    /// Active connections counter
    active_connections: Arc<AtomicUsize>,

    /// Broadcast channel for shutdown signal
    shutdown_tx: broadcast::Sender<()>,

    /// Notify when all connections are drained
    drained: Arc<Notify>,

    /// Configuration
    config: ShutdownConfig,
}

impl ShutdownHandle {
    /// Create a new shutdown handle
    pub fn new(config: ShutdownConfig) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);

        Self {
            shutdown_initiated: Arc::new(AtomicBool::new(false)),
            active_connections: Arc::new(AtomicUsize::new(0)),
            shutdown_tx,
            drained: Arc::new(Notify::new()),
            config,
        }
    }

    /// Create with default configuration
    pub fn default_config() -> Self {
        Self::new(ShutdownConfig::default())
    }

    /// Check if shutdown has been initiated
    pub fn is_shutting_down(&self) -> bool {
        self.shutdown_initiated.load(Ordering::Relaxed)
    }

    /// Get number of active connections
    pub fn active_connections(&self) -> usize {
        self.active_connections.load(Ordering::Relaxed)
    }

    /// Increment active connections counter
    pub fn track_connection(&self) -> ConnectionGuard {
        self.active_connections.fetch_add(1, Ordering::Relaxed);
        debug!(active = self.active_connections(), "Connection tracked");

        ConnectionGuard {
            active_connections: self.active_connections.clone(),
            drained: self.drained.clone(),
        }
    }

    /// Create a shutdown signal receiver
    pub fn subscribe(&self) -> ShutdownSignal {
        ShutdownSignal {
            rx: self.shutdown_tx.subscribe(),
        }
    }

    /// Initiate graceful shutdown
    pub async fn shutdown(&self) -> ShutdownResult {
        if self
            .shutdown_initiated
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::Relaxed)
            .is_err()
        {
            warn!("Shutdown already initiated");
            return ShutdownResult::AlreadyShuttingDown;
        }

        info!(
            active_connections = self.active_connections(),
            grace_period = ?self.config.grace_period,
            "Initiating graceful shutdown"
        );

        // Broadcast shutdown signal
        let _ = self.shutdown_tx.send(());

        // Wait for connections to drain with timeout
        let start = std::time::Instant::now();
        let mut drained = false;

        while start.elapsed() < self.config.grace_period {
            if self.active_connections() == 0 {
                drained = true;
                break;
            }

            // Wait for notification or timeout
            tokio::select! {
                _ = self.drained.notified() => {
                    if self.active_connections() == 0 {
                        drained = true;
                        break;
                    }
                }
                _ = tokio::time::sleep(Duration::from_millis(100)) => {}
            }
        }

        if drained {
            info!(
                elapsed = ?start.elapsed(),
                "Graceful shutdown completed - all connections drained"
            );
            ShutdownResult::GracefullyCompleted
        } else {
            let remaining = self.active_connections();
            warn!(
                remaining_connections = remaining,
                elapsed = ?start.elapsed(),
                "Grace period expired with active connections"
            );

            // Wait additional time before forcing
            let force_start = std::time::Instant::now();
            let remaining_force_time = self.config.force_timeout.saturating_sub(start.elapsed());

            while force_start.elapsed() < remaining_force_time {
                if self.active_connections() == 0 {
                    info!("All connections drained before force timeout");
                    return ShutdownResult::CompletedAfterGracePeriod;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }

            let final_remaining = self.active_connections();
            if final_remaining > 0 {
                warn!(
                    remaining_connections = final_remaining,
                    "Forcing shutdown with active connections"
                );
                ShutdownResult::ForcedWithActiveConnections(final_remaining)
            } else {
                ShutdownResult::CompletedAfterGracePeriod
            }
        }
    }

    /// Get configuration
    pub fn config(&self) -> &ShutdownConfig {
        &self.config
    }
}

impl std::fmt::Debug for ShutdownHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShutdownHandle")
            .field("is_shutting_down", &self.is_shutting_down())
            .field("active_connections", &self.active_connections())
            .field("config", &self.config)
            .finish()
    }
}

/// Connection guard that automatically decrements counter on drop
#[derive(Debug)]
pub struct ConnectionGuard {
    active_connections: Arc<AtomicUsize>,
    drained: Arc<Notify>,
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        let prev = self.active_connections.fetch_sub(1, Ordering::Relaxed);
        debug!(active = prev - 1, "Connection guard dropped");

        // Notify if this was the last connection
        if prev == 1 {
            self.drained.notify_waiters();
        }
    }
}

/// Shutdown signal receiver
pub struct ShutdownSignal {
    rx: broadcast::Receiver<()>,
}

impl ShutdownSignal {
    /// Wait for shutdown signal
    pub async fn recv(&mut self) {
        let _ = self.rx.recv().await;
    }

    /// Check if shutdown signal has been received (non-blocking)
    pub fn try_recv(&mut self) -> bool {
        matches!(self.rx.try_recv(), Ok(()))
    }
}

impl std::fmt::Debug for ShutdownSignal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShutdownSignal").finish()
    }
}

/// Result of shutdown operation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShutdownResult {
    /// Shutdown completed gracefully within grace period
    GracefullyCompleted,

    /// Shutdown completed after grace period but before force timeout
    CompletedAfterGracePeriod,

    /// Shutdown forced with N active connections
    ForcedWithActiveConnections(usize),

    /// Shutdown was already in progress
    AlreadyShuttingDown,
}

impl ShutdownResult {
    /// Check if shutdown was successful
    pub fn is_success(&self) -> bool {
        matches!(
            self,
            ShutdownResult::GracefullyCompleted | ShutdownResult::CompletedAfterGracePeriod
        )
    }

    /// Check if connections were forced closed
    pub fn was_forced(&self) -> bool {
        matches!(self, ShutdownResult::ForcedWithActiveConnections(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_shutdown_no_connections() {
        let handle = ShutdownHandle::default_config();

        let result = handle.shutdown().await;

        assert_eq!(result, ShutdownResult::GracefullyCompleted);
        assert!(result.is_success());
        assert!(!result.was_forced());
    }

    #[tokio::test]
    async fn test_shutdown_with_connections() {
        let config = ShutdownConfig {
            grace_period: Duration::from_millis(100),
            force_timeout: Duration::from_secs(1),
            reject_new_connections: true,
        };
        let handle = ShutdownHandle::new(config);

        // Track some connections
        let _guard1 = handle.track_connection();
        let _guard2 = handle.track_connection();

        assert_eq!(handle.active_connections(), 2);

        // Spawn shutdown in background
        let handle_clone = handle.clone();
        let shutdown_task = tokio::spawn(async move { handle_clone.shutdown().await });

        // Wait a bit then drop guards
        tokio::time::sleep(Duration::from_millis(50)).await;
        drop(_guard1);
        drop(_guard2);

        let result = shutdown_task.await.unwrap();
        assert_eq!(result, ShutdownResult::GracefullyCompleted);
    }

    #[tokio::test]
    async fn test_shutdown_signal() {
        let handle = ShutdownHandle::default_config();
        let mut signal = handle.subscribe();

        // Spawn task that waits for signal
        let signal_task = tokio::spawn(async move {
            signal.recv().await;
            true
        });

        // Initiate shutdown
        let _ = handle.shutdown().await;

        // Task should complete
        let received = signal_task.await.unwrap();
        assert!(received);
    }

    #[tokio::test]
    async fn test_connection_guard() {
        let handle = ShutdownHandle::default_config();

        assert_eq!(handle.active_connections(), 0);

        {
            let _guard = handle.track_connection();
            assert_eq!(handle.active_connections(), 1);
        }

        assert_eq!(handle.active_connections(), 0);
    }

    #[tokio::test]
    async fn test_is_shutting_down() {
        let handle = ShutdownHandle::default_config();

        assert!(!handle.is_shutting_down());

        let _ = handle.shutdown().await;

        assert!(handle.is_shutting_down());
    }

    #[tokio::test]
    async fn test_double_shutdown() {
        let handle = ShutdownHandle::default_config();

        let result1 = handle.shutdown().await;
        assert_eq!(result1, ShutdownResult::GracefullyCompleted);

        let result2 = handle.shutdown().await;
        assert_eq!(result2, ShutdownResult::AlreadyShuttingDown);
    }

    #[tokio::test]
    async fn test_forced_shutdown() {
        let config = ShutdownConfig {
            grace_period: Duration::from_millis(50),
            force_timeout: Duration::from_millis(100),
            reject_new_connections: true,
        };
        let handle = ShutdownHandle::new(config);

        // Keep connections alive
        let _guard1 = handle.track_connection();
        let _guard2 = handle.track_connection();

        let result = handle.shutdown().await;

        assert!(result.was_forced());
        if let ShutdownResult::ForcedWithActiveConnections(count) = result {
            assert_eq!(count, 2);
        } else {
            panic!("Expected ForcedWithActiveConnections");
        }
    }
}
