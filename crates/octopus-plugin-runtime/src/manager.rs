//! Plugin manager for high-level plugin operations

use crate::error::Result;
use crate::registry::{PluginEntry, PluginRegistry};
use octopus_plugin_api::{
    auth::AuthProvider,
    interceptor::{RequestInterceptor, ResponseInterceptor},
    protocol::ProtocolHandler,
    transform::TransformPlugin,
    Plugin, PluginInfo,
};
use std::sync::Arc;
use tracing::info;

/// Plugin manager for high-level plugin operations
///
/// Provides convenience methods for managing plugins, including
/// type-specific accessors and batch operations.
#[derive(Clone, Debug)]
pub struct PluginManager {
    registry: Arc<PluginRegistry>,
}

impl PluginManager {
    /// Create a new plugin manager
    pub fn new() -> Self {
        Self {
            registry: Arc::new(PluginRegistry::new()),
        }
    }

    /// Create a plugin manager with a custom registry
    pub fn with_registry(registry: Arc<PluginRegistry>) -> Self {
        Self { registry }
    }

    /// Get the underlying registry
    pub fn registry(&self) -> &Arc<PluginRegistry> {
        &self.registry
    }

    /// Register a plugin
    pub async fn register(&self, name: impl Into<String>, plugin: Box<dyn Plugin>) -> Result<()> {
        self.registry.register(name, plugin).await
    }

    /// Initialize a plugin with configuration
    pub async fn initialize(&self, name: &str, config: serde_json::Value) -> Result<()> {
        self.registry.initialize(name, config).await
    }

    /// Register and initialize a plugin in one call
    pub async fn register_and_init(
        &self,
        name: impl Into<String>,
        plugin: Box<dyn Plugin>,
        config: serde_json::Value,
    ) -> Result<()> {
        let name = name.into();
        self.registry.register(&name, plugin).await?;
        self.registry.initialize(&name, config).await?;
        Ok(())
    }

    /// Start a plugin
    pub async fn start(&self, name: &str) -> Result<()> {
        self.registry.start(name).await
    }

    /// Stop a plugin
    pub async fn stop(&self, name: &str) -> Result<()> {
        self.registry.stop(name).await
    }

    /// Reload a plugin
    pub async fn reload(&self, name: &str, config: serde_json::Value) -> Result<()> {
        self.registry.reload(name, config).await
    }

    /// Start all plugins
    pub async fn start_all(&self) -> Result<()> {
        info!("Starting all plugins");
        self.registry.start_all().await
    }

    /// Stop all plugins
    pub async fn stop_all(&self) -> Result<()> {
        info!("Stopping all plugins");
        self.registry.stop_all().await
    }

    /// Restart a plugin
    pub async fn restart(&self, name: &str) -> Result<()> {
        info!(plugin = %name, "Restarting plugin");
        self.stop(name).await?;
        self.start(name).await?;
        Ok(())
    }

    /// Get plugin by name
    pub fn get(&self, name: &str) -> Option<PluginEntry> {
        self.registry.get(name)
    }

    /// List all plugins
    pub fn list(&self) -> Vec<PluginInfo> {
        self.registry.list()
    }

    /// List plugins by type (filter by metadata)
    pub fn list_by_type(&self, plugin_type: &str) -> Vec<PluginInfo> {
        self.list()
            .into_iter()
            .filter(|info| info.metadata.description.contains(plugin_type))
            .collect()
    }

    /// Get all request interceptor plugins
    pub fn get_request_interceptors(&self) -> Vec<Arc<dyn RequestInterceptor>> {
        // Note: This requires plugins to be downcasted, which is not straightforward
        // In practice, you'd need to store plugins with their specific trait types
        // or use a type registry pattern. For now, return empty vec.
        vec![]
    }

    /// Get all response interceptor plugins
    pub fn get_response_interceptors(&self) -> Vec<Arc<dyn ResponseInterceptor>> {
        vec![]
    }

    /// Get all auth provider plugins
    pub fn get_auth_providers(&self) -> Vec<Arc<dyn AuthProvider>> {
        vec![]
    }

    /// Get all protocol handler plugins
    pub fn get_protocol_handlers(&self) -> Vec<Arc<dyn ProtocolHandler>> {
        vec![]
    }

    /// Get all transform plugins
    pub fn get_transform_plugins(&self) -> Vec<Arc<dyn TransformPlugin>> {
        vec![]
    }

    /// Check if a plugin exists
    pub fn exists(&self, name: &str) -> bool {
        self.registry.get(name).is_some()
    }

    /// Get plugin count
    pub fn count(&self) -> usize {
        self.list().len()
    }

    /// Get started plugin count
    pub fn started_count(&self) -> usize {
        self.list()
            .iter()
            .filter(|info| info.state.is_started())
            .count()
    }

    /// Health check all plugins
    pub async fn health_check_all(&self) -> Vec<(String, octopus_plugin_api::HealthStatus)> {
        let mut results = Vec::new();

        for info in self.list() {
            if let Ok(status) = self.registry.health_check(&info.metadata.name).await {
                results.push((info.metadata.name, status));
            }
        }

        results
    }

    /// Get plugin statistics
    pub fn stats(&self) -> PluginStats {
        let plugins = self.list();

        PluginStats {
            total: plugins.len(),
            started: plugins.iter().filter(|p| p.state.is_started()).count(),
            stopped: plugins.iter().filter(|p| p.state.is_stopped()).count(),
            failed: plugins.iter().filter(|p| p.state.is_failed()).count(),
        }
    }
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Plugin statistics
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct PluginStats {
    /// Total number of plugins
    pub total: usize,

    /// Number of started plugins
    pub started: usize,

    /// Number of stopped plugins
    pub stopped: usize,

    /// Number of failed plugins
    pub failed: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use octopus_plugin_api::PluginError;

    #[derive(Debug)]
    struct TestPlugin {
        name: String,
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
            Ok(())
        }

        async fn stop(&mut self) -> std::result::Result<(), PluginError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_plugin_manager() {
        let manager = PluginManager::new();

        manager
            .register_and_init(
                "test",
                Box::new(TestPlugin {
                    name: "test".to_string(),
                }),
                serde_json::json!({}),
            )
            .await
            .unwrap();

        assert!(manager.exists("test"));
        assert_eq!(manager.count(), 1);
    }

    #[tokio::test]
    async fn test_plugin_stats() {
        let manager = PluginManager::new();

        manager
            .register_and_init(
                "test",
                Box::new(TestPlugin {
                    name: "test".to_string(),
                }),
                serde_json::json!({}),
            )
            .await
            .unwrap();

        manager.start("test").await.unwrap();

        let stats = manager.stats();
        assert_eq!(stats.total, 1);
        assert_eq!(stats.started, 1);
    }
}
