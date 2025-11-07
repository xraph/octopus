//! Plugin registry

use crate::traits::Plugin;
use octopus_core::{Error, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Plugin registry
#[derive(Clone)]
pub struct PluginRegistry {
    plugins: Arc<RwLock<HashMap<String, Arc<RwLock<Box<dyn Plugin>>>>>>,
}

impl std::fmt::Debug for PluginRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginRegistry")
            .field("plugins", &"<opaque>")
            .finish()
    }
}

impl PluginRegistry {
    /// Create a new plugin registry
    pub fn new() -> Self {
        Self {
            plugins: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a plugin
    pub async fn register(&self, plugin: Box<dyn Plugin>) -> Result<()> {
        let name = plugin.metadata().name.clone();

        let mut plugins = self.plugins.write().await;

        if plugins.contains_key(&name) {
            return Err(Error::Plugin {
                plugin: name,
                message: "Plugin already registered".to_string(),
            });
        }

        plugins.insert(name.clone(), Arc::new(RwLock::new(plugin)));

        tracing::info!(plugin = %name, "Plugin registered");

        Ok(())
    }

    /// Unregister a plugin
    pub async fn unregister(&self, name: &str) -> Result<()> {
        let mut plugins = self.plugins.write().await;

        if plugins.remove(name).is_some() {
            tracing::info!(plugin = %name, "Plugin unregistered");
            Ok(())
        } else {
            Err(Error::Plugin {
                plugin: name.to_string(),
                message: "Plugin not found".to_string(),
            })
        }
    }

    /// Get a plugin by name
    pub async fn get(&self, name: &str) -> Option<Arc<RwLock<Box<dyn Plugin>>>> {
        let plugins = self.plugins.read().await;
        plugins.get(name).cloned()
    }

    /// Get all registered plugin names
    pub async fn list(&self) -> Vec<String> {
        let plugins = self.plugins.read().await;
        plugins.keys().cloned().collect()
    }

    /// Get plugin count
    pub async fn count(&self) -> usize {
        let plugins = self.plugins.read().await;
        plugins.len()
    }

    /// Initialize all plugins
    pub async fn init_all(&self) -> Result<()> {
        let plugins = self.plugins.read().await;

        for (name, plugin) in plugins.iter() {
            let mut plugin = plugin.write().await;
            plugin.init().await.map_err(|e| Error::Plugin {
                plugin: name.clone(),
                message: format!("Failed to initialize: {}", e),
            })?;
        }

        tracing::info!("All plugins initialized");
        Ok(())
    }

    /// Start all plugins
    pub async fn start_all(&self) -> Result<()> {
        let plugins = self.plugins.read().await;

        for (name, plugin) in plugins.iter() {
            let mut plugin = plugin.write().await;
            plugin.start().await.map_err(|e| Error::Plugin {
                plugin: name.clone(),
                message: format!("Failed to start: {}", e),
            })?;
        }

        tracing::info!("All plugins started");
        Ok(())
    }

    /// Stop all plugins
    pub async fn stop_all(&self) -> Result<()> {
        let plugins = self.plugins.read().await;

        for (name, plugin) in plugins.iter() {
            let mut plugin = plugin.write().await;
            plugin.stop().await.map_err(|e| Error::Plugin {
                plugin: name.clone(),
                message: format!("Failed to stop: {}", e),
            })?;
        }

        tracing::info!("All plugins stopped");
        Ok(())
    }

    /// Shutdown all plugins
    pub async fn shutdown_all(&self) -> Result<()> {
        let plugins = self.plugins.read().await;

        for (name, plugin) in plugins.iter() {
            let mut plugin = plugin.write().await;
            plugin.shutdown().await.map_err(|e| Error::Plugin {
                plugin: name.clone(),
                message: format!("Failed to shutdown: {}", e),
            })?;
        }

        tracing::info!("All plugins shutdown");
        Ok(())
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
    async fn test_registry_new() {
        let registry = PluginRegistry::new();
        assert_eq!(registry.count().await, 0);
    }

    #[tokio::test]
    async fn test_register_plugin() {
        let registry = PluginRegistry::new();

        let plugin = Box::new(TestPlugin {
            metadata: PluginMetadata::new("test", "1.0.0"),
        });

        registry.register(plugin).await.unwrap();
        assert_eq!(registry.count().await, 1);
    }

    #[tokio::test]
    async fn test_duplicate_registration() {
        let registry = PluginRegistry::new();

        let plugin1 = Box::new(TestPlugin {
            metadata: PluginMetadata::new("test", "1.0.0"),
        });

        let plugin2 = Box::new(TestPlugin {
            metadata: PluginMetadata::new("test", "1.0.0"),
        });

        registry.register(plugin1).await.unwrap();
        let result = registry.register(plugin2).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_list_plugins() {
        let registry = PluginRegistry::new();

        let plugin = Box::new(TestPlugin {
            metadata: PluginMetadata::new("test", "1.0.0"),
        });

        registry.register(plugin).await.unwrap();

        let list = registry.list().await;
        assert_eq!(list.len(), 1);
        assert!(list.contains(&"test".to_string()));
    }
}
