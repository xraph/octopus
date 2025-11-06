//! Script middleware for request/response interception

use crate::context::{RequestContext, ResponseContext, ScriptContext};
use crate::engine::{ScriptEngine, ScriptLanguage, ScriptSource};
use crate::error::{Result as ScriptResult, ScriptError};
use crate::rhai_engine::RhaiEngine;
use async_trait::async_trait;
use http::{Request, Response};
use octopus_core::middleware::{Body, Middleware, Next};
use octopus_core::{Error, Result};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;
use tracing::{debug, error, trace, warn};

/// Script middleware configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptMiddlewareConfig {
    /// Script language (defaults to rhai)
    #[serde(default = "default_language")]
    pub language: ScriptLanguage,

    /// Script source (inline or file)
    #[serde(flatten)]
    pub source: ScriptSource,

    /// Whether to run on requests (default: true)
    #[serde(default = "default_true")]
    pub on_request: bool,

    /// Whether to run on responses (default: false)
    #[serde(default)]
    pub on_response: bool,

    /// Whether to continue on script errors (default: false)
    #[serde(default)]
    pub continue_on_error: bool,

    /// Maximum execution time in milliseconds (default: 100ms)
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
}

fn default_language() -> ScriptLanguage {
    ScriptLanguage::Rhai
}

fn default_true() -> bool {
    true
}

fn default_timeout() -> u64 {
    100 // 100ms default timeout
}

impl ScriptMiddlewareConfig {
    /// Create config for inline script
    pub fn inline<S: Into<String>>(code: S) -> Self {
        Self {
            language: ScriptLanguage::Rhai,
            source: ScriptSource::inline(code),
            on_request: true,
            on_response: false,
            continue_on_error: false,
            timeout_ms: 100,
        }
    }

    /// Create config for file-based script
    pub fn file<P: Into<std::path::PathBuf>>(path: P) -> Self {
        Self {
            language: ScriptLanguage::Rhai,
            source: ScriptSource::file(path),
            on_request: true,
            on_response: false,
            continue_on_error: false,
            timeout_ms: 100,
        }
    }

    /// Enable response interception
    pub fn with_response(mut self) -> Self {
        self.on_response = true;
        self
    }

    /// Set language
    pub fn with_language(mut self, language: ScriptLanguage) -> Self {
        self.language = language;
        self
    }

    /// Set timeout
    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    /// Continue on errors
    pub fn continue_on_error(mut self) -> Self {
        self.continue_on_error = true;
        self
    }
}

/// Script middleware
pub struct ScriptMiddleware {
    config: ScriptMiddlewareConfig,
    engine: Arc<dyn ScriptEngine>,
}

impl ScriptMiddleware {
    /// Create new script middleware
    pub fn new(config: ScriptMiddlewareConfig) -> Self {
        let engine: Arc<dyn ScriptEngine> = match config.language {
            ScriptLanguage::Rhai => Arc::new(RhaiEngine::new()),
            ScriptLanguage::Lua => {
                warn!("Lua scripting not yet implemented, falling back to Rhai");
                Arc::new(RhaiEngine::new())
            }
            ScriptLanguage::JavaScript => {
                warn!("JavaScript scripting not yet implemented, falling back to Rhai");
                Arc::new(RhaiEngine::new())
            }
            ScriptLanguage::Wasm => {
                warn!("WebAssembly scripting not yet implemented, falling back to Rhai");
                Arc::new(RhaiEngine::new())
            }
        };

        Self { config, engine }
    }

    /// Create with custom engine
    pub fn with_engine(config: ScriptMiddlewareConfig, engine: Arc<dyn ScriptEngine>) -> Self {
        Self { config, engine }
    }

    /// Pre-compile script (optional optimization)
    pub async fn prepare(&self) -> ScriptResult<()> {
        self.engine.prepare(&self.config.source).await
    }

