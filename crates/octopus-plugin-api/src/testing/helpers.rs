//! Test helpers and harness for plugin testing

use crate::context::{RequestContext, ResponseContext};
use crate::plugin::{HealthStatus, Plugin, PluginState};
use crate::PluginError;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

/// Plugin test harness for testing plugin lifecycle
pub struct PluginTestHarness<P: Plugin> {
    plugin: P,
    state: PluginState,
}

impl<P: Plugin> PluginTestHarness<P> {
    /// Create a new test harness with a plugin
    pub fn new(plugin: P) -> Self {
        Self {
            plugin,
            state: PluginState::Loaded,
        }
    }

    /// Get a reference to the plugin
    pub fn plugin(&self) -> &P {
        &self.plugin
    }

    /// Get a mutable reference to the plugin
    pub fn plugin_mut(&mut self) -> &mut P {
        &mut self.plugin
    }

    /// Get the current plugin state
    pub fn state(&self) -> &PluginState {
        &self.state
    }

    /// Initialize the plugin
    pub async fn init(&mut self, config: serde_json::Value) -> Result<(), PluginError> {
        match self.plugin.init(config).await {
            Ok(()) => {
                self.state = PluginState::Initialized;
                Ok(())
            }
            Err(e) => {
                self.state = PluginState::Failed(e.to_string());
                Err(e)
            }
        }
    }

    /// Start the plugin
    pub async fn start(&mut self) -> Result<(), PluginError> {
        match self.plugin.start().await {
            Ok(()) => {
                self.state = PluginState::Started;
                Ok(())
            }
            Err(e) => {
                self.state = PluginState::Failed(e.to_string());
                Err(e)
            }
        }
    }

    /// Stop the plugin
    pub async fn stop(&mut self) -> Result<(), PluginError> {
        match self.plugin.stop().await {
            Ok(()) => {
                self.state = PluginState::Stopped;
                Ok(())
            }
            Err(e) => {
                self.state = PluginState::Failed(e.to_string());
                Err(e)
            }
        }
    }

    /// Reload the plugin
    pub async fn reload(&mut self, config: serde_json::Value) -> Result<(), PluginError> {
        self.plugin.reload(config).await
    }

    /// Check plugin health
    pub async fn health_check(&self) -> Result<HealthStatus, PluginError> {
        self.plugin.health_check().await
    }

    /// Run full lifecycle test (init -> start -> stop)
    pub async fn run_lifecycle_test(
        &mut self,
        config: serde_json::Value,
    ) -> Result<(), PluginError> {
        self.init(config).await?;
        assert_eq!(*self.state(), PluginState::Initialized);

        self.start().await?;
        assert_eq!(*self.state(), PluginState::Started);

        self.stop().await?;
        assert_eq!(*self.state(), PluginState::Stopped);

        Ok(())
    }
}

/// Test context helper
pub struct TestContext {
    request_id: String,
    remote_addr: SocketAddr,
}

impl TestContext {
    /// Create a new test context with default values
    pub fn new() -> Self {
        Self {
            request_id: uuid::Uuid::new_v4().to_string(),
            remote_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080),
        }
    }

    /// Set custom request ID
    pub fn with_request_id(mut self, request_id: impl Into<String>) -> Self {
        self.request_id = request_id.into();
        self
    }

    /// Set custom remote address
    pub fn with_remote_addr(mut self, addr: SocketAddr) -> Self {
        self.remote_addr = addr;
        self
    }

    /// Build a RequestContext
    pub fn build_request_context(&self) -> RequestContext {
        RequestContext::new(self.request_id.clone(), self.remote_addr)
    }

    /// Build a ResponseContext
    pub fn build_response_context(
        &self,
        duration: Duration,
        status_code: u16,
    ) -> ResponseContext {
        ResponseContext::new(self.request_id.clone(), duration, status_code)
    }
}

impl Default for TestContext {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Plugin;
    use async_trait::async_trait;

    #[derive(Debug)]
    struct TestPlugin {
        initialized: bool,
        started: bool,
    }

    #[async_trait]
    impl Plugin for TestPlugin {
        fn name(&self) -> &str {
            "test"
        }

        fn version(&self) -> &str {
            "1.0.0"
        }

        async fn init(&mut self, _config: serde_json::Value) -> Result<(), PluginError> {
            self.initialized = true;
            Ok(())
        }

        async fn start(&mut self) -> Result<(), PluginError> {
            self.started = true;
            Ok(())
        }

        async fn stop(&mut self) -> Result<(), PluginError> {
            self.started = false;
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_plugin_harness() {
        let plugin = TestPlugin {
            initialized: false,
            started: false,
        };

        let mut harness = PluginTestHarness::new(plugin);

        harness.init(serde_json::json!({})).await.unwrap();
        assert_eq!(*harness.state(), PluginState::Initialized);
        assert!(harness.plugin().initialized);

        harness.start().await.unwrap();
        assert_eq!(*harness.state(), PluginState::Started);
        assert!(harness.plugin().started);

        harness.stop().await.unwrap();
        assert_eq!(*harness.state(), PluginState::Stopped);
        assert!(!harness.plugin().started);
    }

    #[tokio::test]
    async fn test_lifecycle_test() {
        let plugin = TestPlugin {
            initialized: false,
            started: false,
        };

        let mut harness = PluginTestHarness::new(plugin);
        harness
            .run_lifecycle_test(serde_json::json!({}))
            .await
            .unwrap();
    }

    #[test]
    fn test_context_builder() {
        let ctx = TestContext::new()
            .with_request_id("test-123");

        let req_ctx = ctx.build_request_context();
        assert_eq!(req_ctx.request_id, "test-123");

        let res_ctx = ctx.build_response_context(Duration::from_millis(100), 200);
        assert_eq!(res_ctx.request_id, "test-123");
        assert_eq!(res_ctx.status_code, 200);
    }
}

