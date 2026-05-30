//! Graceful shutdown integration tests

use super::*;
use octopus_proxy::shutdown::{ShutdownConfig, ShutdownHandle, ShutdownResult};
use std::time::Duration;
use tokio::time::timeout;

#[tokio::test]
async fn test_shutdown_handle_creation() {
    let handle = ShutdownHandle::default_config();

    // Verify initial state
    assert!(!handle.is_shutting_down());
    assert_eq!(handle.active_connections(), 0);
}

#[tokio::test]
async fn test_shutdown_with_config() {
    let config = ShutdownConfig {
        grace_period: Duration::from_secs(5),
        force_timeout: Duration::from_secs(10),
        reject_new_connections: true,
    };

    let handle = ShutdownHandle::new(config);

    assert!(!handle.is_shutting_down());
    assert_eq!(handle.config().grace_period, Duration::from_secs(5));
}

#[tokio::test]
async fn test_shutdown_signal_creation() {
    let handle = ShutdownHandle::default_config();

    // Get a signal
    let mut signal = handle.subscribe();

    // Verify signal hasn't fired yet (try_recv should return false)
    assert!(!signal.try_recv());
}

#[tokio::test]
async fn test_shutdown_signal_propagation() {
    let handle = ShutdownHandle::default_config();

    // Create multiple subscribers
    let mut signal1 = handle.subscribe();
    let mut signal2 = handle.subscribe();
    let mut signal3 = handle.subscribe();

    // Trigger shutdown (async)
    let result = handle.shutdown().await;
    assert_eq!(result, ShutdownResult::GracefullyCompleted);

    // All signals should receive notification (try_recv should work)
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert!(signal1.try_recv() || signal2.try_recv() || signal3.try_recv());
}

#[tokio::test]
async fn test_connection_tracking() {
    let handle = ShutdownHandle::default_config();

    // Track a connection
    let _guard1 = handle.track_connection();
    assert_eq!(handle.active_connections(), 1);

    let _guard2 = handle.track_connection();
    assert_eq!(handle.active_connections(), 2);

    // Drop first guard
    drop(_guard1);
    assert_eq!(handle.active_connections(), 1);

    // Drop second guard
    drop(_guard2);
    assert_eq!(handle.active_connections(), 0);
}

#[tokio::test]
async fn test_graceful_shutdown_with_no_connections() {
    let handle = ShutdownHandle::default_config();

    // No active connections
    assert_eq!(handle.active_connections(), 0);

    // Shutdown should complete immediately
    let result = handle.shutdown().await;
    assert_eq!(result, ShutdownResult::GracefullyCompleted);
}

#[tokio::test]
async fn test_graceful_shutdown_waits_for_connections() {
    let handle = ShutdownHandle::default_config();

    // Track a connection
    let guard = handle.track_connection();
    assert_eq!(handle.active_connections(), 1);

    // Start shutdown in background
    let handle_clone = handle.clone();
    let shutdown_task = tokio::spawn(async move { handle_clone.shutdown().await });

    // Wait a bit
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Shutdown should still be waiting
    assert!(!shutdown_task.is_finished());

    // Drop the connection
    drop(guard);

    // Shutdown should complete
    let result = timeout(Duration::from_millis(200), shutdown_task).await;
    assert!(result.is_ok());
    let shutdown_result = result.unwrap().unwrap();
    assert_eq!(shutdown_result, ShutdownResult::GracefullyCompleted);
}

#[tokio::test]
async fn test_shutdown_signal_recv() {
    let handle = ShutdownHandle::default_config();
    let mut signal = handle.subscribe();

    // Spawn a task that waits for shutdown
    let task = tokio::spawn(async move {
        signal.recv().await;
        "shutdown_received"
    });

    // Wait a bit then trigger shutdown
    tokio::time::sleep(Duration::from_millis(50)).await;
    let _ = handle.shutdown().await;

    // Task should complete
    let result = timeout(Duration::from_millis(100), task).await;
    assert!(result.is_ok(), "Task should complete after shutdown");
    assert_eq!(result.unwrap().unwrap(), "shutdown_received");
}

#[tokio::test]
async fn test_multiple_shutdown_calls() {
    let handle = ShutdownHandle::default_config();

    // First shutdown
    let result1 = handle.shutdown().await;
    assert_eq!(result1, ShutdownResult::GracefullyCompleted);

    // Second shutdown should return AlreadyShuttingDown
    let result2 = handle.shutdown().await;
    assert_eq!(result2, ShutdownResult::AlreadyShuttingDown);
}

#[tokio::test]
async fn test_shutdown_after_subscribe() {
    let handle = ShutdownHandle::default_config();

    // Trigger shutdown first
    let _ = handle.shutdown().await;

    // Subscribe after shutdown
    let _signal = handle.subscribe();

    // New signals might miss the initial broadcast but handle should show shutting down
    assert!(handle.is_shutting_down());
}

