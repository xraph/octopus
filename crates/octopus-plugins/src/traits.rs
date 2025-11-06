//! Plugin traits and types

use async_trait::async_trait;
use octopus_core::{Middleware, Result};
use serde::{Deserialize, Serialize};

/// Plugin type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PluginType {
    /// Static plugin (compiled into binary)
    Static,
    /// Dynamic plugin (loaded at runtime)
    Dynamic,
}

/// Plugin metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMetadata {
    /// Plugin name
    pub name: String,
    
    /// Plugin version
    pub version: String,
    
    /// Plugin author
    pub author: String,
    
    /// Plugin description
    pub description: String,
    
    /// Plugin type
    pub plugin_type: PluginType,
    
    /// Plugin dependencies
    pub dependencies: Vec<String>,
}

impl PluginMetadata {
    /// Create new plugin metadata
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            author: String::new(),
            description: String::new(),
            plugin_type: PluginType::Static,
            dependencies: Vec::new(),
        }
    }
}

/// Plugin trait
#[async_trait]
pub trait Plugin: Send + Sync {
    /// Get plugin metadata
    fn metadata(&self) -> &PluginMetadata;
    
    /// Initialize the plugin
    async fn init(&mut self) -> Result<()> {
        Ok(())
    }
    
    /// Start the plugin
    async fn start(&mut self) -> Result<()> {
        Ok(())
    }
    
    /// Stop the plugin
    async fn stop(&mut self) -> Result<()> {
        Ok(())
    }
    
    /// Shutdown the plugin
    async fn shutdown(&mut self) -> Result<()> {
        Ok(())
    }
    
    /// Get plugin as middleware (if applicable)
    fn as_middleware(&self) -> Option<&dyn Middleware> {
        None
    }
}

/// Plugin builder trait for dynamic loading
pub trait PluginBuilder: Send + Sync {
    /// Build a plugin instance
    fn build(&self, config: serde_json::Value) -> Result<Box<dyn Plugin>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_metadata_new() {
        let metadata = PluginMetadata::new("test-plugin", "1.0.0");
        assert_eq!(metadata.name, "test-plugin");
        assert_eq!(metadata.version, "1.0.0");
        assert_eq!(metadata.plugin_type, PluginType::Static);
    }

    #[test]
    fn test_plugin_type_eq() {
        assert_eq!(PluginType::Static, PluginType::Static);
        assert_ne!(PluginType::Static, PluginType::Dynamic);
    }
}


