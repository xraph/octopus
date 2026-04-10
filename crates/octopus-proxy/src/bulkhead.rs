//! Bulkhead pattern for per-target concurrency isolation
//!
//! Limits the number of concurrent requests to each upstream target,
//! preventing a slow or failing service from consuming all available connections.

use dashmap::DashMap;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::time::timeout;

/// Bulkhead configuration
#[derive(Debug, Clone)]
pub struct BulkheadConfig {
    /// Whether the bulkhead is enabled
    pub enabled: bool,
    /// Maximum concurrent requests per target (default: 64)
    pub max_concurrent: usize,
    /// Maximum queue size when at capacity (default: 0 = no queuing, reject immediately)
    pub max_queue: usize,
    /// Queue wait timeout (default: 5s)
    pub timeout: Duration,
}

impl Default for BulkheadConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_concurrent: 64,
            max_queue: 0,
            timeout: Duration::from_secs(5),
        }
    }
}

/// Errors from the bulkhead
#[derive(Debug, Error)]
pub enum BulkheadError {
    /// The bulkhead is full and no queue space is available
    #[error("bulkhead full for target '{target_id}': max {max_concurrent} concurrent requests")]
    Full {
        /// The target that is at capacity
        target_id: String,
        /// The configured limit
        max_concurrent: usize,
    },
    /// Timed out waiting in the queue
    #[error("bulkhead timeout for target '{target_id}' after {elapsed:?}")]
    Timeout {
        /// The target that timed out
        target_id: String,
        /// How long we waited
        elapsed: Duration,
    },
}

/// RAII permit — releasing this frees the bulkhead slot
pub struct BulkheadPermit {
    _permit: OwnedSemaphorePermit,
}

impl fmt::Debug for BulkheadPermit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BulkheadPermit").finish()
    }
}

/// Per-target concurrency limiter using the bulkhead pattern
#[derive(Clone)]
pub struct Bulkhead {
    config: BulkheadConfig,
    semaphores: Arc<DashMap<String, Arc<Semaphore>>>,
}

impl Bulkhead {
    /// Create a new Bulkhead with default config
    pub fn new() -> Self {
        Self::with_config(BulkheadConfig::default())
    }

    /// Create a new Bulkhead with custom config
    pub fn with_config(config: BulkheadConfig) -> Self {
        Self {
            config,
            semaphores: Arc::new(DashMap::new()),
        }
    }

    /// Acquire a permit for the given target.
    ///
    /// If `max_queue` is 0, this will return `BulkheadError::Full` immediately
    /// when the target is at capacity. Otherwise, it will wait up to `config.timeout`
    /// for a slot to become available.
    pub async fn acquire(&self, target_id: &str) -> std::result::Result<BulkheadPermit, BulkheadError> {
        if !self.config.enabled {
            // When disabled, create an unbounded semaphore so we never block
            let sem = Arc::new(Semaphore::new(Semaphore::MAX_PERMITS));
            let permit = sem.acquire_owned().await.expect("semaphore closed");
            return Ok(BulkheadPermit { _permit: permit });
        }

        let sem = self.get_or_create_semaphore(target_id);

        if self.config.max_queue == 0 {
            // No queuing: try to acquire immediately
            match sem.try_acquire_owned() {
                Ok(permit) => Ok(BulkheadPermit { _permit: permit }),
                Err(_) => Err(BulkheadError::Full {
                    target_id: target_id.to_string(),
                    max_concurrent: self.config.max_concurrent,
                }),
            }
        } else {
            // With queuing: wait up to timeout
            match timeout(self.config.timeout, sem.acquire_owned()).await {
                Ok(Ok(permit)) => Ok(BulkheadPermit { _permit: permit }),
                Ok(Err(_)) => Err(BulkheadError::Full {
                    target_id: target_id.to_string(),
                    max_concurrent: self.config.max_concurrent,
                }),
                Err(_) => Err(BulkheadError::Timeout {
                    target_id: target_id.to_string(),
                    elapsed: self.config.timeout,
                }),
            }
        }
    }

    /// Get or create the semaphore for a target
    fn get_or_create_semaphore(&self, target_id: &str) -> Arc<Semaphore> {
        self.semaphores
            .entry(target_id.to_string())
            .or_insert_with(|| Arc::new(Semaphore::new(self.config.max_concurrent)))
            .value()
            .clone()
    }

    /// Get the number of available permits for a target (for metrics)
    pub fn available_permits(&self, target_id: &str) -> usize {
        self.semaphores
            .get(target_id)
            .map(|sem| sem.available_permits())
            .unwrap_or(self.config.max_concurrent)
    }
}

