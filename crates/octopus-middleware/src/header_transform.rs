//! Header transformation middleware
//!
//! Add, set, remove, or rename headers on requests and responses.

use async_trait::async_trait;
use bytes::Bytes;
use http::{header::HeaderName, HeaderValue, Request, Response};
use http_body_util::Full;
use octopus_core::{Middleware, Next, Result};
use std::fmt;

/// Body type alias
pub type Body = Full<Bytes>;

/// Rules for transforming headers
#[derive(Clone, Default)]
pub struct HeaderRules {
    /// Append header (does not replace existing values)
    pub add: Vec<(String, String)>,
    /// Set/replace header (replaces if exists, adds if not)
    pub set: Vec<(String, String)>,
    /// Remove headers by name
    pub remove: Vec<String>,
    /// Rename headers (old name -> new name)
    pub rename: Vec<(String, String)>,
}

impl fmt::Debug for HeaderRules {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HeaderRules")
            .field("add", &self.add.len())
            .field("set", &self.set.len())
            .field("remove", &self.remove.len())
            .field("rename", &self.rename.len())
            .finish()
    }
}

/// Configuration for header transformation middleware
#[derive(Clone, Default)]
pub struct HeaderTransformConfig {
    /// Rules applied to the incoming request
    pub request: HeaderRules,
    /// Rules applied to the outgoing response
    pub response: HeaderRules,
}

impl fmt::Debug for HeaderTransformConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HeaderTransformConfig")
            .field("request", &self.request)
            .field("response", &self.response)
            .finish()
    }
}

/// Header transformation middleware
///
/// Applies add/set/remove/rename rules to request and response headers.
#[derive(Clone)]
pub struct HeaderTransform {
    config: HeaderTransformConfig,
}

impl HeaderTransform {
    /// Create a new HeaderTransform middleware
    pub fn new(config: HeaderTransformConfig) -> Self {
        Self { config }
    }

    /// Apply header rules to a set of headers (request or response)
    fn apply_rules(headers: &mut http::HeaderMap, rules: &HeaderRules) {
        // 1. Remove headers
        for name in &rules.remove {
            if let Ok(header_name) = name.parse::<HeaderName>() {
                headers.remove(&header_name);
            }
        }

        // 2. Rename headers (old -> new)
        for (old_name, new_name) in &rules.rename {
            if let (Ok(old), Ok(new)) = (
                old_name.parse::<HeaderName>(),
                new_name.parse::<HeaderName>(),
            ) {
                if let Some(value) = headers.remove(&old) {
                    headers.insert(new, value);
                }
            }
        }

        // 3. Set headers (replace or add)
        for (name, value) in &rules.set {
            if let (Ok(header_name), Ok(header_value)) =
                (name.parse::<HeaderName>(), HeaderValue::from_str(value))
            {
                headers.insert(header_name, header_value);
            }
        }

        // 4. Add headers (append, does not replace)
        for (name, value) in &rules.add {
            if let (Ok(header_name), Ok(header_value)) =
                (name.parse::<HeaderName>(), HeaderValue::from_str(value))
            {
                headers.append(header_name, header_value);
            }
        }
    }
}

impl fmt::Debug for HeaderTransform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HeaderTransform")
            .field("config", &self.config)
            .finish()
    }
}

impl Default for HeaderTransform {
    fn default() -> Self {
        Self {
            config: HeaderTransformConfig::default(),
        }
    }
}

