//! Hot reload support for plugins

use crate::error::{PluginRuntimeError, Result};
use crate::manager::PluginManager;
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

/// Hot reload watcher for plugin configuration files
///
/// Watches for changes to plugin configuration files and automatically
/// reloads the affected plugins.
#[derive(Debug)]
pub struct HotReloadWatcher {
    /// Plugin manager
    manager: Arc<PluginManager>,

    /// Configuration directory to watch
    config_dir: PathBuf,

    /// File watcher
    watcher: Option<RecommendedWatcher>,

    /// Event receiver
    rx: Option<mpsc::UnboundedReceiver<notify::Result<Event>>>,

    /// Debounce duration (to avoid multiple reloads)
    debounce_duration: Duration,
}

impl HotReloadWatcher {
    /// Create a new hot reload watcher
    pub fn new(manager: Arc<PluginManager>, config_dir: impl Into<PathBuf>) -> Self {
        Self {
            manager,
            config_dir: config_dir.into(),
            watcher: None,
            rx: None,
            debounce_duration: Duration::from_secs(1),
        }
    }

    /// Set debounce duration
    pub fn with_debounce(mut self, duration: Duration) -> Self {
        self.debounce_duration = duration;
        self
    }

    /// Start watching for configuration changes
    pub async fn start(&mut self) -> Result<()> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.rx = Some(rx);

        let mut watcher = RecommendedWatcher::new(
            move |res| {
                let _ = tx.send(res);
            },
            Config::default().with_poll_interval(Duration::from_secs(2)),
        )
        .map_err(|e| PluginRuntimeError::other(format!("Failed to create file watcher: {}", e)))?;

        watcher
            .watch(&self.config_dir, RecursiveMode::Recursive)
            .map_err(|e| PluginRuntimeError::other(format!("Failed to watch directory: {}", e)))?;

        self.watcher = Some(watcher);

        info!(
            config_dir = %self.config_dir.display(),
            "Hot reload watcher started"
        );

