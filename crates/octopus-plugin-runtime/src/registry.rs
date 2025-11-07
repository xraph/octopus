//! Plugin registry for managing plugin lifecycle

use crate::error::{PluginRuntimeError, Result};
use dashmap::DashMap;
use octopus_plugin_api::{HealthStatus, Plugin, PluginDependency, PluginInfo, PluginMetadata};
use std::sync::Arc;
use std::time::Instant;
use tracing::{error, info, warn};

/// Plugin registry for managing plugin lifecycle
///
/// The registry maintains all registered plugins and their states,
/// handles dependency resolution, and manages lifecycle transitions.
#[derive(Clone, Debug)]
pub struct PluginRegistry {
    /// Registered plugins
    plugins: Arc<DashMap<String, PluginEntry>>,

    /// Dependency graph (plugin -> dependencies)
    dependencies: Arc<DashMap<String, Vec<String>>>,
}

/// Plugin entry with metadata and state
#[derive(Clone)]
pub struct PluginEntry {
    /// Plugin instance
    pub plugin: Arc<tokio::sync::RwLock<Box<dyn Plugin>>>,

    /// Plugin metadata
    pub metadata: PluginMetadata,

    /// Plugin state
    pub state: Arc<parking_lot::RwLock<PluginState>>,

    /// Configuration
    pub config: Arc<parking_lot::RwLock<serde_json::Value>>,

    /// When the plugin was registered
    pub registered_at: Instant,

    /// When the plugin was last started
    pub started_at: Arc<parking_lot::RwLock<Option<Instant>>>,
}

impl std::fmt::Debug for PluginEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginEntry")
            .field("metadata", &self.metadata)
            .field("state", &self.state)
            .field("registered_at", &self.registered_at)
            .finish()
    }
}

/// Plugin state
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginState {
    /// Plugin is registered but not initialized
    Registered,

    /// Plugin is initialized
    Initialized,

    /// Plugin is started and running
    Started,

    /// Plugin is stopped
    Stopped,

    /// Plugin failed with an error
    Failed(String),
}

impl PluginRegistry {
    /// Create a new plugin registry
    pub fn new() -> Self {
        Self {
            plugins: Arc::new(DashMap::new()),
            dependencies: Arc::new(DashMap::new()),
        }
    }

    /// Register a plugin
    ///
    /// Registers a plugin with the given name and instance.
    /// The plugin will be in the `Registered` state after this call.
    pub async fn register(&self, name: impl Into<String>, plugin: Box<dyn Plugin>) -> Result<()> {
        let name = name.into();

        if self.plugins.contains_key(&name) {
            return Err(PluginRuntimeError::already_exists(&name));
        }

        let metadata = plugin.metadata();
        let dependencies = plugin.dependencies();

        // Validate dependencies exist
        for dep in &dependencies {
            if !dep.optional && !self.plugins.contains_key(&dep.name) {
                return Err(PluginRuntimeError::dependency_missing(&dep.name));
            }
        }

        // Check for dependency cycles
        self.check_dependency_cycle(&name, &dependencies)?;

        let entry = PluginEntry {
            plugin: Arc::new(tokio::sync::RwLock::new(plugin)),
            metadata: metadata.clone(),
            state: Arc::new(parking_lot::RwLock::new(PluginState::Registered)),
            config: Arc::new(parking_lot::RwLock::new(serde_json::Value::Null)),
            registered_at: Instant::now(),
            started_at: Arc::new(parking_lot::RwLock::new(None)),
        };

        self.plugins.insert(name.clone(), entry);

        // Store dependencies
        let dep_names: Vec<String> = dependencies.iter().map(|d| d.name.clone()).collect();
        self.dependencies.insert(name.clone(), dep_names);

        info!(plugin = %name, "Plugin registered");

        Ok(())
    }

    /// Initialize a plugin with configuration
    pub async fn initialize(&self, name: &str, config: serde_json::Value) -> Result<()> {
        let entry = self
            .plugins
            .get(name)
            .ok_or_else(|| PluginRuntimeError::not_found(name))?;

        // Check current state
        {
            let state = entry.state.read();
            if *state != PluginState::Registered {
                return Err(PluginRuntimeError::invalid_state(format!(
                    "Plugin {name} is not in Registered state"
                )));
            }
        }

        // Initialize plugin
        let mut plugin = entry.plugin.write().await;
        match plugin.init(config.clone()).await {
            Ok(()) => {
                *entry.state.write() = PluginState::Initialized;
                *entry.config.write() = config;
                info!(plugin = %name, "Plugin initialized");
                Ok(())
            }
            Err(e) => {
                *entry.state.write() = PluginState::Failed(e.to_string());
                error!(plugin = %name, error = %e, "Plugin initialization failed");
                Err(e.into())
            }
        }
    }

