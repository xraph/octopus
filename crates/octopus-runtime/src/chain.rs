//! Construction of the request-processing middleware chain from configuration.
//!
//! These middleware run *before* authentication and are assembled here (rather
//! than inline in [`crate::server`]) so the wiring can be unit-tested directly
//! against configuration.

use std::sync::Arc;
use std::time::Duration;

use octopus_config::types::{CompressionConfig, CorsGlobalConfig};
use octopus_core::middleware::Middleware;

/// Build the pre-auth request middleware from configuration.
///
/// Currently: response compression and CORS (global policy; per-route overrides
/// are applied from request extensions by the CORS middleware itself). Returned
/// in execution order (outermost first); the caller appends the auth gateway
/// middleware after these.
pub(crate) fn build_request_middleware(
    compression: &CompressionConfig,
    cors: Option<&CorsGlobalConfig>,
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
        let mut mws = build_request_middleware(&compression_off(), Some(&cors));
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
        let mut mws = build_request_middleware(&compression_off(), None);
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
    async fn preflight_short_circuits_with_204() {
        let cors = cors_allow_all();
        let mut mws = build_request_middleware(&compression_off(), Some(&cors));
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
}