#[async_trait]
impl Middleware for HeaderTransform {
    async fn call(&self, mut req: Request<Body>, next: Next) -> Result<Response<Body>> {
        // Apply request rules
        Self::apply_rules(req.headers_mut(), &self.config.request);

        // Call next middleware
        let mut response = next.run(req).await?;

        // Apply response rules
        Self::apply_rules(response.headers_mut(), &self.config.response);

        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::StatusCode;
    use octopus_core::Error;

    #[derive(Debug)]
    struct EchoHandler;

    #[async_trait]
    impl Middleware for EchoHandler {
        async fn call(&self, req: Request<Body>, _next: Next) -> Result<Response<Body>> {
            // Echo back request headers as X-Echo-* response headers
            let mut builder = Response::builder().status(StatusCode::OK);

            for (name, value) in req.headers().iter() {
                let echo_name = format!("x-echo-{}", name.as_str());
                if let Ok(hname) = echo_name.parse::<HeaderName>() {
                    builder = builder.header(hname, value.clone());
                }
            }

            builder = builder.header("x-original", "from-handler");

            builder
                .body(Full::new(Bytes::from("ok")))
                .map_err(|e| Error::Internal(e.to_string()))
        }
    }

    fn make_stack(mw: HeaderTransform) -> std::sync::Arc<[std::sync::Arc<dyn Middleware>]> {
        std::sync::Arc::new([
            std::sync::Arc::new(mw) as std::sync::Arc<dyn Middleware>,
            std::sync::Arc::new(EchoHandler),
        ])
    }

    #[tokio::test]
    async fn test_add_request_header() {
        let config = HeaderTransformConfig {
            request: HeaderRules {
                add: vec![("x-custom".to_string(), "injected".to_string())],
                ..Default::default()
            },
            ..Default::default()
        };
        let mw = HeaderTransform::new(config);
        let stack = make_stack(mw);
        let next = Next::new(stack);

        let req = Request::builder()
            .uri("/test")
            .body(Body::from(""))
            .unwrap();

        let resp = next.run(req).await.unwrap();
        // The echo handler copies request headers as x-echo-* response headers
        assert_eq!(
            resp.headers()
                .get("x-echo-x-custom")
                .unwrap()
                .to_str()
                .unwrap(),
            "injected"
        );
    }

    #[tokio::test]
    async fn test_remove_response_header() {
        let config = HeaderTransformConfig {
            response: HeaderRules {
                remove: vec!["x-original".to_string()],
                ..Default::default()
            },
            ..Default::default()
        };
        let mw = HeaderTransform::new(config);
        let stack = make_stack(mw);
        let next = Next::new(stack);

        let req = Request::builder()
            .uri("/test")
            .body(Body::from(""))
            .unwrap();

        let resp = next.run(req).await.unwrap();
        // The handler adds x-original, but our middleware should remove it
        assert!(resp.headers().get("x-original").is_none());
    }

    #[tokio::test]
    async fn test_rename_response_header() {
        let config = HeaderTransformConfig {
            response: HeaderRules {
                rename: vec![("x-original".to_string(), "x-renamed".to_string())],
                ..Default::default()
            },
            ..Default::default()
        };
        let mw = HeaderTransform::new(config);
        let stack = make_stack(mw);
        let next = Next::new(stack);

        let req = Request::builder()
            .uri("/test")
            .body(Body::from(""))
            .unwrap();

        let resp = next.run(req).await.unwrap();
        // x-original should be gone, x-renamed should have its value
        assert!(resp.headers().get("x-original").is_none());
        assert_eq!(
            resp.headers().get("x-renamed").unwrap().to_str().unwrap(),
            "from-handler"
        );
    }

    #[tokio::test]
    async fn test_set_response_header_replaces_existing() {
        let config = HeaderTransformConfig {
            response: HeaderRules {
                set: vec![("x-original".to_string(), "overridden".to_string())],
                ..Default::default()
            },
            ..Default::default()
        };
        let mw = HeaderTransform::new(config);
        let stack = make_stack(mw);
        let next = Next::new(stack);

        let req = Request::builder()
            .uri("/test")
            .body(Body::from(""))
            .unwrap();

        let resp = next.run(req).await.unwrap();
        assert_eq!(
            resp.headers().get("x-original").unwrap().to_str().unwrap(),
            "overridden"
        );
    }

    #[tokio::test]
    async fn test_add_does_not_replace() {
        let config = HeaderTransformConfig {
            response: HeaderRules {
                add: vec![("x-original".to_string(), "appended".to_string())],
                ..Default::default()
            },
            ..Default::default()
        };
        let mw = HeaderTransform::new(config);
        let stack = make_stack(mw);
        let next = Next::new(stack);

        let req = Request::builder()
            .uri("/test")
            .body(Body::from(""))
            .unwrap();

        let resp = next.run(req).await.unwrap();
        let values: Vec<&str> = resp
            .headers()
            .get_all("x-original")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .collect();
        assert!(values.contains(&"from-handler"));
        assert!(values.contains(&"appended"));
    }
}
