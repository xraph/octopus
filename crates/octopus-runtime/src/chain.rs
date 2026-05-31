//! Construction of the request-processing middleware chain from configuration.
//!
//! These middleware run *before* authentication and are assembled here (rather
//! than inline in [`crate::server`]) so the wiring can be unit-tested directly
//! against configuration.

use std::sync::Arc;
use std::time::Duration;

use octopus_config::types::{
    CompressionConfig, CorsGlobalConfig, PluginConfig, SecurityHeadersConfig,
};
use octopus_core::middleware::Middleware;

/// Build the pre-auth request middleware from configuration.
///
/// Currently: response compression, CORS (global policy; per-route overrides are
/// applied from request extensions by the CORS middleware itself), and security
/// response headers (when `security_headers.enabled`). Returned in execution
/// order (outermost first); the caller appends the auth gateway middleware after
/// these.
pub(crate) fn build_request_middleware(
    compression: &CompressionConfig,
    cors: Option<&CorsGlobalConfig>,
    security_headers: &SecurityHeadersConfig,
) -> Vec<Arc<dyn Middleware>> {
    let mut mws: Vec<Arc<dyn Middleware>> = Vec::new();

    if compression.enabled {
        let cfg = octopus_compression::CompressionConfig {
            enabled: compression.enabled,
            level: compression.level,
            min_size: compression.min_size,
            algorithms: compression.algorithms.clone(),
        };
        mws.push(Arc::new(octopus_compression::CompressionMiddleware::new(
            cfg,
        )));
    }

    if let Some(c) = cors {
        let cfg = octopus_middleware::CorsConfig {
            allowed_origins: c.allowed_origins.clone(),
            allowed_methods: c
                .allowed_methods
                .iter()
                .filter_map(|m| m.parse().ok())
                .collect(),
            allowed_headers: c.allowed_headers.clone(),
            exposed_headers: c.exposed_headers.clone(),
            max_age: Duration::from_secs(c.max_age),
            allow_credentials: c.allow_credentials,
        };
        mws.push(Arc::new(octopus_middleware::Cors::with_config(cfg)));
    }

    if security_headers.enabled {
        let cfg = octopus_middleware::SecurityHeadersConfig {
            hsts: security_headers.hsts.clone(),
            csp: security_headers.csp.clone(),
            frame_options: security_headers.frame_options.clone(),
            content_type_options: security_headers.content_type_options.clone(),
            xss_protection: security_headers.xss_protection.clone(),
            referrer_policy: security_headers.referrer_policy.clone(),
            permissions_policy: security_headers.permissions_policy.clone(),
        };
        mws.push(Arc::new(octopus_middleware::SecurityHeaders::with_config(
            cfg,
        )));
    }

    mws
}

