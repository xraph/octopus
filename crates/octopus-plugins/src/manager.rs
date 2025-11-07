//! Plugin manager

use crate::registry::PluginRegistry;
use crate::traits::Plugin;
use octopus_core::Result;

/// Plugin manager
#[derive(Debug, Clone)]
pub struct PluginManager {
    registry: PluginRegistry,
}

impl PluginManager {
    /// Create a new plugin manager
    pub fn new() -> Self {
        Self {
            registry: PluginRegistry::new(),
        }
    }

    /// Get the plugin registry
    pub fn registry(&self) -> &PluginRegistry {
        &self.registry
    }

    /// Register a plugin
    pub async fn register(&self, plugin: Box<dyn Plugin>) -> Result<()> {
        self.registry.register(plugin).await
    }

    /// Initialize all plugins
    pub async fn init_all(&self) -> Result<()> {
        self.registry.init_all().await
    }

    /// Start all plugins
    pub async fn start_all(&self) -> Result<()> {
        self.registry.start_all().await
    }

    /// Stop all plugins
    pub async fn stop_all(&self) -> Result<()> {
        self.registry.stop_all().await
    }

    /// Shutdown all plugins
    pub async fn shutdown_all(&self) -> Result<()> {
        self.registry.shutdown_all().await
    }

    /// Get plugin count
    pub async fn plugin_count(&self) -> usize {
        self.registry.count().await
    }

    /// List all plugins
    pub async fn list_plugins(&self) -> Vec<String> {
        self.registry.list().await
    }
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::PluginMetadata;
    use async_trait::async_trait;

    struct TestPlugin {
        metadata: PluginMetadata,
    }

    #[async_trait]
    impl Plugin for TestPlugin {
        fn metadata(&self) -> &PluginMetadata {
            &self.metadata
        }
    }

    #[tokio::test]
    async fn test_manager_new() {
        let manager = PluginManager::new();
        assert_eq!(manager.plugin_count().await, 0);
    }

    #[tokio::test]
    async fn test_manager_register() {
        let manager = PluginManager::new();

        let plugin = Box::new(TestPlugin {
            metadata: PluginMetadata::new("test", "1.0.0"),
        });

        manager.register(plugin).await.unwrap();
        assert_eq!(manager.plugin_count().await, 1);
    }

    #[tokio::test]
    async fn test_manager_lifecycle() {
        let manager = PluginManager::new();

        let plugin = Box::new(TestPlugin {
            metadata: PluginMetadata::new("test", "1.0.0"),
        });

        manager.register(plugin).await.unwrap();

        // Test lifecycle methods
        manager.init_all().await.unwrap();
        manager.start_all().await.unwrap();
        manager.stop_all().await.unwrap();
        manager.shutdown_all().await.unwrap();
    }
}
