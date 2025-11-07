//! Plugin system for the admin dashboard
//!
//! Plugins can extend the dashboard by registering views, stats cards, and nav items.

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;

use crate::models::PluginStatsCard;

/// Plugin metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMetadata {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: Option<String>,
}

/// A dashboard view provided by a plugin
#[derive(Clone)]
pub struct DashboardView {
    pub id: String,
    pub title: String,
    pub path: String,
    pub icon: String,
    pub priority: i32,
    /// Function that renders the view's HTML
    pub render: fn() -> String,
}

/// A navigation item provided by a plugin
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavItem {
    pub label: String,
    pub icon: String,
    pub path: String,
    pub priority: i32,
}

/// API endpoint provided by a plugin
#[derive(Clone)]
pub struct ApiEndpoint {
    pub path: String,
    pub method: String,
    /// Handler function (simplified for now)
    pub handler: fn() -> String,
}

/// Dashboard plugin trait
pub trait DashboardPlugin: Send + Sync {
    /// Get plugin metadata
    fn metadata(&self) -> PluginMetadata;

    /// Register dashboard views
    fn register_views(&self) -> Vec<DashboardView> {
        vec![]
    }

    /// Register stats cards for the overview page
    fn register_stats_cards(&self) -> Vec<PluginStatsCard> {
        vec![]
    }

    /// Register navigation items
    fn register_nav_items(&self) -> Vec<NavItem> {
        vec![]
    }

    /// Register API endpoints
    fn register_api_endpoints(&self) -> Vec<ApiEndpoint> {
        vec![]
    }
}

/// Global plugin registry
static PLUGIN_REGISTRY: Lazy<RwLock<PluginRegistry>> =
    Lazy::new(|| RwLock::new(PluginRegistry::new()));

/// Plugin registry
#[derive(Default)]
pub struct PluginRegistry {
    plugins: HashMap<String, Box<dyn DashboardPlugin>>,
    views: Vec<DashboardView>,
    stats_cards: Vec<PluginStatsCard>,
    nav_items: Vec<NavItem>,
    api_endpoints: Vec<ApiEndpoint>,
}

impl PluginRegistry {
    /// Create a new plugin registry
    #[must_use]
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
            views: Vec::new(),
            stats_cards: Vec::new(),
            nav_items: Vec::new(),
            api_endpoints: Vec::new(),
        }
    }

    /// Register a plugin
    pub fn register(&mut self, plugin: Box<dyn DashboardPlugin>) -> Result<(), String> {
        let metadata = plugin.metadata();
        let plugin_id = metadata.id;

        if self.plugins.contains_key(&plugin_id) {
            return Err(format!(
                "Plugin with id '{plugin_id}' is already registered"
            ));
        }

        // Register views
        self.views.extend(plugin.register_views());

        // Register stats cards
        self.stats_cards.extend(plugin.register_stats_cards());

        // Register nav items
        self.nav_items.extend(plugin.register_nav_items());

        // Register API endpoints
        self.api_endpoints.extend(plugin.register_api_endpoints());

        // Store plugin
        self.plugins.insert(plugin_id, plugin);

        Ok(())
    }

    /// Get all registered plugins
    #[must_use]
    pub fn get_plugins_metadata(&self) -> Vec<PluginMetadata> {
        self.plugins.values().map(|p| p.metadata()).collect()
    }

    /// Get all registered views
    #[must_use]
    pub fn get_views(&self) -> &[DashboardView] {
        &self.views
    }

    /// Get all registered stats cards
    #[must_use]
    pub fn get_stats_cards(&self) -> &[PluginStatsCard] {
        &self.stats_cards
    }

    /// Get all registered nav items
    #[must_use]
    pub fn get_nav_items(&self) -> &[NavItem] {
        &self.nav_items
    }

    /// Get all registered API endpoints
    #[must_use]
    pub fn get_api_endpoints(&self) -> &[ApiEndpoint] {
        &self.api_endpoints
    }
}

// Public API functions

/// Register a plugin
///
/// # Errors
///
/// Returns an error if a plugin with the same ID is already registered
pub fn register_plugin(plugin: Box<dyn DashboardPlugin>) -> Result<(), String> {
    let mut registry = PLUGIN_REGISTRY
        .write()
        .map_err(|e| format!("Failed to acquire write lock: {e}"))?;
    registry.register(plugin)
}

/// Get all registered plugins
///
/// # Errors
///
/// Returns an error if the registry lock is poisoned
pub fn get_plugins_metadata() -> Result<Vec<PluginMetadata>, String> {
    let registry = PLUGIN_REGISTRY
        .read()
        .map_err(|e| format!("Failed to acquire read lock: {e}"))?;
    Ok(registry.get_plugins_metadata())
}

/// Get all registered views
///
/// # Errors
///
/// Returns an error if the registry lock is poisoned
pub fn get_plugin_views() -> Result<Vec<DashboardView>, String> {
    let registry = PLUGIN_REGISTRY
        .read()
        .map_err(|e| format!("Failed to acquire read lock: {e}"))?;
    Ok(registry.get_views().to_vec())
}

/// Get all registered stats cards
///
/// # Errors
///
/// Returns an error if the registry lock is poisoned
pub fn get_plugin_stats_cards() -> Result<Vec<PluginStatsCard>, String> {
    let registry = PLUGIN_REGISTRY
        .read()
        .map_err(|e| format!("Failed to acquire read lock: {e}"))?;
    Ok(registry.get_stats_cards().to_vec())
}

/// Get all registered nav items
///
/// # Errors
///
/// Returns an error if the registry lock is poisoned
pub fn get_plugin_nav_items() -> Result<Vec<NavItem>, String> {
    let registry = PLUGIN_REGISTRY
        .read()
        .map_err(|e| format!("Failed to acquire read lock: {e}"))?;
    Ok(registry.get_nav_items().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestPlugin;

    impl DashboardPlugin for TestPlugin {
        fn metadata(&self) -> PluginMetadata {
            PluginMetadata {
                id: "test".to_string(),
                name: "Test Plugin".to_string(),
                version: "0.1.0".to_string(),
                description: "A test plugin".to_string(),
                author: Some("Test Author".to_string()),
            }
        }

        fn register_views(&self) -> Vec<DashboardView> {
            vec![DashboardView {
                id: "test-view".to_string(),
                title: "Test View".to_string(),
                path: "/test".to_string(),
                icon: "🧪".to_string(),
                priority: 50,
                render: || "<h1>Test View</h1>".to_string(),
            }]
        }
    }

    #[test]
    fn test_plugin_registration() {
        let mut registry = PluginRegistry::new();
        let plugin = Box::new(TestPlugin);

        assert!(registry.register(plugin).is_ok());
        assert_eq!(registry.get_plugins_metadata().len(), 1);
        assert_eq!(registry.get_views().len(), 1);
    }

    #[test]
    fn test_duplicate_plugin_registration() {
        let mut registry = PluginRegistry::new();

        let plugin1 = Box::new(TestPlugin);
        assert!(registry.register(plugin1).is_ok());

        let plugin2 = Box::new(TestPlugin);
        assert!(registry.register(plugin2).is_err());
    }
}
