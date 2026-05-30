//! Script interceptor plugin trait

use crate::context::{RequestContext, ResponseContext};
use crate::error::Result;
use crate::plugin::Plugin;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Script language supported by plugin
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScriptLanguage {
    /// Rhai scripting language
    Rhai,
    /// Lua scripting language
    Lua,
    /// JavaScript
    JavaScript,
    /// WebAssembly
    Wasm,
    /// Custom language
    Custom,
}

/// Script source for plugins
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptConfig {
    /// Script language
    pub language: ScriptLanguage,

    /// Inline script code (if provided)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,

    /// Script file path (if provided)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,

    /// Whether to run on requests
    #[serde(default = "default_true")]
    pub on_request: bool,

    /// Whether to run on responses
    #[serde(default)]
    pub on_response: bool,

    /// Timeout in milliseconds
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
}

fn default_true() -> bool {
    true
}

fn default_timeout() -> u64 {
    100
}

/// Plugin that provides script-based interception
///
/// This allows plugins to execute custom scripts for request/response transformation.
/// The plugin is responsible for:
/// - Providing a scripting engine
/// - Compiling/caching scripts
/// - Executing scripts safely with timeouts
/// - Exposing request/response context to scripts
#[async_trait]
pub trait ScriptInterceptorPlugin: Plugin {
    /// Get supported script languages
    fn supported_languages(&self) -> Vec<ScriptLanguage>;

    /// Execute script on request
    ///
    /// Returns whether to continue processing (true) or short-circuit (false)
    async fn execute_on_request(
        &self,
        config: &ScriptConfig,
        ctx: &mut RequestContext,
    ) -> Result<bool>;

    /// Execute script on response  
    ///
    /// Returns whether to continue processing (true)
    async fn execute_on_response(
        &self,
        config: &ScriptConfig,
        ctx: &mut ResponseContext,
    ) -> Result<bool>;

    /// Pre-compile script (optional optimization)
    async fn prepare_script(&self, config: &ScriptConfig) -> Result<()> {
        let _ = config;
        Ok(())
    }

    /// Clear cached scripts
    async fn clear_cache(&self) -> Result<()> {
        Ok(())
    }

    /// Get cache statistics
    fn cache_stats(&self) -> ScriptCacheStats {
        ScriptCacheStats::default()
    }
}

/// Script cache statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScriptCacheStats {
    /// Number of compiled scripts in cache
    pub cached_scripts: usize,

    /// Cache hits
    pub hits: u64,

    /// Cache misses
    pub misses: u64,

    /// Total cache size in bytes
    pub size_bytes: usize,
}

impl ScriptCacheStats {
    /// Get cache hit rate (0.0 to 1.0)
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_script_config() {
        let config = ScriptConfig {
            language: ScriptLanguage::Rhai,
            code: Some("true".to_string()),
            file: None,
            on_request: true,
            on_response: false,
            timeout_ms: 100,
        };

        assert_eq!(config.language, ScriptLanguage::Rhai);
        assert!(config.on_request);
        assert!(!config.on_response);
    }

    #[test]
    fn test_cache_stats() {
        let stats = ScriptCacheStats {
            cached_scripts: 10,
            hits: 90,
            misses: 10,
            size_bytes: 102400,
        };

        assert_eq!(stats.hit_rate(), 0.9);
    }
}
