//! Configuration file watcher with polling-based change detection.
//!
//! Monitors configuration file modification times and triggers a reload when
//! any tracked file changes. The watcher validates the new configuration
//! before emitting it on the channel, ensuring consumers always receive a
//! valid [`Config`].

use crate::{load_and_merge, validate_config, Config};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};
use tokio::sync::mpsc;

/// Polls configuration files for changes and sends reloaded configs on a channel.
///
/// # Usage
///
/// ```rust,no_run
/// use octopus_config::watcher::ConfigWatcher;
/// use std::path::PathBuf;
/// use std::time::Duration;
///
/// # async fn example() {
/// let watcher = ConfigWatcher::new(
///     vec![PathBuf::from("config/base.yaml")],
///     Duration::from_secs(5),
/// );
///
/// let mut rx = watcher.watch().await;
///
/// while let Some(new_config) = rx.recv().await {
///     println!("Config reloaded: {:?}", new_config.gateway.listen);
/// }
/// # }
/// ```
#[derive(Debug)]
pub struct ConfigWatcher {
    paths: Vec<PathBuf>,
    poll_interval: Duration,
}

impl ConfigWatcher {
    /// Create a new config watcher.
    ///
    /// * `paths` - Configuration file paths to monitor. They are loaded and
    ///   merged in order (later files override earlier ones).
    /// * `poll_interval` - How often to check for file modifications.
    pub fn new(paths: Vec<PathBuf>, poll_interval: Duration) -> Self {
        Self {
            paths,
            poll_interval,
        }
    }

    /// Start watching for changes.
    ///
    /// Returns a receiver that yields a new [`Config`] every time the files
    /// change and the resulting configuration passes validation. Invalid
    /// configurations are logged and skipped.
    pub async fn watch(self) -> mpsc::Receiver<Config> {
        let (tx, rx) = mpsc::channel::<Config>(4);

        // Snapshot initial modification times.
        let mut last_mtimes = collect_mtimes(&self.paths);

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(self.poll_interval);
            // The first tick completes immediately — skip it so we don't
            // reload on startup.
            interval.tick().await;

            loop {
                interval.tick().await;

                let current_mtimes = collect_mtimes(&self.paths);

                if current_mtimes != last_mtimes {
                    tracing::info!("Config file change detected, reloading");

                    // Always update stored mtimes so we don't re-attempt on
                    // every tick when the file is persistently invalid.
                    last_mtimes = current_mtimes;

                    match load_and_merge(self.paths.clone()) {
                        Ok(config) => match validate_config(&config) {
                            Ok(()) => {
                                if tx.send(config).await.is_err() {
                                    tracing::debug!(
                                        "Config watcher channel closed, stopping"
                                    );
                                    break;
                                }
                                tracing::info!("Config reloaded successfully");
                            }
                            Err(e) => {
                                tracing::error!(
                                    error = %e,
                                    "Reloaded config failed validation, keeping previous config"
                                );
                            }
                        },
                        Err(e) => {
                            tracing::error!(
                                error = %e,
                                "Failed to load config files, keeping previous config"
                            );
                        }
                    }
                }
            }
        });

        rx
    }
}

/// Collect modification times for a set of paths.
///
/// If a file cannot be read (e.g. deleted), its mtime is recorded as
/// `UNIX_EPOCH` so that a subsequent re-creation is detected as a change.
fn collect_mtimes(paths: &[PathBuf]) -> Vec<(PathBuf, SystemTime)> {
    paths
        .iter()
        .map(|p| {
            let mtime = std::fs::metadata(p)
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            (p.clone(), mtime)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::time::{sleep, timeout};

    fn config_yaml(listen: &str) -> String {
        format!(
            r#"
gateway:
  listen: "{listen}"
  workers: 4
  request_timeout: "30s"
  shutdown_timeout: "30s"
  max_body_size: 10485760

upstreams: []
routes: []
plugins: []

observability:
  logging:
    level: "info"
    format: "text"
  metrics:
    enabled: true
    endpoint: "/metrics"
  tracing:
    enabled: false
"#
        )
    }

    /// Write config using `std::fs::write` so the filesystem mtime is
    /// reliably updated (avoids in-process file-handle caching issues).
    fn write_config(path: &std::path::Path, listen: &str) {
        std::fs::write(path, config_yaml(listen)).expect("write config");
    }

    #[tokio::test]
    async fn test_watcher_detects_change() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("config.yaml");
        write_config(&path, "127.0.0.1:9000");

        let watcher = ConfigWatcher::new(
            vec![path.clone()],
            Duration::from_millis(200),
        );
        let mut rx = watcher.watch().await;

        // macOS HFS+/APFS has 1-second mtime granularity so we wait >1s.
        sleep(Duration::from_millis(1200)).await;
        write_config(&path, "127.0.0.1:9001");

        let result = timeout(Duration::from_secs(5), rx.recv()).await;
        assert!(result.is_ok(), "Expected config change notification");

        let config = result.unwrap().expect("channel should not be closed");
        assert_eq!(
            config.gateway.listen,
            "127.0.0.1:9001".parse().unwrap()
        );
    }

    #[tokio::test]
    async fn test_watcher_skips_invalid_config() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("config.yaml");
        write_config(&path, "127.0.0.1:9000");

        let watcher = ConfigWatcher::new(
            vec![path.clone()],
            Duration::from_millis(200),
        );
        let mut rx = watcher.watch().await;

        // Write invalid YAML after waiting for mtime to tick.
        sleep(Duration::from_millis(1200)).await;
        std::fs::write(&path, "invalid_yaml: [broken").expect("write invalid");

        // Should NOT receive a config since it is invalid.
        let result = timeout(Duration::from_millis(800), rx.recv()).await;
        assert!(
            result.is_err(),
            "Should not receive config for invalid file"
        );

        // Fix the config after another mtime tick.
        sleep(Duration::from_millis(1200)).await;
        write_config(&path, "127.0.0.1:9002");

        let result = timeout(Duration::from_secs(5), rx.recv()).await;
        assert!(result.is_ok(), "Expected config change after fix");
        let config = result.unwrap().expect("channel open");
        assert_eq!(
            config.gateway.listen,
            "127.0.0.1:9002".parse().unwrap()
        );
    }

    #[tokio::test]
    async fn test_watcher_no_false_trigger() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("config.yaml");
        write_config(&path, "127.0.0.1:9000");

        let watcher = ConfigWatcher::new(
            vec![path.clone()],
            Duration::from_millis(100),
        );
        let mut rx = watcher.watch().await;

        // Don't modify the file -- should timeout.
        let result = timeout(Duration::from_millis(800), rx.recv()).await;
        assert!(
            result.is_err(),
            "Should not receive config when file is unchanged"
        );
    }
}