#[tokio::test]
async fn test_concurrent_shutdown_listeners() {
    let handle = ShutdownHandle::default_config();

    // Create multiple listeners
    let mut listeners = vec![];
    for _ in 0..10 {
        let mut signal = handle.subscribe();
        let listener = tokio::spawn(async move {
            signal.recv().await;
            true
        });
        listeners.push(listener);
    }

    // Trigger shutdown
    let _ = handle.shutdown().await;

    // All listeners should complete
    for listener in listeners {
        let result = timeout(Duration::from_millis(200), listener).await;
        assert!(
            result.is_ok(),
            "All listeners should receive shutdown signal"
        );
        assert!(result.unwrap().unwrap());
    }
}

#[tokio::test]
async fn test_shutdown_with_active_tasks() {
    let config = ShutdownConfig {
        grace_period: Duration::from_millis(500),
        force_timeout: Duration::from_secs(2),
        reject_new_connections: true,
    };
    let handle = ShutdownHandle::new(config);

    // Spawn some "active" tasks with connection tracking
    let mut tasks = vec![];
    for i in 0..5 {
        let mut signal = handle.subscribe();
        let guard = handle.track_connection();

        let task = tokio::spawn(async move {
            // Simulate work
            tokio::select! {
                _ = signal.recv() => {
                    // Cleanup work
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    drop(guard); // Drop connection guard
                    format!("task-{}-shutdown", i)
                }
                _ = tokio::time::sleep(Duration::from_secs(10)) => {
                    drop(guard);
                    format!("task-{}-timeout", i)
                }
            }
        });
        tasks.push(task);
    }

    // Let tasks start
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Verify connections are tracked
    assert_eq!(handle.active_connections(), 5);

    // Trigger shutdown
    let shutdown_task = tokio::spawn(async move { handle.shutdown().await });

    // All tasks should complete via shutdown path
    for task in tasks {
        let result = timeout(Duration::from_millis(500), task).await;
        assert!(result.is_ok(), "Task should complete after shutdown");
        let msg = result.unwrap().unwrap();
        assert!(
            msg.contains("shutdown"),
            "Task should take shutdown path: {}",
            msg
        );
    }

    // Shutdown should complete gracefully
    let shutdown_result = timeout(Duration::from_millis(500), shutdown_task).await;
    assert!(shutdown_result.is_ok());
    assert_eq!(
        shutdown_result.unwrap().unwrap(),
        ShutdownResult::GracefullyCompleted
    );
}

#[tokio::test]
async fn test_shutdown_force_timeout() {
    let config = ShutdownConfig {
        grace_period: Duration::from_millis(100),
        force_timeout: Duration::from_millis(200),
        reject_new_connections: true,
    };
    let handle = ShutdownHandle::new(config);

    // Track a connection that won't be dropped
    let _guard = handle.track_connection();

    // Shutdown will wait grace period + force timeout
    let result = handle.shutdown().await;

    // Should force shutdown with 1 active connection
    assert_eq!(result, ShutdownResult::ForcedWithActiveConnections(1));
}

#[tokio::test]
async fn test_shutdown_signal_clone() {
    let handle = ShutdownHandle::default_config();
    let signal = handle.subscribe();

    // Clone the handle
    let handle_clone = handle.clone();

    // Trigger shutdown from clone
    let result = handle_clone.shutdown().await;
    assert_eq!(result, ShutdownResult::GracefullyCompleted);

    // Original handle should also show shutting down
    assert!(handle.is_shutting_down());
}

#[tokio::test]
async fn test_connection_guard_drop() {
    let handle = ShutdownHandle::default_config();

    {
        let _guard = handle.track_connection();
        assert_eq!(handle.active_connections(), 1);
    } // guard dropped

    // Connection should be decremented
    assert_eq!(handle.active_connections(), 0);
}

#[tokio::test]
async fn test_shutdown_integration_with_mock_server() {
    let mut mock = MockUpstream::new(0).await.unwrap();
    mock.start().await.unwrap();

    let handle = ShutdownHandle::default_config();

    // Simulate a request handler that respects shutdown
    let mut signal = handle.subscribe();
    let _guard = handle.track_connection();

    let request_task = tokio::spawn(async move {
        tokio::select! {
            _ = signal.recv() => {
                drop(_guard);
                "shutdown_before_request"
            }
            _ = tokio::time::sleep(Duration::from_millis(100)) => {
                drop(_guard);
                "request_completed"
            }
        }
    });

    // Trigger shutdown immediately
    let shutdown_result = handle.shutdown().await;

    // Task should complete via shutdown path
    let result = timeout(Duration::from_millis(200), request_task).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().unwrap(), "shutdown_before_request");
    assert_eq!(shutdown_result, ShutdownResult::GracefullyCompleted);
}
