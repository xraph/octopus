//! GraphQL protocol handler with federation support

use crate::handler::{ProtocolHandler, ProtocolType};
use async_trait::async_trait;
use bytes::Bytes;
use http::{header, Method, Request, Response, StatusCode};
use http_body_util::{BodyExt, Full};
use octopus_core::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use tracing::debug;

/// GraphQL protocol handler
#[derive(Debug, Clone)]
pub struct GraphQLHandler {
    /// GraphQL endpoint path
    pub endpoint: String,

    /// Enable GraphQL Playground/IDE
    pub enable_playground: bool,

    /// Federated services
    #[allow(dead_code)]
    services: HashMap<String, String>,

    /// Maximum query depth
    pub max_depth: usize,

    /// Enable introspection
    pub enable_introspection: bool,
}

/// GraphQL request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphQLRequest {
    /// GraphQL query
    pub query: String,

    /// Operation name (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_name: Option<String>,

    /// Variables (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variables: Option<Value>,
}

/// GraphQL response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphQLResponse {
    /// Response data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,

    /// Errors (if any)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub errors: Option<Vec<GraphQLError>>,

    /// Extensions (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Value>,
}

/// GraphQL error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphQLError {
    /// Error message
    pub message: String,

    /// Error locations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locations: Option<Vec<ErrorLocation>>,

    /// Error path
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<Vec<Value>>,

    /// Extensions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Value>,
}

/// Error location
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorLocation {
    /// Line number
    pub line: usize,

    /// Column number
    pub column: usize,
}

impl Default for GraphQLHandler {
    fn default() -> Self {
        Self::new("/graphql")
    }
}

impl GraphQLHandler {
    /// Create a new GraphQL handler
    #[must_use] pub fn new(endpoint: &str) -> Self {
        Self {
            endpoint: endpoint.to_string(),
            enable_playground: true,
            services: HashMap::new(),
            max_depth: 15,
            enable_introspection: true,
        }
    }

    /// Create GraphQL handler with custom configuration
    #[must_use] pub const fn with_config(
        endpoint: String,
        enable_playground: bool,
        services: HashMap<String, String>,
        max_depth: usize,
    ) -> Self {
        Self {
            endpoint,
            enable_playground,
            services,
            max_depth,
            enable_introspection: true,
        }
    }

    /// Check if request is a GraphQL request
    pub fn is_graphql_request(req: &Request<Full<Bytes>>, endpoint: &str) -> bool {
        let path = req.uri().path();
        path == endpoint || path.starts_with(&format!("{endpoint}/"))
    }

    /// Check if request is asking for GraphQL Playground
    pub fn is_playground_request(req: &Request<Full<Bytes>>) -> bool {
        req.method() == Method::GET
            && req
                .headers()
                .get(header::ACCEPT)
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| v.contains("text/html"))
    }

    /// Parse GraphQL request from HTTP request
    pub async fn parse_request(req: Request<Full<Bytes>>) -> Result<GraphQLRequest> {
        match *req.method() {
            Method::POST => {
                let body_bytes = req
                    .into_body()
                    .collect()
                    .await
                    .map_err(|e| Error::InvalidRequest(format!("Failed to read body: {e}")))?
                    .to_bytes();

                serde_json::from_slice(&body_bytes).map_err(|e| {
                    Error::InvalidRequest(format!("Failed to parse GraphQL request: {e}"))
                })
            }
            Method::GET => {
                // Parse query from URL parameters
                let query = req
                    .uri()
                    .query()
                    .and_then(|q| {
                        url::form_urlencoded::parse(q.as_bytes())
                            .find(|(key, _)| key == "query")
                            .map(|(_, value)| value.to_string())
                    })
                    .ok_or_else(|| Error::InvalidRequest("Missing query parameter".to_string()))?;

                Ok(GraphQLRequest {
                    query,
                    operation_name: None,
                    variables: None,
                })
            }
            _ => Err(Error::InvalidRequest(format!(
                "Unsupported HTTP method for GraphQL: {}",
                req.method()
            ))),
        }
    }

    /// Build GraphQL Playground HTML
    #[must_use] pub fn playground_html() -> String {
        r#"
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>GraphQL Playground</title>
  <style>
    body {
      height: 100vh;
      margin: 0;
      overflow: hidden;
      font-family: 'Segoe UI', Tahoma, Geneva, Verdana, sans-serif;
    }
    #root {
      height: 100%;
    }
  </style>
</head>
<body>
  <div id="root">
    <div style="display: flex; height: 100vh; align-items: center; justify-content: center;">
      <h1>GraphQL Endpoint</h1>
    </div>
  </div>
  <script>
    // Placeholder for GraphQL Playground
    // In production, integrate GraphQL Playground or GraphiQL
  </script>
