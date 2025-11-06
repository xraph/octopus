//! Rhai script engine implementation

use crate::context::{RequestContext, ResponseContext, ScriptContext};
use crate::engine::{CacheStats, ScriptEngine, ScriptLanguage, ScriptSource};
use crate::error::{Result, ScriptError};
use async_trait::async_trait;
use rhai::{Dynamic, Engine, AST, Scope};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, trace, warn};

/// Rhai script engine with AST caching
#[derive(Debug)]
pub struct RhaiEngine {
    /// Rhai engine instance
    engine: Engine,
    /// Compiled AST cache (script name -> AST)
    ast_cache: Arc<RwLock<HashMap<String, AST>>>,
    /// Cache statistics
    cache_hits: Arc<AtomicU64>,
    cache_misses: Arc<AtomicU64>,
}

impl RhaiEngine {
    /// Create new Rhai engine with default configuration
    pub fn new() -> Self {
        let mut engine = Engine::new();

        // Configure engine for safety and performance
        engine.set_max_expr_depths(25, 10); // Reasonable depth limits
        engine.set_max_operations(10_000); // Prevent infinite loops
        engine.set_max_string_size(1024 * 1024); // 1MB string limit
        engine.set_max_array_size(10_000); // Array size limit
        engine.set_max_map_size(10_000); // Map size limit

        // Register custom functions
        Self::register_functions(&mut engine);

        Self {
            engine,
            ast_cache: Arc::new(RwLock::new(HashMap::new())),
            cache_hits: Arc::new(AtomicU64::new(0)),
            cache_misses: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Create engine with custom configuration
    pub fn with_config(max_operations: u64, max_string_size: usize) -> Self {
        let mut engine = Engine::new();
        engine.set_max_operations(max_operations);
        engine.set_max_string_size(max_string_size);
        Self::register_functions(&mut engine);

        Self {
            engine,
            ast_cache: Arc::new(RwLock::new(HashMap::new())),
            cache_hits: Arc::new(AtomicU64::new(0)),
            cache_misses: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Register custom functions for request/response manipulation
    fn register_functions(engine: &mut Engine) {
        // JSON parsing/serialization - simplified without rhai::serde
        engine.register_fn("parse_json", |s: &str| -> String {
            // Return JSON string as-is for script manipulation
            // In Rhai, scripts can work with strings directly
            s.to_string()
        });

        engine.register_fn("to_json", |value: String| -> String {
            // Return string as-is (already JSON format from script)
            value
        });

        // String utilities
        engine.register_fn("base64_encode", |s: &str| -> String {
            use base64::{engine::general_purpose, Engine as _};
            general_purpose::STANDARD.encode(s.as_bytes())
        });

        engine.register_fn("base64_decode", |s: &str| -> String {
            use base64::{engine::general_purpose, Engine as _};
            general_purpose::STANDARD
                .decode(s.as_bytes())
                .ok()
                .and_then(|bytes| String::from_utf8(bytes).ok())
                .unwrap_or_default()
        });

        // Utility functions
        engine.register_fn("unix_time", || -> i64 {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64
        });

        engine.register_fn("uuid", || -> String {
            uuid::Uuid::new_v4().to_string()
        });

        // Logging (for debugging scripts)
        engine.register_fn("log_debug", |msg: &str| {
            debug!(script_log = msg);
        });

        engine.register_fn("log_info", |msg: &str| {
            tracing::info!(script_log = msg);
        });

        engine.register_fn("log_warn", |msg: &str| {
            warn!(script_log = msg);
        });
    }

    /// Get or compile AST
    async fn get_ast(&self, source: &ScriptSource) -> Result<AST> {
        let name = source.name();
        let code = source.get_code().await?;

        // Check cache
        {
            let cache = self.ast_cache.read().await;
            if let Some(ast) = cache.get(&name) {
                self.cache_hits.fetch_add(1, Ordering::Relaxed);
                trace!(script = %name, "AST cache hit");
                return Ok(ast.clone());
            }
        }

        // Cache miss - compile
        self.cache_misses.fetch_add(1, Ordering::Relaxed);
        trace!(script = %name, "AST cache miss, compiling");

        let ast = self.engine.compile(&code).map_err(|e| {
            ScriptError::CompilationError {
                message: e.to_string(),
                line: None,
                column: None,
            }
        })?;

        // Store in cache
        {
            let mut cache = self.ast_cache.write().await;
            cache.insert(name.clone(), ast.clone());
        }

        debug!(script = %name, "Script compiled and cached");
        Ok(ast)
    }

    /// Execute script with request context
    async fn execute_with_request(
        &self,
        ast: &AST,
        ctx: &mut RequestContext,
    ) -> Result<bool> {
        let mut scope = Scope::new();

        // Convert context to Rhai map
        scope.push("method", ctx.method.clone());
        scope.push("uri", ctx.uri.clone());
        scope.push("version", ctx.version.clone());
        
        // Headers as map
        let headers_map: rhai::Map = ctx.headers.iter()
            .map(|(k, v)| (k.clone().into(), Dynamic::from(v.clone())))
            .collect();
        scope.push("headers", headers_map);

        // Query params as map
        let query_map: rhai::Map = ctx.query.iter()
            .map(|(k, v)| (k.clone().into(), Dynamic::from(v.clone())))
            .collect();
        scope.push("query", query_map);

        // Path params as map
        let path_params_map: rhai::Map = ctx.path_params.iter()
            .map(|(k, v)| (k.clone().into(), Dynamic::from(v.clone())))
            .collect();
        scope.push("path_params", path_params_map);

        // Body as string if available
        if let Some(body_str) = ctx.body_string() {
            scope.push("body", body_str);
        }

        // Execute script
        let result: Dynamic = self.engine.eval_ast_with_scope(&mut scope, ast)
            .map_err(|e| ScriptError::RuntimeError {
                message: e.to_string(),
                line: None,
            })?;

        // Extract modified values back to context
        if let Some(method) = scope.get_value::<String>("method") {
            ctx.method = method;
        }
        if let Some(uri) = scope.get_value::<String>("uri") {
            ctx.uri = uri;
        }
        
        // Extract headers
        if let Some(headers) = scope.get_value::<rhai::Map>("headers") {
            ctx.headers = headers.iter()
                .filter_map(|(k, v)| {
                    Some((k.to_string(), v.clone().try_cast::<String>()?))
                })
                .collect();
        }

        // Extract body if modified
        if let Some(body) = scope.get_value::<String>("body") {
            ctx.set_body_string(body);
        }

        // Check return value - false means short-circuit
        if let Some(should_continue) = result.try_cast::<bool>() {
            Ok(should_continue)
        } else {
            Ok(true) // Default: continue
        }
    }

    /// Execute script with response context
    async fn execute_with_response(
        &self,
        ast: &AST,
        ctx: &mut ResponseContext,
    ) -> Result<bool> {
        let mut scope = Scope::new();

        // Convert context to Rhai map
        scope.push("status", ctx.status as i64);
        
        // Headers as map
        let headers_map: rhai::Map = ctx.headers.iter()
            .map(|(k, v)| (k.clone().into(), Dynamic::from(v.clone())))
            .collect();
        scope.push("headers", headers_map);

        // Body as string if available
        if let Some(body_str) = ctx.body_string() {
            scope.push("body", body_str);
        }

        // Execute script
        let result: Dynamic = self.engine.eval_ast_with_scope(&mut scope, ast)
            .map_err(|e| ScriptError::RuntimeError {
                message: e.to_string(),
                line: None,
            })?;

        // Extract modified values back to context
        if let Some(status) = scope.get_value::<i64>("status") {
            ctx.status = status as u16;
        }
        
        // Extract headers
        if let Some(headers) = scope.get_value::<rhai::Map>("headers") {
            ctx.headers = headers.iter()
                .filter_map(|(k, v)| {
                    Some((k.to_string(), v.clone().try_cast::<String>()?))
                })
                .collect();
        }

        // Extract body if modified
        if let Some(body) = scope.get_value::<String>("body") {
            ctx.set_body_string(body);
        }

        // Check return value - false means don't continue
        if let Some(should_continue) = result.try_cast::<bool>() {
            Ok(should_continue)
        } else {
            Ok(true) // Default: continue
        }
    }
}

impl Default for RhaiEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ScriptEngine for RhaiEngine {
    fn language(&self) -> ScriptLanguage {
        ScriptLanguage::Rhai
    }

    async fn prepare(&self, source: &ScriptSource) -> Result<()> {
        // Pre-compile and cache AST
        self.get_ast(source).await?;
        Ok(())
    }

    async fn execute_request(
        &self,
        source: &ScriptSource,
        ctx: &mut ScriptContext,
    ) -> Result<bool> {
        let ast = self.get_ast(source).await?;
        
        if let Some(req_ctx) = ctx.as_request_mut() {
            self.execute_with_request(&ast, req_ctx).await
        } else {
            Err(ScriptError::runtime("Expected request context"))
        }
    }

    async fn execute_response(
        &self,
        source: &ScriptSource,
        ctx: &mut ScriptContext,
    ) -> Result<bool> {
        let ast = self.get_ast(source).await?;
        
        if let Some(res_ctx) = ctx.as_response_mut() {
            self.execute_with_response(&ast, res_ctx).await
        } else {
            Err(ScriptError::runtime("Expected response context"))
        }
    }

    async fn clear_cache(&self) -> Result<()> {
        let mut cache = self.ast_cache.write().await;
        cache.clear();
        debug!("Rhai AST cache cleared");
        Ok(())
    }

    fn cache_stats(&self) -> CacheStats {
        let cache = futures::executor::block_on(self.ast_cache.read());
        CacheStats {
            cached_scripts: cache.len(),
            hits: self.cache_hits.load(Ordering::Relaxed),
            misses: self.cache_misses.load(Ordering::Relaxed),
            size_bytes: cache.len() * 1024, // Rough estimate
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_rhai_engine_basic() {
        let engine = RhaiEngine::new();
        let source = ScriptSource::inline("let x = 1 + 1; x");
        
        // Just test compilation
        assert!(engine.prepare(&source).await.is_ok());
    }

    #[tokio::test]
    async fn test_rhai_request_modification() {
        let engine = RhaiEngine::new();
        let script = r#"
            headers["X-Custom"] = "test";
            method = "POST";
            true
        "#;
        let source = ScriptSource::inline(script);

        let mut ctx = RequestContext {
            method: "GET".to_string(),
            uri: "/test".to_string(),
            version: "HTTP/1.1".to_string(),
            headers: HashMap::new(),
            body: None,
            query: HashMap::new(),
            path_params: HashMap::new(),
            metadata: HashMap::new(),
        };

        let mut script_ctx = ScriptContext::Request(ctx.clone());
        let result = engine.execute_request(&source, &mut script_ctx).await;
        
        assert!(result.is_ok());
        assert!(result.unwrap());
        
        if let ScriptContext::Request(modified) = script_ctx {
            assert_eq!(modified.method, "POST");
            assert_eq!(modified.headers.get("X-Custom"), Some(&"test".to_string()));
        }
    }
}

