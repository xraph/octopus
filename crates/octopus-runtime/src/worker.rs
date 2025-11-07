//! Worker thread management

/// Worker pool configuration
#[derive(Debug, Clone)]
pub struct WorkerConfig {
    /// Number of worker threads (0 = auto)
    pub threads: usize,

    /// Thread stack size
    pub stack_size: Option<usize>,

    /// Thread name prefix
    pub thread_name: String,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            threads: 0, // Auto-detect
            stack_size: None,
            thread_name: "octopus-worker".to_string(),
        }
    }
}

/// Worker pool manager
///
/// Note: This struct tracks worker configuration but does not create a separate runtime.
/// The server uses the runtime provided by #[tokio::main] in the CLI entry point.
/// Creating a nested runtime would cause "Cannot drop a runtime in a context where
/// blocking is not allowed" panics on shutdown.
#[derive(Debug)]
pub struct WorkerPool {
    config: WorkerConfig,
}

impl WorkerPool {
    /// Create a new worker pool
    pub fn new(config: WorkerConfig) -> octopus_core::Result<Self> {
        let threads = if config.threads == 0 {
            num_cpus::get()
        } else {
            config.threads
        };

        tracing::info!(
            threads = threads,
            thread_name = %config.thread_name,
            "Worker pool configuration loaded (using existing runtime)"
        );

        Ok(Self { config })
    }

    /// Get worker count
    pub fn worker_count(&self) -> usize {
        if self.config.threads == 0 {
            num_cpus::get()
        } else {
            self.config.threads
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_worker_config_default() {
        let config = WorkerConfig::default();
        assert_eq!(config.threads, 0); // Auto
        assert_eq!(config.thread_name, "octopus-worker");
    }

    #[test]
    fn test_worker_pool_auto_threads() {
        let config = WorkerConfig::default();
        let pool = WorkerPool::new(config).unwrap();

        // Should use number of CPUs
        assert_eq!(pool.worker_count(), num_cpus::get());
    }

    #[test]
    fn test_worker_pool_custom_threads() {
        let config = WorkerConfig {
            threads: 4,
            ..Default::default()
        };
        let pool = WorkerPool::new(config).unwrap();

        assert_eq!(pool.worker_count(), 4);
    }
}
