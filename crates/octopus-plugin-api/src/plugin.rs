//! Core plugin trait and types

use crate::error::Result;
use async_trait::async_trait;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::{Duration, Instant};

/// Core plugin trait that all plugins must implement
#[async_trait]
pub trait Plugin: Send + Sync + fmt::Debug {
    /// Plugin name (must be unique)
    fn name(&self) -> &str;

    /// Plugin version (semver)
    fn version(&self) -> &str;

    /// Plugin description
    fn description(&self) -> &str {
        ""
    }

    /// Plugin author
    fn author(&self) -> &str {
        "Unknown"
    }

    /// Plugin homepage/repository URL
    fn homepage(&self) -> Option<&str> {
        None
    }

    /// Plugin dependencies (other plugins this depends on)
    fn dependencies(&self) -> Vec<PluginDependency> {
        vec![]
    }

    /// Initialize plugin with configuration
    ///
    /// This is called once when the plugin is loaded.
    async fn init(&mut self, config: serde_json::Value) -> Result<()>;

    /// Start plugin (called after all plugins are initialized)
    ///
    /// This is where you should start background tasks, open connections, etc.
    async fn start(&mut self) -> Result<()>;

    /// Stop plugin (graceful shutdown)
    ///
    /// This is where you should clean up resources, close connections, etc.
    async fn stop(&mut self) -> Result<()>;

    /// Health check
    ///
    /// Returns the current health status of the plugin.
    async fn health_check(&self) -> Result<HealthStatus> {
        Ok(HealthStatus::Healthy)
    }

    /// Reload plugin configuration
    ///
    /// Default implementation: stop, init, start.
    /// Override for more efficient reloading.
    async fn reload(&mut self, config: serde_json::Value) -> Result<()> {
        self.stop().await?;
        self.init(config).await?;
        self.start().await
    }

    /// Get plugin metadata
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: self.name().to_string(),
            version: self.version().to_string(),
            description: self.description().to_string(),
            author: self.author().to_string(),
            homepage: self.homepage().map(String::from),
            dependencies: self.dependencies(),
        }
    }
}

/// Plugin dependency specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginDependency {
    /// Plugin name
    pub name: String,

    /// Version requirement (semver)
    pub version_req: String,

    /// Whether this dependency is optional
    pub optional: bool,
}

impl PluginDependency {
    /// Create a required dependency
    pub fn required(name: impl Into<String>, version_req: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version_req: version_req.into(),
            optional: false,
        }
    }

    /// Create an optional dependency
    pub fn optional(name: impl Into<String>, version_req: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version_req: version_req.into(),
            optional: true,
        }
    }

    /// Check if a version satisfies this dependency
    pub fn satisfies(&self, version: &str) -> bool {
        let Ok(req) = semver::VersionReq::parse(&self.version_req) else {
            return false;
        };
        let Ok(ver) = Version::parse(version) else {
            return false;
        };
        req.matches(&ver)
    }
}

/// Plugin health status
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", content = "message")]
pub enum HealthStatus {
    /// Plugin is healthy and operating normally
    Healthy,

    /// Plugin is degraded but still functioning
    Degraded(String),

    /// Plugin is unhealthy and not functioning
    Unhealthy(String),
}

impl HealthStatus {
    /// Check if the plugin is healthy
    pub fn is_healthy(&self) -> bool {
        matches!(self, HealthStatus::Healthy)
    }

    /// Check if the plugin is degraded
    pub fn is_degraded(&self) -> bool {
        matches!(self, HealthStatus::Degraded(_))
    }

    /// Check if the plugin is unhealthy
    pub fn is_unhealthy(&self) -> bool {
        matches!(self, HealthStatus::Unhealthy(_))
    }

    /// Get the health message if any
    pub fn message(&self) -> Option<&str> {
        match self {
            HealthStatus::Healthy => None,
            HealthStatus::Degraded(msg) | HealthStatus::Unhealthy(msg) => Some(msg),
        }
    }
}

/// Plugin metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMetadata {
    /// Plugin name
    pub name: String,

    /// Plugin version
    pub version: String,

    /// Plugin description
    pub description: String,

    /// Plugin author
    pub author: String,

    /// Plugin homepage
    pub homepage: Option<String>,

    /// Plugin dependencies
    pub dependencies: Vec<PluginDependency>,
}

/// Plugin information (runtime state)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInfo {
    /// Plugin metadata
    #[serde(flatten)]
    pub metadata: PluginMetadata,

    /// Plugin state
    pub state: PluginState,

    /// When the plugin was loaded
    #[serde(skip)]
    pub loaded_at: Option<Instant>,

    /// When the plugin was started
    #[serde(skip)]
    pub started_at: Option<Instant>,

    /// Plugin uptime (if started)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uptime: Option<Duration>,

    /// Plugin health status
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health: Option<HealthStatus>,
}

/// Plugin state
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginState {
    /// Plugin is loaded but not initialized
    Loaded,

    /// Plugin is initialized but not started
    Initialized,

    /// Plugin is started and running
    Started,

    /// Plugin is stopped
    Stopped,

    /// Plugin failed with an error
    Failed(String),
}

impl PluginState {
    /// Check if the plugin is started
    pub fn is_started(&self) -> bool {
        matches!(self, PluginState::Started)
    }

    /// Check if the plugin is stopped
    pub fn is_stopped(&self) -> bool {
        matches!(self, PluginState::Stopped)
    }

    /// Check if the plugin has failed
    pub fn is_failed(&self) -> bool {
        matches!(self, PluginState::Failed(_))
    }
}

impl fmt::Display for PluginState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PluginState::Loaded => write!(f, "loaded"),
            PluginState::Initialized => write!(f, "initialized"),
            PluginState::Started => write!(f, "started"),
            PluginState::Stopped => write!(f, "stopped"),
            PluginState::Failed(msg) => write!(f, "failed: {}", msg),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_dependency() {
        let dep = PluginDependency::required("auth", "1.0.0");
        assert!(!dep.optional);
        assert_eq!(dep.name, "auth");

        assert!(dep.satisfies("1.0.0"));
        assert!(dep.satisfies("1.0.1"));
        assert!(!dep.satisfies("0.9.0"));
        assert!(!dep.satisfies("2.0.0"));
    }

    #[test]
    fn test_health_status() {
        let healthy = HealthStatus::Healthy;
        assert!(healthy.is_healthy());
        assert!(!healthy.is_degraded());
        assert!(!healthy.is_unhealthy());
        assert_eq!(healthy.message(), None);

        let degraded = HealthStatus::Degraded("slow".to_string());
        assert!(!degraded.is_healthy());
        assert!(degraded.is_degraded());
        assert_eq!(degraded.message(), Some("slow"));

        let unhealthy = HealthStatus::Unhealthy("down".to_string());
        assert!(!unhealthy.is_healthy());
        assert!(unhealthy.is_unhealthy());
        assert_eq!(unhealthy.message(), Some("down"));
    }

    #[test]
    fn test_plugin_state() {
        let state = PluginState::Started;
        assert!(state.is_started());
        assert!(!state.is_stopped());
        assert!(!state.is_failed());
        assert_eq!(state.to_string(), "started");

        let state = PluginState::Failed("error".to_string());
        assert!(!state.is_started());
        assert!(state.is_failed());
        assert_eq!(state.to_string(), "failed: error");
    }
}