impl Default for Bulkhead {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Bulkhead {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Bulkhead")
            .field("enabled", &self.config.enabled)
            .field("max_concurrent", &self.config.max_concurrent)
            .field("max_queue", &self.config.max_queue)
            .field("targets", &self.semaphores.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_acquire_within_limit_succeeds() {
        let bulkhead = Bulkhead::with_config(BulkheadConfig {
            max_concurrent: 2,
            ..Default::default()
        });
        let _p1 = bulkhead.acquire("svc-a").await.unwrap();
        let _p2 = bulkhead.acquire("svc-a").await.unwrap();
    }

    #[tokio::test]
    async fn test_acquire_beyond_limit_fails() {
        let bulkhead = Bulkhead::with_config(BulkheadConfig {
            max_concurrent: 1,
            max_queue: 0,
            ..Default::default()
        });
        let _p1 = bulkhead.acquire("svc-a").await.unwrap();
        let result = bulkhead.acquire("svc-a").await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), BulkheadError::Full { .. }));
    }

    #[tokio::test]
    async fn test_permit_release_frees_slot() {
        let bulkhead = Bulkhead::with_config(BulkheadConfig {
            max_concurrent: 1,
            max_queue: 0,
            ..Default::default()
        });
        {
            let _p1 = bulkhead.acquire("svc-a").await.unwrap();
            // _p1 dropped here
        }
        // Slot freed, should succeed
        let _p2 = bulkhead.acquire("svc-a").await.unwrap();
    }

    #[tokio::test]
    async fn test_per_target_isolation() {
        let bulkhead = Bulkhead::with_config(BulkheadConfig {
            max_concurrent: 1,
            max_queue: 0,
            ..Default::default()
        });
        let _p1 = bulkhead.acquire("svc-a").await.unwrap();
        // svc-b has its own limit
        let _p2 = bulkhead.acquire("svc-b").await.unwrap();
    }

    #[tokio::test]
    async fn test_queue_waits_for_release() {
        let bulkhead = Bulkhead::with_config(BulkheadConfig {
            max_concurrent: 1,
            max_queue: 1,
            timeout: Duration::from_secs(5),
            ..Default::default()
        });

        let p1 = bulkhead.acquire("svc-a").await.unwrap();
        let bh = bulkhead.clone();

        let handle = tokio::spawn(async move {
            bh.acquire("svc-a").await
        });

        // Small delay then release
        tokio::time::sleep(Duration::from_millis(50)).await;
        drop(p1);

        let result = handle.await.unwrap();
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_queue_timeout_returns_error() {
        let bulkhead = Bulkhead::with_config(BulkheadConfig {
            max_concurrent: 1,
            max_queue: 1,
            timeout: Duration::from_millis(50),
            ..Default::default()
        });

        let _p1 = bulkhead.acquire("svc-a").await.unwrap();
        let result = bulkhead.acquire("svc-a").await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), BulkheadError::Timeout { .. }));
    }

    #[tokio::test]
    async fn test_disabled_always_succeeds() {
        let bulkhead = Bulkhead::with_config(BulkheadConfig {
            enabled: false,
            max_concurrent: 1,
            ..Default::default()
        });
        // Should succeed even beyond the limit since disabled
        let _p1 = bulkhead.acquire("svc-a").await.unwrap();
        let _p2 = bulkhead.acquire("svc-a").await.unwrap();
        let _p3 = bulkhead.acquire("svc-a").await.unwrap();
    }

    #[tokio::test]
    async fn test_concurrent_acquisitions() {
        let bulkhead = Bulkhead::with_config(BulkheadConfig {
            max_concurrent: 10,
            max_queue: 0,
            ..Default::default()
        });

        let mut handles = Vec::new();
        for _ in 0..10 {
            let bh = bulkhead.clone();
            handles.push(tokio::spawn(async move {
                bh.acquire("svc-a").await
            }));
        }

        let results: Vec<_> = futures::future::join_all(handles).await;
        let successes = results.iter().filter(|r| r.as_ref().unwrap().is_ok()).count();
        assert_eq!(successes, 10);
    }

    #[tokio::test]
    async fn test_different_targets_independent_limits() {
        let bulkhead = Bulkhead::with_config(BulkheadConfig {
            max_concurrent: 2,
            max_queue: 0,
            ..Default::default()
        });

        let _p1 = bulkhead.acquire("svc-a").await.unwrap();
        let _p2 = bulkhead.acquire("svc-a").await.unwrap();
        assert!(bulkhead.acquire("svc-a").await.is_err()); // svc-a full

        // svc-b still has capacity
        let _p3 = bulkhead.acquire("svc-b").await.unwrap();
        let _p4 = bulkhead.acquire("svc-b").await.unwrap();
        assert!(bulkhead.acquire("svc-b").await.is_err()); // svc-b full
    }

    #[test]
    fn test_default_config_values() {
        let config = BulkheadConfig::default();
        assert!(config.enabled);
        assert_eq!(config.max_concurrent, 64);
        assert_eq!(config.max_queue, 0);
        assert_eq!(config.timeout, Duration::from_secs(5));
    }

    #[tokio::test]
    async fn test_available_permits() {
        let bulkhead = Bulkhead::with_config(BulkheadConfig {
            max_concurrent: 3,
            ..Default::default()
        });

        assert_eq!(bulkhead.available_permits("svc-a"), 3);
        let _p1 = bulkhead.acquire("svc-a").await.unwrap();
        assert_eq!(bulkhead.available_permits("svc-a"), 2);
    }
}
