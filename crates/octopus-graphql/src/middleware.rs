//! `GraphQlMiddleware`: parses and policy-checks GraphQL requests, serves
//! GraphiQL, and delegates valid operations to the next handler (the proxy).

use async_trait::async_trait;
use bytes::Bytes;
use http::{header, Method, Request, Response, StatusCode};
use http_body_util::{BodyExt, Full};
use octopus_config::types::GraphQLConfig;
use octopus_core::middleware::{Body, Middleware, Next};
use octopus_core::{Error, Result};

use crate::playground::graphiql_html;
use crate::query::analyze_query;

/// GraphQL-aware gateway middleware. See module docs.
#[derive(Debug, Clone)]
pub struct GraphQlMiddleware {
    endpoint: String,
    playground: bool,
    introspection: bool,
    max_depth: usize,
    max_complexity: usize,
}

impl GraphQlMiddleware {
    /// Build the middleware from configuration.
    #[must_use]
    pub fn from_config(config: &GraphQLConfig) -> Self {
        Self {
            endpoint: config.endpoint.clone(),
            playground: config.playground,
            introspection: config.introspection,
            max_depth: config.max_depth,
            max_complexity: config.max_complexity,
        }
    }

    fn is_playground_request(req: &Request<Body>) -> bool {
        req.method() == Method::GET
            && req.uri().query().map_or(true, |q| !q.contains("query="))
            && req
                .headers()
                .get(header::ACCEPT)
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| v.contains("text/html"))
    }

    /// Extract the GraphQL query string from a POST JSON body or GET `?query=`.
    fn extract_query(method: &Method, uri: &http::Uri, body: &Bytes) -> Result<String> {
        match *method {
            Method::POST => {
                let v: serde_json::Value = serde_json::from_slice(body).map_err(|e| {
                    Error::InvalidRequest(format!("Invalid GraphQL JSON body: {e}"))
                })?;
                v.get("query")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
                    .ok_or_else(|| Error::InvalidRequest("Missing 'query' field".to_string()))
            }
            Method::GET => uri
                .query()
                .and_then(|q| {
                    url::form_urlencoded::parse(q.as_bytes())
                        .find(|(k, _)| k == "query")
                        .map(|(_, v)| v.into_owned())
                })
                .ok_or_else(|| Error::InvalidRequest("Missing 'query' parameter".to_string())),
            _ => Err(Error::InvalidRequest(format!(
                "Unsupported method for GraphQL: {method}"
            ))),
        }
    }

    fn html_response(body: String) -> Result<Response<Body>> {
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .body(Full::new(Bytes::from(body)))
            .map_err(|e| Error::Internal(format!("Failed to build playground response: {e}")))
    }

    /// A GraphQL error envelope: HTTP 200 with `{"errors":[{"message":...}]}`,
    /// the conventional shape GraphQL clients expect for request-level errors.
    fn error_response(message: &str) -> Result<Response<Body>> {
        let json = serde_json::json!({ "errors": [{ "message": message }] }).to_string();
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Full::new(Bytes::from(json)))
            .map_err(|e| Error::Internal(format!("Failed to build error response: {e}")))
    }
}