/// Build middleware from the `plugins` config. Currently supports **script**
/// plugins (`plugin_type: "script"`): each enabled entry's `config` is
/// deserialized into a [`octopus_scripting::ScriptMiddlewareConfig`] (inline
/// `code` or file `path`, `language`, `on_request`/`on_response`, `timeout_ms`)
/// and run as a [`octopus_scripting::ScriptMiddleware`], ordered by descending
/// `priority`. Other plugin types (`static`/`dynamic`) are not yet loaded and are
/// skipped with a warning.
pub(crate) fn build_plugin_middleware(plugins: &[PluginConfig]) -> Vec<Arc<dyn Middleware>> {
    let mut enabled: Vec<&PluginConfig> = plugins.iter().filter(|p| p.enabled).collect();
    enabled.sort_by(|a, b| b.priority.cmp(&a.priority));

    let mut mws: Vec<Arc<dyn Middleware>> = Vec::new();
    for p in enabled {
        match p.plugin_type.as_str() {
            "script" => {
                let value = serde_json::Value::Object(p.config.clone().into_iter().collect());
                match serde_json::from_value::<octopus_scripting::ScriptMiddlewareConfig>(value) {
                    Ok(cfg) => {
                        mws.push(Arc::new(octopus_scripting::ScriptMiddleware::new(cfg)));
                        tracing::info!(plugin = %p.name, "Script plugin middleware loaded");
                    }
                    Err(e) => {
                        tracing::warn!(plugin = %p.name, error = %e, "Invalid script plugin config; skipping");
                    }
                }
            }
            other => {
                tracing::warn!(
                    plugin = %p.name,
                    plugin_type = %other,
                    "Plugin type not yet loaded (only 'script' is wired); skipping"
                );
            }
        }
    }
    mws
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use bytes::Bytes;
    use http::{header, Method, Request, Response, StatusCode};
    use http_body_util::Full;
    use octopus_core::middleware::{Body, Next};
    use octopus_core::Result as CoreResult;

    /// Terminal middleware that returns 200 OK.
    #[derive(Debug)]
    struct TerminalOk;
    #[async_trait]
    impl Middleware for TerminalOk {
        async fn call(&self, _req: Request<Body>, _next: Next) -> CoreResult<Response<Body>> {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(Full::new(Bytes::from("ok")))
                .unwrap())
        }
    }

    /// Terminal middleware that must never be reached (proves short-circuit).
    #[derive(Debug)]
    struct PanicTerminal;
    #[async_trait]
    impl Middleware for PanicTerminal {
        async fn call(&self, _req: Request<Body>, _next: Next) -> CoreResult<Response<Body>> {
            panic!("request reached the upstream; CORS preflight should have short-circuited");
        }
    }

    fn compression_off() -> CompressionConfig {
        CompressionConfig {
            enabled: false,
            ..Default::default()
        }
    }

    fn cors_allow_all() -> CorsGlobalConfig {
        CorsGlobalConfig {
            allowed_origins: vec!["*".to_string()],
            allowed_methods: vec!["GET".to_string(), "POST".to_string()],
            allowed_headers: vec![],
            exposed_headers: vec![],
            max_age: 600,
            allow_credentials: false,
        }
    }

    fn sh_off() -> SecurityHeadersConfig {
        SecurityHeadersConfig {
            enabled: false,
            ..Default::default()
        }
    }

    fn sh_on() -> SecurityHeadersConfig {
        SecurityHeadersConfig {
            enabled: true,
            ..Default::default()
        }
    }

    fn req_with_origin(method: Method) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri("/x")
            .header(header::ORIGIN, "https://app.example.com")
            .body(Full::new(Bytes::new()))
            .unwrap()
    }

    #[tokio::test]
    async fn global_cors_applies_allow_origin_header() {
        let cors = cors_allow_all();
        let mut mws = build_request_middleware(&compression_off(), Some(&cors), &sh_off());
        mws.push(Arc::new(TerminalOk));
        let stack: Arc<[Arc<dyn Middleware>]> = Arc::from(mws);

        let resp = Next::new(stack)
            .run(req_with_origin(Method::GET))
            .await
            .unwrap();

        assert_eq!(
            resp.headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .and_then(|v| v.to_str().ok()),
            Some("*"),
        );
    }

    #[tokio::test]
    async fn no_cors_header_without_global_config() {
        let mut mws = build_request_middleware(&compression_off(), None, &sh_off());
        mws.push(Arc::new(TerminalOk));
        let stack: Arc<[Arc<dyn Middleware>]> = Arc::from(mws);

        let resp = Next::new(stack)
            .run(req_with_origin(Method::GET))
            .await
            .unwrap();

        assert!(resp
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none());
    }

    #[tokio::test]
    async fn security_headers_added_when_enabled() {
        let mut mws = build_request_middleware(&compression_off(), None, &sh_on());
        mws.push(Arc::new(TerminalOk));
        let stack: Arc<[Arc<dyn Middleware>]> = Arc::from(mws);

        let req = Request::builder()
            .uri("/x")
            .body(Full::new(Bytes::new()))
            .unwrap();
        let resp = Next::new(stack).run(req).await.unwrap();

        assert_eq!(
            resp.headers()
                .get("x-frame-options")
                .and_then(|v| v.to_str().ok()),
            Some("DENY"),
        );
        assert!(resp.headers().contains_key("content-security-policy"));
    }

    #[tokio::test]
    async fn no_security_headers_when_disabled() {
        let mut mws = build_request_middleware(&compression_off(), None, &sh_off());
        mws.push(Arc::new(TerminalOk));
        let stack: Arc<[Arc<dyn Middleware>]> = Arc::from(mws);

        let req = Request::builder()
            .uri("/x")
            .body(Full::new(Bytes::new()))
            .unwrap();
        let resp = Next::new(stack).run(req).await.unwrap();

        assert!(resp.headers().get("x-frame-options").is_none());
    }

    #[tokio::test]
    async fn preflight_short_circuits_with_204() {
        let cors = cors_allow_all();
        let mut mws = build_request_middleware(&compression_off(), Some(&cors), &sh_off());
        mws.push(Arc::new(PanicTerminal));
        let stack: Arc<[Arc<dyn Middleware>]> = Arc::from(mws);

        let req = Request::builder()
            .method(Method::OPTIONS)
            .uri("/x")
            .header(header::ORIGIN, "https://app.example.com")
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
            .body(Full::new(Bytes::new()))
            .unwrap();

        let resp = Next::new(stack).run(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    fn script_plugin(name: &str, enabled: bool, priority: i32) -> PluginConfig {
        let mut config = std::collections::HashMap::new();
        config.insert("language".to_string(), serde_json::json!("rhai"));
        config.insert("code".to_string(), serde_json::json!("true"));
        config.insert("on_request".to_string(), serde_json::json!(true));
        PluginConfig {
            name: name.to_string(),
            plugin_type: "script".to_string(),
            enabled,
            priority,
            config,
        }
    }

    #[test]
    fn script_plugin_produces_middleware() {
        let mws = build_plugin_middleware(&[script_plugin("s", true, 0)]);
        assert_eq!(mws.len(), 1);
        assert!(format!("{:?}", mws[0]).contains("ScriptMiddleware"));
    }

    #[test]
    fn disabled_or_unknown_plugins_skipped() {
        let disabled = script_plugin("d", false, 0);
        let mut static_plugin = script_plugin("st", true, 0);
        static_plugin.plugin_type = "static".to_string();
        assert!(build_plugin_middleware(&[disabled, static_plugin]).is_empty());
    }
}