    /// Execute script on request
    async fn execute_on_request(&self, req: &mut Request<Body>) -> ScriptResult<bool> {
        let start = std::time::Instant::now();
        
        // Extract request context
        let req_ctx = RequestContext::from_request(req);

        // Read body if present
        // Note: This is simplified - in production, you'd want to handle streaming bodies
        if let Some(content_length) = req.headers().get("content-length") {
            if let Ok(len_str) = content_length.to_str() {
                if let Ok(len) = len_str.parse::<usize>() {
                    if len > 0 && len < 10 * 1024 * 1024 {
                        // Read body (up to 10MB)
                        // In production, use body streaming
                        trace!("Reading request body for script");
                    }
                }
            }
        }

        let mut ctx = ScriptContext::Request(req_ctx);

        // Execute with timeout
        let timeout = tokio::time::Duration::from_millis(self.config.timeout_ms);
        let result = tokio::time::timeout(
            timeout,
            self.engine.execute_request(&self.config.source, &mut ctx),
        )
        .await
        .map_err(|_| ScriptError::timeout(self.config.timeout_ms))?;

        let elapsed = start.elapsed();
        trace!(
            script = %self.config.source.name(),
            elapsed_us = elapsed.as_micros(),
            "Script executed on request"
        );

        // Apply changes back to request
        if let Some(req_ctx) = ctx.as_request() {
            req_ctx.apply_to_request(req)
                .map_err(|e| ScriptError::runtime(e))?;
        }

        result
    }

    /// Execute script on response
    async fn execute_on_response(&self, res: &mut Response<Body>) -> ScriptResult<bool> {
        let start = std::time::Instant::now();
        
        // Extract response context
        let res_ctx = ResponseContext::from_response(res);

        let mut ctx = ScriptContext::Response(res_ctx);

        // Execute with timeout
        let timeout = tokio::time::Duration::from_millis(self.config.timeout_ms);
        let result = tokio::time::timeout(
            timeout,
            self.engine.execute_response(&self.config.source, &mut ctx),
        )
        .await
        .map_err(|_| ScriptError::timeout(self.config.timeout_ms))?;

        let elapsed = start.elapsed();
        trace!(
            script = %self.config.source.name(),
            elapsed_us = elapsed.as_micros(),
            "Script executed on response"
        );

        // Apply changes back to response
        if let Some(res_ctx) = ctx.as_response() {
            res_ctx.apply_to_response(res)
                .map_err(|e| ScriptError::runtime(e))?;
        }

        result
    }
}

impl fmt::Debug for ScriptMiddleware {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScriptMiddleware")
            .field("language", &self.config.language)
            .field("script", &self.config.source.name())
            .field("on_request", &self.config.on_request)
            .field("on_response", &self.config.on_response)
            .finish()
    }
}

#[async_trait]
impl Middleware for ScriptMiddleware {
    async fn call(&self, mut req: Request<Body>, next: Next) -> Result<Response<Body>> {
        // Execute on request if enabled
        if self.config.on_request {
            match self.execute_on_request(&mut req).await {
                Ok(should_continue) => {
                    if !should_continue {
                        debug!(
                            script = %self.config.source.name(),
                            "Script short-circuited request"
                        );
                        // Script returned false - create default response
                        return Ok(Response::builder()
                            .status(200)
                            .body(Body::from(""))
                            .unwrap());
                    }
                }
                Err(e) => {
                    error!(
                        script = %self.config.source.name(),
                        error = %e,
                        "Script execution failed on request"
                    );
                    if !self.config.continue_on_error {
                        return Err(Error::Internal(format!("Script error: {}", e)));
                    }
                }
            }
        }

        // Continue to next middleware/handler
        let mut res = next.run(req).await?;

        // Execute on response if enabled
        if self.config.on_response {
            match self.execute_on_response(&mut res).await {
                Ok(_) => {
                    // Response script executed successfully
                }
                Err(e) => {
                    error!(
                        script = %self.config.source.name(),
                        error = %e,
                        "Script execution failed on response"
                    );
                    if !self.config.continue_on_error {
                        return Err(Error::Internal(format!("Script error: {}", e)));
                    }
                }
            }
        }

        Ok(res)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_script_middleware_creation() {
        let config = ScriptMiddlewareConfig::inline("true");
        let middleware = ScriptMiddleware::new(config);
        assert!(middleware.prepare().await.is_ok());
    }

    #[tokio::test]
    async fn test_script_middleware_config() {
        let config = ScriptMiddlewareConfig::inline("headers['X-Test'] = 'value'; true")
            .with_response()
            .with_timeout(200)
            .continue_on_error();

        assert!(config.on_request);
        assert!(config.on_response);
        assert!(config.continue_on_error);
        assert_eq!(config.timeout_ms, 200);
    }
}