#[async_trait]
impl Middleware for GraphQlMiddleware {
    async fn call(&self, req: Request<Body>, next: Next) -> Result<Response<Body>> {
        // Only intercept the configured endpoint; everything else flows through.
        if req.uri().path() != self.endpoint {
            return next.run(req).await;
        }

        // Serve the IDE on a plain HTML GET.
        if self.playground && Self::is_playground_request(&req) {
            return Self::html_response(graphiql_html(&self.endpoint));
        }

        let (parts, body) = req.into_parts();
        let bytes = body
            .collect()
            .await
            .map_err(|e| Error::Internal(format!("Failed to read body: {e}")))?
            .to_bytes();

        let query = match Self::extract_query(&parts.method, &parts.uri, &bytes) {
            Ok(q) => q,
            Err(e) => return Self::error_response(&e.to_string()),
        };

        let analysis = match analyze_query(&query) {
            Ok(a) => a,
            Err(e) => return Self::error_response(&e.to_string()),
        };

        if !self.introspection && analysis.has_introspection {
            return Self::error_response("Introspection is disabled");
        }
        if analysis.depth > self.max_depth {
            return Self::error_response(&format!(
                "Query exceeds maximum depth of {} (got {})",
                self.max_depth, analysis.depth
            ));
        }
        if analysis.complexity > self.max_complexity {
            return Self::error_response(&format!(
                "Query exceeds maximum complexity of {} (got {})",
                self.max_complexity, analysis.complexity
            ));
        }

        tracing::debug!(
            depth = analysis.depth,
            complexity = analysis.complexity,
            "GraphQL query accepted; delegating to proxy"
        );

        // Rebuild the request with the original bytes and continue the chain.
        let req = Request::from_parts(parts, Full::new(bytes));
        next.run(req).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use octopus_core::middleware::{HandlerFn, Next};
    use std::sync::Arc;

    // A `Next` whose final handler tags the response so tests can tell whether
    // the middleware delegated (proxied) or short-circuited.
    fn delegating_next() -> Next {
        let stack: Arc<[Arc<dyn Middleware>]> = Arc::from(Vec::new());
        let final_handler: HandlerFn = Box::new(|_req| {
            Box::pin(async {
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .header("x-delegated", "1")
                    .body(Full::new(Bytes::from("upstream")))
                    .unwrap())
            })
        });
        Next::with_handler(stack, final_handler)
    }

    fn cfg() -> GraphQLConfig {
        GraphQLConfig {
            enabled: true,
            ..GraphQLConfig::default()
        }
    }

    fn post_query(path: &str, body: &str) -> Request<Body> {
        Request::builder()
            .method(Method::POST)
            .uri(path)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Full::new(Bytes::from(body.to_string())))
            .unwrap()
    }

    #[tokio::test]
    async fn non_graphql_path_passes_through() {
        let mw = GraphQlMiddleware::from_config(&cfg());
        let req = Request::builder()
            .method(Method::GET)
            .uri("/api/users")
            .body(Full::new(Bytes::new()))
            .unwrap();
        let resp = mw.call(req, delegating_next()).await.unwrap();
        assert_eq!(resp.headers().get("x-delegated").unwrap(), "1");
    }

    #[tokio::test]
    async fn valid_query_delegates_to_next() {
        let mw = GraphQlMiddleware::from_config(&cfg());
        let req = post_query("/graphql", r#"{"query":"{ user { name } }"}"#);
        let resp = mw.call(req, delegating_next()).await.unwrap();
        assert_eq!(resp.headers().get("x-delegated").unwrap(), "1");
    }

    #[tokio::test]
    async fn playground_served_on_html_get() {
        let mw = GraphQlMiddleware::from_config(&cfg());
        let req = Request::builder()
            .method(Method::GET)
            .uri("/graphql")
            .header(header::ACCEPT, "text/html")
            .body(Full::new(Bytes::new()))
            .unwrap();
        let resp = mw.call(req, delegating_next()).await.unwrap();
        assert!(resp.headers().get("x-delegated").is_none());
        assert_eq!(
            resp.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/html; charset=utf-8"
        );
    }

    #[tokio::test]
    async fn over_depth_query_is_rejected() {
        let mut c = cfg();
        c.max_depth = 2;
        let mw = GraphQlMiddleware::from_config(&c);
        let req = post_query("/graphql", r#"{"query":"{ a { b { c { d } } } }"}"#);
        let resp = mw.call(req, delegating_next()).await.unwrap();
        assert!(resp.headers().get("x-delegated").is_none());
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert!(String::from_utf8_lossy(&body).contains("depth"));
    }

    #[tokio::test]
    async fn introspection_rejected_when_disabled() {
        let mut c = cfg();
        c.introspection = false;
        let mw = GraphQlMiddleware::from_config(&c);
        let req = post_query("/graphql", r#"{"query":"{ __schema { types { name } } }"}"#);
        let resp = mw.call(req, delegating_next()).await.unwrap();
        assert!(resp.headers().get("x-delegated").is_none());
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert!(String::from_utf8_lossy(&body)
            .to_lowercase()
            .contains("introspection"));
    }

    #[tokio::test]
    async fn malformed_json_body_is_rejected() {
        let mw = GraphQlMiddleware::from_config(&cfg());
        let req = post_query("/graphql", "not json");
        let resp = mw.call(req, delegating_next()).await.unwrap();
        assert!(resp.headers().get("x-delegated").is_none());
    }
}