    /// Start a plugin
    pub async fn start(&self, name: &str) -> Result<()> {
        self.start_internal(name).await
    }

    /// Internal start implementation (for recursive calls with Box::pin)
    fn start_internal<'a>(
        &'a self,
        name: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            let entry = self
                .plugins
                .get(name)
                .ok_or_else(|| PluginRuntimeError::not_found(name))?;

            // Check current state
            {
                let state = entry.state.read();
                if *state != PluginState::Initialized && *state != PluginState::Stopped {
                    return Err(PluginRuntimeError::invalid_state(format!(
                        "Plugin {name} is not in Initialized or Stopped state"
                    )));
                }
            }

            // Start dependencies first
            if let Some(deps) = self.dependencies.get(name) {
                for dep_name in deps.value() {
                    if let Some(dep_entry) = self.plugins.get(dep_name) {
                        let should_start = {
                            let dep_state = dep_entry.state.read();
                            *dep_state != PluginState::Started
                        };

                        if should_start {
                            self.start_internal(dep_name).await?;
                        }
                    }
                }
            }

            // Start plugin
            let mut plugin = entry.plugin.write().await;
            match plugin.start().await {
                Ok(()) => {
                    *entry.state.write() = PluginState::Started;
                    *entry.started_at.write() = Some(Instant::now());
                    info!(plugin = %name, "Plugin started");
                    Ok(())
                }
                Err(e) => {
                    *entry.state.write() = PluginState::Failed(e.to_string());
                    error!(plugin = %name, error = %e, "Plugin start failed");
                    Err(e.into())
                }
            }
        })
    }

    /// Stop a plugin
    pub async fn stop(&self, name: &str) -> Result<()> {
        let entry = self
            .plugins
            .get(name)
            .ok_or_else(|| PluginRuntimeError::not_found(name))?;

        // Check current state
        {
            let state = entry.state.read();
            if *state != PluginState::Started {
                return Err(PluginRuntimeError::invalid_state(format!(
                    "Plugin {name} is not in Started state"
                )));
            }
        }

        // Stop plugin
        let mut plugin = entry.plugin.write().await;
        match plugin.stop().await {
            Ok(()) => {
                *entry.state.write() = PluginState::Stopped;
                info!(plugin = %name, "Plugin stopped");
                Ok(())
            }
            Err(e) => {
                *entry.state.write() = PluginState::Failed(e.to_string());
                error!(plugin = %name, error = %e, "Plugin stop failed");
                Err(e.into())
            }
        }
    }

    /// Reload a plugin with new configuration
    pub async fn reload(&self, name: &str, config: serde_json::Value) -> Result<()> {
        let entry = self
            .plugins
            .get(name)
            .ok_or_else(|| PluginRuntimeError::not_found(name))?;

        let mut plugin = entry.plugin.write().await;
        match plugin.reload(config.clone()).await {
            Ok(()) => {
                *entry.config.write() = config;
                info!(plugin = %name, "Plugin reloaded");
                Ok(())
            }
            Err(e) => {
                *entry.state.write() = PluginState::Failed(e.to_string());
                error!(plugin = %name, error = %e, "Plugin reload failed");
                Err(e.into())
            }
        }
    }

    /// Get plugin health status
    pub async fn health_check(&self, name: &str) -> Result<HealthStatus> {
        let entry = self
            .plugins
            .get(name)
            .ok_or_else(|| PluginRuntimeError::not_found(name))?;

        let plugin = entry.plugin.read().await;
        plugin.health_check().await.map_err(Into::into)
    }

    /// Start all plugins
    pub async fn start_all(&self) -> Result<()> {
        let plugin_names: Vec<String> = self.plugins.iter().map(|e| e.key().clone()).collect();

        for name in plugin_names {
            if let Err(e) = self.start(&name).await {
                warn!(plugin = %name, error = %e, "Failed to start plugin");
            }
        }

        Ok(())
    }

    /// Stop all plugins
    pub async fn stop_all(&self) -> Result<()> {
        let plugin_names: Vec<String> = self.plugins.iter().map(|e| e.key().clone()).collect();

        for name in plugin_names {
            if let Err(e) = self.stop(&name).await {
                warn!(plugin = %name, error = %e, "Failed to stop plugin");
            }
        }

        Ok(())
    }

    /// Get plugin by name
    pub fn get(&self, name: &str) -> Option<PluginEntry> {
        self.plugins.get(name).map(|e| e.clone())
    }

    /// List all plugins
    pub fn list(&self) -> Vec<PluginInfo> {
        self.plugins
            .iter()
            .map(|entry| {
                let state = entry.state.read().clone();
                let started_at = *entry.started_at.read();

                PluginInfo {
                    metadata: entry.metadata.clone(),
                    state: match state {
                        PluginState::Registered => octopus_plugin_api::PluginState::Loaded,
                        PluginState::Initialized => octopus_plugin_api::PluginState::Initialized,
                        PluginState::Started => octopus_plugin_api::PluginState::Started,
                        PluginState::Stopped => octopus_plugin_api::PluginState::Stopped,
                        PluginState::Failed(msg) => octopus_plugin_api::PluginState::Failed(msg),
                    },
                    loaded_at: Some(entry.registered_at),
                    started_at,
                    uptime: started_at.map(|t| t.elapsed()),
                    health: None,
                }
            })
            .collect()
    }

    /// Check for dependency cycles
    fn check_dependency_cycle(&self, name: &str, deps: &[PluginDependency]) -> Result<()> {
        let mut visited = std::collections::HashSet::new();
        let mut stack = vec![name.to_string()];

        for dep in deps {
            if dep.name == name {
                return Err(PluginRuntimeError::dependency_cycle(format!(
                    "Plugin {name} depends on itself"
                )));
            }

            if let Some(dep_deps) = self.dependencies.get(&dep.name) {
                stack.push(dep.name.clone());
                if !self.check_cycle_recursive(&dep.name, dep_deps.value(), &mut visited, &stack)? {
                    return Err(PluginRuntimeError::dependency_cycle(format!(
                        "Cycle detected involving {name}"
                    )));
                }
                stack.pop();
            }
        }

        Ok(())
    }

    fn check_cycle_recursive(
        &self,
        current: &str,
        deps: &[String],
        visited: &mut std::collections::HashSet<String>,
        stack: &[String],
    ) -> Result<bool> {
        if stack.contains(&current.to_string()) {
            return Ok(false); // Cycle detected
        }

        if visited.contains(current) {
            return Ok(true); // Already checked
        }

        visited.insert(current.to_string());

        for dep in deps {
            if let Some(dep_deps) = self.dependencies.get(dep) {
                if !self.check_cycle_recursive(dep, dep_deps.value(), visited, stack)? {
                    return Ok(false);
                }
            }
        }

        Ok(true)
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use octopus_plugin_api::PluginError;

    #[derive(Debug)]
    struct TestPlugin {
        name: String,
        started: bool,
    }

    impl TestPlugin {
        fn new(name: impl Into<String>) -> Self {
            Self {
                name: name.into(),
                started: false,
            }
        }
    }

    #[async_trait]
    impl Plugin for TestPlugin {
        fn name(&self) -> &str {
            &self.name
        }

        fn version(&self) -> &str {
            "1.0.0"
        }

        async fn init(
            &mut self,
            _config: serde_json::Value,
        ) -> std::result::Result<(), PluginError> {
            Ok(())
        }

        async fn start(&mut self) -> std::result::Result<(), PluginError> {
            self.started = true;
            Ok(())
        }

        async fn stop(&mut self) -> std::result::Result<(), PluginError> {
            self.started = false;
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_plugin_registration() {
        let registry = PluginRegistry::new();
        let plugin = Box::new(TestPlugin::new("test"));

        registry.register("test", plugin).await.unwrap();

        assert!(registry.get("test").is_some());
    }

    #[tokio::test]
    async fn test_plugin_lifecycle() {
        let registry = PluginRegistry::new();
        let plugin = Box::new(TestPlugin::new("test"));

        registry.register("test", plugin).await.unwrap();

        registry
            .initialize("test", serde_json::json!({}))
            .await
            .unwrap();

        registry.start("test").await.unwrap();

        let entry = registry.get("test").unwrap();
        assert_eq!(*entry.state.read(), PluginState::Started);

        registry.stop("test").await.unwrap();
    }

    #[tokio::test]
    async fn test_plugin_list() {
        let registry = PluginRegistry::new();

        registry
            .register("test1", Box::new(TestPlugin::new("test1")))
            .await
            .unwrap();

        registry
            .register("test2", Box::new(TestPlugin::new("test2")))
            .await
            .unwrap();

        let plugins = registry.list();
        assert_eq!(plugins.len(), 2);
    }
}
