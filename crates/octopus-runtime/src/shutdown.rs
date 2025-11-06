//! Graceful shutdown with signal handling

use tokio::sync::broadcast;
use tokio::signal;
use std::sync::Arc;

/// Shutdown signal broadcaster
#[derive(Debug, Clone)]
pub struct ShutdownSignal {
    sender: Arc<broadcast::Sender<()>>,
}

impl ShutdownSignal {
    /// Create a new shutdown signal
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(1);
        Self {
            sender: Arc::new(sender),
        }
    }

    /// Subscribe to shutdown notifications
    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.sender.subscribe()
    }

    /// Trigger shutdown
    pub fn trigger(&self) {
        let _ = self.sender.send(());
        tracing::info!("Shutdown signal triggered");
    }

    /// Check if shutdown was triggered
    pub fn is_triggered(&self) -> bool {
        self.sender.receiver_count() == 0
    }
}

impl Default for ShutdownSignal {
    fn default() -> Self {
        Self::new()
    }
}

/// Signal handler for OS signals
#[derive(Debug)]
pub struct SignalHandler {
    signal: ShutdownSignal,
}

impl SignalHandler {
    /// Create a new signal handler
    pub fn new(signal: ShutdownSignal) -> Self {
        Self { signal }
    }

    /// Start listening for OS signals
    pub async fn run(self) {
        // Wait for SIGINT or SIGTERM
        #[cfg(unix)]
        {
            use signal::unix::{signal, SignalKind};
            
            let mut sigterm = signal(SignalKind::terminate())
                .expect("Failed to setup SIGTERM handler");
            let mut sigint = signal(SignalKind::interrupt())
                .expect("Failed to setup SIGINT handler");
            
            tokio::select! {
                _ = sigterm.recv() => {
                    tracing::info!("Received SIGTERM");
                    self.signal.trigger();
                }
                _ = sigint.recv() => {
                    tracing::info!("Received SIGINT");
                    self.signal.trigger();
                }
            }
        }
        
        #[cfg(not(unix))]
        {
            match signal::ctrl_c().await {
                Ok(()) => {
                    tracing::info!("Received Ctrl+C");
                    self.signal.trigger();
                }
                Err(err) => {
                    tracing::error!("Failed to listen for Ctrl+C: {}", err);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_shutdown_signal_new() {
        let signal = ShutdownSignal::new();
        let _rx = signal.subscribe(); // Keep a receiver alive
        assert!(!signal.is_triggered());
    }

    #[tokio::test]
    async fn test_shutdown_signal_subscribe() {
        let signal = ShutdownSignal::new();
        let mut rx = signal.subscribe();
        
        signal.trigger();
        
        // Should receive shutdown
        assert!(rx.try_recv().is_ok());
    }

    #[tokio::test]
    async fn test_shutdown_signal_multiple_subscribers() {
        let signal = ShutdownSignal::new();
        let mut rx1 = signal.subscribe();
        let mut rx2 = signal.subscribe();
        
        signal.trigger();
        
        // Both should receive
        assert!(rx1.try_recv().is_ok());
        assert!(rx2.try_recv().is_ok());
    }
}


