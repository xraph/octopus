//! Script engine trait and abstractions

use crate::context::ScriptContext;
use crate::error::{Result, ScriptError};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

/// Supported scripting languages
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScriptLanguage {
    /// Rhai scripting language
    Rhai,
    /// Lua scripting language (future)
    Lua,
    /// JavaScript via Deno (future)
    JavaScript,
    /// WebAssembly (future)
    Wasm,
}

impl ScriptLanguage {
    /// Get file extension for this language
    pub fn extension(&self) -> &str {
        match self {
            Self::Rhai => "rhai",
            Self::Lua => "lua",
            Self::JavaScript => "js",
            Self::Wasm => "wasm",
        }
    }

    /// Detect language from file extension
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "rhai" => Some(Self::Rhai),
            "lua" => Some(Self::Lua),
            "js" | "javascript" => Some(Self::JavaScript),
            "wasm" => Some(Self::Wasm),
            _ => None,
        }
    }
}

impl fmt::Display for ScriptLanguage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rhai => write!(f, "rhai"),
            Self::Lua => write!(f, "lua"),
            Self::JavaScript => write!(f, "javascript"),
            Self::Wasm => write!(f, "wasm"),
        }
    }
}

/// Script source (inline or file-based)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ScriptSource {
    /// Inline script code
    Inline {
        /// Script code
        code: String,
        /// Optional name for debugging
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    /// File-based script
    File {
        /// Path to script file
        path: PathBuf,
    },
}

impl ScriptSource {
    /// Create inline script source
    pub fn inline<S: Into<String>>(code: S) -> Self {
        Self::Inline {
            code: code.into(),
            name: None,
        }
    }

    /// Create inline script with name
    pub fn inline_named<S: Into<String>, N: Into<String>>(code: S, name: N) -> Self {
        Self::Inline {
            code: code.into(),
            name: Some(name.into()),
        }
    }

    /// Create file-based script source
    pub fn file<P: Into<PathBuf>>(path: P) -> Self {
        Self::File { path: path.into() }
    }

    /// Get script code (loads from file if needed)
    pub async fn get_code(&self) -> Result<String> {
        match self {
            Self::Inline { code, .. } => Ok(code.clone()),
            Self::File { path } => tokio::fs::read_to_string(path)
                .await
                .map_err(|e| ScriptError::IoError {
                    message: format!("Failed to read script file {:?}: {}", path, e),
                }),
        }
    }

    /// Get a descriptive name for this script
    pub fn name(&self) -> String {
        match self {
            Self::Inline { name, .. } => name.clone().unwrap_or_else(|| "inline".to_string()),
            Self::File { path } => path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string(),
        }
    }
}

/// Script engine trait
///
/// This trait abstracts over different scripting languages.
#[async_trait]
pub trait ScriptEngine: Send + Sync + fmt::Debug {
    /// Get the language this engine supports
    fn language(&self) -> ScriptLanguage;

    /// Compile/prepare a script (optional, for caching)
    async fn prepare(&self, source: &ScriptSource) -> Result<()> {
        // Default: no preparation needed
        let _ = source;
        Ok(())
    }

    /// Execute script on request (before routing)
    ///
    /// Returns whether to continue processing (true) or short-circuit (false)
    async fn execute_request(
        &self,
        source: &ScriptSource,
        ctx: &mut ScriptContext,
    ) -> Result<bool>;

    /// Execute script on response (after upstream)
    ///
    /// Returns whether to continue processing (true)
    async fn execute_response(
        &self,
        source: &ScriptSource,
        ctx: &mut ScriptContext,
    ) -> Result<bool>;

    /// Clear cached ASTs/compiled scripts
    async fn clear_cache(&self) -> Result<()> {
        Ok(())
    }

    /// Get cache statistics (compiled scripts, hit rate, etc.)
    fn cache_stats(&self) -> CacheStats {
        CacheStats::default()
    }
}

/// Cache statistics for script engines
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    /// Number of compiled scripts in cache
    pub cached_scripts: usize,
    /// Cache hits
    pub hits: u64,
    /// Cache misses
    pub misses: u64,
    /// Total cache size in bytes (approximate)
    pub size_bytes: usize,
}

impl CacheStats {
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