</body>
</html>
"#
        .to_string()
    }

    /// Build error response
    #[must_use] pub fn error_response(message: &str) -> GraphQLResponse {
        GraphQLResponse {
            data: None,
            errors: Some(vec![GraphQLError {
                message: message.to_string(),
                locations: None,
                path: None,
                extensions: None,
            }]),
            extensions: None,
        }
    }
}

#[async_trait]
impl ProtocolHandler for GraphQLHandler {
    fn protocol_type(&self) -> ProtocolType {
        ProtocolType::GraphQL
    }

    fn can_handle(&self, req: &Request<Full<Bytes>>) -> bool {
        Self::is_graphql_request(req, &self.endpoint)
    }

    async fn handle(&self, req: Request<Full<Bytes>>) -> Result<Response<Full<Bytes>>> {
        // Check if this is a request for GraphQL Playground
        if self.enable_playground && Self::is_playground_request(&req) {
            let html = Self::playground_html();
            return Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "text/html")
                .body(Full::new(Bytes::from(html)))
                .map_err(|e| {
                    Error::Internal(format!("Failed to build playground response: {e}"))
                });
        }

        // Parse GraphQL request
        let gql_req = Self::parse_request(req).await?;

        debug!(
            query = %gql_req.query,
            operation = ?gql_req.operation_name,
            "Handling GraphQL request"
        );

        // Check for introspection query
        if (gql_req.query.trim().starts_with("query IntrospectionQuery")
            || gql_req.query.contains("__schema"))
            && !self.enable_introspection {
                let error_response = Self::error_response("Introspection is disabled");
                let json = serde_json::to_string(&error_response)
                    .map_err(|e| Error::Internal(format!("Failed to serialize response: {e}")))?;

                return Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Full::new(Bytes::from(json)))
                    .map_err(|e| Error::Internal(format!("Failed to build response: {e}")));
            }

        // In a real implementation, this would:
        // 1. Parse and validate the GraphQL query
        // 2. Execute query against federated services
        // 3. Merge results from multiple services
        // 4. Handle subscriptions via WebSocket
        // 5. Apply query depth limits
        // 6. Handle batched queries

        // For now, return a placeholder success response
        let response = GraphQLResponse {
            data: Some(serde_json::json!({
                "message": "GraphQL endpoint is configured"
            })),
            errors: None,
            extensions: Some(serde_json::json!({
                "tracing": {
                    "version": 1,
                    "startTime": chrono::Utc::now().to_rfc3339(),
                }
            })),
        };

        let json = serde_json::to_string(&response)
            .map_err(|e| Error::Internal(format!("Failed to serialize response: {e}")))?;

        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Full::new(Bytes::from(json)))
            .map_err(|e| Error::Internal(format!("Failed to build response: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_graphql_handler_creation() {
        let handler = GraphQLHandler::new("/graphql");
        assert_eq!(handler.protocol_type(), ProtocolType::GraphQL);
        assert_eq!(handler.endpoint, "/graphql");
    }

    #[test]
    fn test_is_graphql_request() {
        let gql_req = Request::builder()
            .method(Method::POST)
            .uri("/graphql")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Full::new(Bytes::new()))
            .unwrap();

        assert!(GraphQLHandler::is_graphql_request(&gql_req, "/graphql"));

        let http_req = Request::builder()
            .method(Method::GET)
            .uri("/api/users")
            .body(Full::new(Bytes::new()))
            .unwrap();

        assert!(!GraphQLHandler::is_graphql_request(&http_req, "/graphql"));
    }

    #[test]
    fn test_is_playground_request() {
        let playground_req = Request::builder()
            .method(Method::GET)
            .uri("/graphql")
            .header(header::ACCEPT, "text/html")
            .body(Full::new(Bytes::new()))
            .unwrap();

        assert!(GraphQLHandler::is_playground_request(&playground_req));

        let api_req = Request::builder()
            .method(Method::POST)
            .uri("/graphql")
            .header(header::ACCEPT, "application/json")
            .body(Full::new(Bytes::new()))
            .unwrap();

        assert!(!GraphQLHandler::is_playground_request(&api_req));
    }

    #[tokio::test]
    async fn test_graphql_handler_can_handle() {
        let handler = GraphQLHandler::new("/graphql");

        let gql_req = Request::builder()
            .uri("/graphql")
            .body(Full::new(Bytes::new()))
            .unwrap();

        assert!(handler.can_handle(&gql_req));

        let other_req = Request::builder()
            .uri("/api")
            .body(Full::new(Bytes::new()))
            .unwrap();

        assert!(!handler.can_handle(&other_req));
    }

    #[test]
    fn test_error_response() {
        let response = GraphQLHandler::error_response("Test error");
        assert!(response.data.is_none());
        assert!(response.errors.is_some());
        assert_eq!(response.errors.unwrap()[0].message, "Test error");
    }
}