        Ok(())
    }

    /// Stop watching
    pub fn stop(&mut self) {
        self.watcher = None;
        self.rx = None;
        info!("Hot reload watcher stopped");
    }

    /// Run the watcher event loop
    ///
    /// This method will block until the watcher is stopped.
    pub async fn run(&mut self) -> Result<()> {
        let rx = self
            .rx
            .take()
            .ok_or_else(|| PluginRuntimeError::invalid_state("Watcher not started"))?;

        let mut last_reload = std::time::Instant::now();
        let debounce = self.debounce_duration;
        let manager = Arc::clone(&self.manager);
        let config_dir = self.config_dir.clone();

        tokio::spawn(async move {
            let mut rx = rx;
            while let Some(res) = rx.recv().await {
                match res {
                    Ok(event) => {
                        // Debounce: ignore events that happen too quickly
                        if last_reload.elapsed() < debounce {
                            continue;
                        }

                        // Only process config files (json, yaml, toml)
                        let should_process =
                            matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_))
                                && event.paths.iter().any(|path| {
                                    path.extension()
                                        .and_then(|s| s.to_str())
                                        .map(|ext| matches!(ext, "json" | "yaml" | "yml" | "toml"))
                                        .unwrap_or(false)
                                });

                        if should_process {
                            if let Err(e) =
                                Self::handle_event_static(event, &manager, &config_dir).await
                            {
                                error!(error = %e, "Failed to handle file change event");
                            } else {
                                last_reload = std::time::Instant::now();
                            }
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "File watcher error");
                    }
                }
            }
        });

        Ok(())
    }

    /// Check if we should process this event
    #[allow(dead_code)]
    fn should_process_event(&self, event: &Event) -> bool {
        match event.kind {
            EventKind::Modify(_) | EventKind::Create(_) => {
                // Only process config files (json, yaml, toml)
                event.paths.iter().any(|path| {
                    path.extension()
                        .and_then(|s| s.to_str())
                        .map(|ext| matches!(ext, "json" | "yaml" | "yml" | "toml"))
                        .unwrap_or(false)
                })
            }
            _ => false,
        }
    }

    /// Handle a file change event (static method for spawned task)
    async fn handle_event_static(
        event: Event,
        manager: &PluginManager,
        _config_dir: &Path,
    ) -> Result<()> {
        for path in &event.paths {
            debug!(path = %path.display(), "Configuration file changed");

            if let Some(plugin_name) = Self::extract_plugin_name_static(path) {
                info!(plugin = %plugin_name, "Reloading plugin");

                // Read new configuration
                let config = Self::load_config_static(path).await?;

                // Reload plugin
                if let Err(e) = manager.reload(&plugin_name, config).await {
                    error!(
                        plugin = %plugin_name,
                        error = %e,
                        "Failed to reload plugin"
                    );
                } else {
                    info!(plugin = %plugin_name, "Plugin reloaded successfully");
                }
            }
        }

        Ok(())
    }

    /// Extract plugin name from configuration file path
    ///
    /// Assumes config files are named like: `plugin-name.yaml` or `plugin-name.json`
    #[allow(dead_code)]
    fn extract_plugin_name(&self, path: &Path) -> Option<String> {
        Self::extract_plugin_name_static(path)
    }

    fn extract_plugin_name_static(path: &Path) -> Option<String> {
        path.file_stem().and_then(|s| s.to_str()).map(String::from)
    }

    /// Load configuration from file
    #[allow(dead_code)]
    async fn load_config(&self, path: &Path) -> Result<serde_json::Value> {
        Self::load_config_static(path).await
    }

    async fn load_config_static(path: &Path) -> Result<serde_json::Value> {
        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| PluginRuntimeError::other(format!("Failed to read config: {}", e)))?;

        // Parse based on file extension
        let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");

        match extension {
            "json" => serde_json::from_str(&content).map_err(Into::into),
            "yaml" | "yml" => {
                // Convert YAML to JSON value
                let yaml: serde_json::Value = serde_yaml::from_str(&content)
                    .map_err(|e| PluginRuntimeError::config(format!("YAML parse error: {}", e)))?;
                Ok(yaml)
            }
            "toml" => {
                // Convert TOML to JSON value
                let toml: toml::Value = content
                    .parse()
                    .map_err(|e| PluginRuntimeError::config(format!("TOML parse error: {}", e)))?;
                serde_json::to_value(toml).map_err(Into::into)
            }
            _ => Err(PluginRuntimeError::config(format!(
                "Unsupported config format: {}",
                extension
            ))),
        }
    }
}

/// Plugin reload event
#[derive(Debug, Clone)]
pub struct ReloadEvent {
    /// Plugin name
    pub plugin_name: String,

    /// Timestamp of the reload
    pub timestamp: chrono::DateTime<chrono::Utc>,

    /// Whether the reload was successful
    pub success: bool,

    /// Error message if reload failed
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_plugin_name() {
        let watcher = HotReloadWatcher::new(Arc::new(PluginManager::new()), "/tmp/plugins");

        let path = PathBuf::from("/tmp/plugins/my-plugin.yaml");
        assert_eq!(
            watcher.extract_plugin_name(&path),
            Some("my-plugin".to_string())
        );

        let path = PathBuf::from("/tmp/plugins/auth.json");
        assert_eq!(watcher.extract_plugin_name(&path), Some("auth".to_string()));
    }

    #[test]
    fn test_should_process_event() {
        let watcher = HotReloadWatcher::new(Arc::new(PluginManager::new()), "/tmp/plugins");

        let event = Event::new(EventKind::Modify(notify::event::ModifyKind::Data(
            notify::event::DataChange::Content,
        )))
        .add_path(PathBuf::from("/tmp/plugins/test.yaml"));

        assert!(watcher.should_process_event(&event));

        let event = Event::new(EventKind::Modify(notify::event::ModifyKind::Data(
            notify::event::DataChange::Content,
        )))
        .add_path(PathBuf::from("/tmp/plugins/test.txt"));

        assert!(!watcher.should_process_event(&event));
    }
}
