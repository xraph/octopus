//! FARP Registration API for service registration

use crate::federation::SchemaFederation;
use crate::manifest::SchemaManifest;
use crate::registry::SchemaRegistry;
use crate::route_generator::{RouteGenerator, GeneratedRoute};
use crate::schema::{SchemaDescriptor, SchemaFormat};
use bytes::Bytes;
use http::{Method, Request, Response, StatusCode};
use http_body_util::{BodyExt, Full};
use octopus_core::{Error, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{debug, info};

/// FARP API handler
#[derive(Clone)]
#[derive(Debug)]
pub struct FarpApiHandler {
    registry: Arc<SchemaRegistry>,
    route_generator: Arc<RouteGenerator>,
    federation: Arc<SchemaFederation>,
}

/// Registration request
#[derive(Debug, Serialize, Deserialize)]
pub struct RegistrationRequest {
    /// Service manifest
    pub manifest: SchemaManifest,
    
    /// Optional schemas (if not using location strategy)
    pub schemas: Option<Vec<SchemaDescriptor>>,
}

/// Registration response
#[derive(Debug, Serialize, Deserialize)]
pub struct RegistrationResponse {
    /// Success status
    pub success: bool,
    
    /// Message
    pub message: String,
    
    /// Generated routes
    pub routes: Vec<GeneratedRoute>,
}

/// Service list response
#[derive(Debug, Serialize, Deserialize)]
pub struct ServiceListResponse {
    /// List of services
    pub services: Vec<ServiceInfo>,
}

/// Service info
#[derive(Debug, Serialize, Deserialize)]
pub struct ServiceInfo {
    /// Service name
    pub name: String,
    
    /// Service version
    pub version: String,
    
    /// Number of schemas
    pub schema_count: usize,
    
    /// Last updated
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

impl FarpApiHandler {
    /// Create a new FARP API handler
    pub fn new(registry: Arc<SchemaRegistry>) -> Self {
        Self {
            registry,
            route_generator: Arc::new(RouteGenerator::new()),
            federation: Arc::new(SchemaFederation::new()),
        }
    }
    
    /// Create a new FARP API handler with custom federation
    pub fn with_federation(
        registry: Arc<SchemaRegistry>,
        federation: Arc<SchemaFederation>,
    ) -> Self {
        Self {
            registry,
            route_generator: Arc::new(RouteGenerator::new()),
            federation,
        }
    }

    /// Handle FARP API requests
    pub async fn handle(
        &self,
        req: Request<Full<Bytes>>,
    ) -> Result<Response<Full<Bytes>>> {
        let method = req.method().clone();
        let path = req.uri().path().to_string();

        debug!(method = %method, path = %path, "FARP API request");

        match (method, path.as_str()) {
            // Service registration
            (Method::POST, "/farp/register") => self.register_service(req).await,
            (Method::PUT, p) if p.starts_with("/farp/services/") => {
                let service_name = p.trim_start_matches("/farp/services/");
                self.update_service(service_name, req).await
            }
            (Method::DELETE, p) if p.starts_with("/farp/services/") => {
                let service_name = p.trim_start_matches("/farp/services/");
                self.deregister_service(service_name).await
            }
            (Method::GET, "/farp/services") => self.list_services().await,
            (Method::GET, p) if p.starts_with("/farp/services/") => {
                let service_name = p.trim_start_matches("/farp/services/");
                self.get_service(service_name).await
            }
            
            // Federated schema endpoints (under /farp/ prefix)
            (Method::GET, "/farp/openapi.json") => self.get_federated_openapi().await,
            (Method::GET, "/farp/asyncapi.json") => self.get_federated_asyncapi().await,
            (Method::GET, "/farp/graphql") => self.get_federated_graphql().await,
            (Method::GET, "/farp/grpc") => self.get_federated_grpc().await,
            (Method::GET, "/farp/schemas") => self.list_federated_schemas().await,
            (Method::GET, p) if p.starts_with("/farp/schema/") => {
                let format_str = p.trim_start_matches("/farp/schema/");
                self.get_federated_schema(format_str).await
            }
            (Method::POST, "/farp/federate") => self.trigger_federation().await,
            
            // Documentation UIs (under /farp/ prefix)
            (Method::GET, "/farp/docs") => self.swagger_ui().await,
            (Method::GET, "/farp/redoc") => self.redoc_ui().await,
            
            // Legacy root paths for backwards compatibility
            (Method::GET, "/openapi.json") => self.get_federated_openapi().await,
            (Method::GET, "/asyncapi.json") => self.get_federated_asyncapi().await,
            (Method::GET, "/graphql") => self.get_federated_graphql().await,
            (Method::GET, "/grpc") => self.get_federated_grpc().await,
            (Method::GET, "/docs") => self.swagger_ui().await,
            (Method::GET, "/redoc") => self.redoc_ui().await,
            
            _ => self.not_found(),
        }
    }

    /// Register a service
    async fn register_service(
        &self,
        req: Request<Full<Bytes>>,
    ) -> Result<Response<Full<Bytes>>> {
        let body_bytes = req.into_body().collect().await
            .map_err(|e| Error::Farp(format!("Failed to read request body: {}", e)))?
            .to_bytes();
        let registration: RegistrationRequest = serde_json::from_slice(&body_bytes)
            .map_err(|e| Error::Farp(format!("Invalid registration request: {}", e)))?;

        let service_name = registration.manifest.service_name.clone();
        
        info!(service = %service_name, "Registering service");

        // Register with registry
        self.registry.register_service(registration.manifest.clone())?;

        // Store schemas and trigger federation
        if let Some(schemas) = &registration.schemas {
            // Add schemas to registry for federation
            for schema in schemas {
                if let Ok(mut reg) = self.registry.get_service(&service_name) {
                    reg.schemas.push(schema.clone());
                }
            }
            
            // Trigger federation automatically
            let all_service_schemas: Vec<_> = self.registry
                .list_services()
                .iter()
                .filter_map(|s| self.registry.get_service(s).ok())
                .flat_map(|r| r.schemas)
                .collect();
            
            if !all_service_schemas.is_empty() {
                let _ = self.federation.federate_schemas(all_service_schemas);
            }
        }

        // Generate routes from schemas
        let mut all_routes = Vec::new();
        if let Some(schemas) = registration.schemas {
            for schema in &schemas {
                let routes = self.route_generator.generate_routes(schema)?;
                all_routes.extend(routes);
            }
        }

        let response = RegistrationResponse {
            success: true,
            message: format!("Service '{}' registered successfully", service_name),
            routes: all_routes,
        };

        self.json_response(StatusCode::CREATED, &response)
    }

    /// Update a service
    async fn update_service(
        &self,
        service_name: &str,
        req: Request<Full<Bytes>>,
    ) -> Result<Response<Full<Bytes>>> {
        let body_bytes = req.into_body().collect().await
            .map_err(|e| Error::Farp(format!("Failed to read request body: {}", e)))?
            .to_bytes();
        let registration: RegistrationRequest = serde_json::from_slice(&body_bytes)
            .map_err(|e| Error::Farp(format!("Invalid registration request: {}", e)))?;

        info!(service = %service_name, "Updating service");

        self.registry.update_service(registration.manifest)?;

        let response = serde_json::json!({
            "success": true,
            "message": format!("Service '{}' updated successfully", service_name)
        });

        self.json_response(StatusCode::OK, &response)
    }

    /// Deregister a service
    async fn deregister_service(&self, service_name: &str) -> Result<Response<Full<Bytes>>> {
        info!(service = %service_name, "Deregistering service");

        self.registry.deregister_service(service_name)?;

        let response = serde_json::json!({
            "success": true,
            "message": format!("Service '{}' deregistered successfully", service_name)
        });

        self.json_response(StatusCode::OK, &response)
    }

    /// List all services
    async fn list_services(&self) -> Result<Response<Full<Bytes>>> {
        let service_names = self.registry.list_services();

        let mut services = Vec::new();
        for name in service_names {
            if let Ok(reg) = self.registry.get_service(&name) {
                services.push(ServiceInfo {
                    name: reg.service_name.clone(),
                    version: reg.manifest.service_version.clone(),
                    schema_count: reg.manifest.schemas.len(),  // ← Fixed: read from manifest
                    updated_at: reg
                        .updated_at
                        .duration_since(std::time::UNIX_EPOCH)
                        .ok()
                        .map(|d| d.as_secs().to_string()),
                });
            }
        }

        let response = ServiceListResponse { services };

        self.json_response(StatusCode::OK, &response)
    }

    /// Get a specific service
    async fn get_service(&self, service_name: &str) -> Result<Response<Full<Bytes>>> {
        let registration = self.registry.get_service(service_name)?;

        let response = serde_json::json!({
            "service_name": registration.service_name,
            "manifest": registration.manifest,
            "schemas": registration.schemas,
            "registered_at": registration.registered_at
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map(|d| d.as_secs()),
            "updated_at": registration.updated_at
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map(|d| d.as_secs()),
        });

        self.json_response(StatusCode::OK, &response)
    }

    /// Get federated OpenAPI schema
    async fn get_federated_openapi(&self) -> Result<Response<Full<Bytes>>> {
        info!("Serving federated OpenAPI schema");
        
        match self.federation.get_federated(&SchemaFormat::OpenApi) {
            Ok(schema) => {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Full::new(Bytes::from(schema.content)))
                    .map_err(|e| Error::Internal(format!("Failed to build response: {}", e)))
            }
            Err(_) => {
                // Return empty OpenAPI schema if none available
                let empty_schema = serde_json::json!({
                    "openapi": "3.0.0",
                    "info": {
                        "title": "Federated API (No Services)",
                        "version": "1.0.0",
                        "description": "No services have been registered yet"
                    },
                    "paths": {}
                });
                
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Full::new(Bytes::from(serde_json::to_string_pretty(&empty_schema).unwrap())))
                    .map_err(|e| Error::Internal(format!("Failed to build response: {}", e)))
            }
        }
    }

    /// Get federated AsyncAPI schema
    async fn get_federated_asyncapi(&self) -> Result<Response<Full<Bytes>>> {
        info!("Serving federated AsyncAPI schema");
        
        match self.federation.get_federated(&SchemaFormat::AsyncApi) {
            Ok(schema) => {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Full::new(Bytes::from(schema.content)))
                    .map_err(|e| Error::Internal(format!("Failed to build response: {}", e)))
            }
            Err(_) => {
                let empty_schema = serde_json::json!({
                    "asyncapi": "2.6.0",
                    "info": {
                        "title": "Federated Async API (No Services)",
                        "version": "1.0.0"
                    },
                    "channels": {}
                });
                
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Full::new(Bytes::from(serde_json::to_string_pretty(&empty_schema).unwrap())))
                    .map_err(|e| Error::Internal(format!("Failed to build response: {}", e)))
            }
        }
    }

    /// Get federated GraphQL schema
    async fn get_federated_graphql(&self) -> Result<Response<Full<Bytes>>> {
        info!("Serving federated GraphQL schema");
        
        match self.federation.get_federated(&SchemaFormat::GraphQL) {
            Ok(schema) => {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/plain; charset=utf-8")
                    .body(Full::new(Bytes::from(schema.content)))
                    .map_err(|e| Error::Internal(format!("Failed to build response: {}", e)))
            }
            Err(_) => {
                let empty_schema = "# Federated GraphQL Schema (No Services)\ntype Query { _empty: String }";
                
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/plain; charset=utf-8")
                    .body(Full::new(Bytes::from(empty_schema)))
                    .map_err(|e| Error::Internal(format!("Failed to build response: {}", e)))
            }
        }
    }

    /// Get federated gRPC schema (protobuf)
    async fn get_federated_grpc(&self) -> Result<Response<Full<Bytes>>> {
        info!("Serving federated gRPC schema");
        
        match self.federation.get_federated(&SchemaFormat::Grpc) {
            Ok(schema) => {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/plain; charset=utf-8")
                    .body(Full::new(Bytes::from(schema.content)))
                    .map_err(|e| Error::Internal(format!("Failed to build response: {}", e)))
            }
            Err(_) => {
                let empty_schema = "// Federated gRPC Schema (No Services)\nsyntax = \"proto3\";";
                
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/plain; charset=utf-8")
                    .body(Full::new(Bytes::from(empty_schema)))
                    .map_err(|e| Error::Internal(format!("Failed to build response: {}", e)))
            }
        }
    }

    /// List all available federated schemas
    async fn list_federated_schemas(&self) -> Result<Response<Full<Bytes>>> {
        let formats = self.federation.list_formats();
        
        let schemas: Vec<_> = formats
            .iter()
            .filter_map(|format| {
                self.federation.get_federated(format).ok().map(|schema| {
                    serde_json::json!({
                        "format": format.to_string(),
                        "sources": schema.sources,
                        "updated_at": schema.updated_at
                            .duration_since(std::time::UNIX_EPOCH)
                            .ok()
                            .map(|d| d.as_secs()),
                    })
                })
            })
            .collect();
        
        let response = serde_json::json!({
            "schemas": schemas
        });
        
        self.json_response(StatusCode::OK, &response)
    }

    /// Get federated schema by format
    async fn get_federated_schema(&self, format_str: &str) -> Result<Response<Full<Bytes>>> {
        let format = match format_str.to_lowercase().as_str() {
            "openapi" => SchemaFormat::OpenApi,
            "asyncapi" => SchemaFormat::AsyncApi,
            "graphql" => SchemaFormat::GraphQL,
            "grpc" => SchemaFormat::Grpc,
            _ => {
                return self.json_response(
                    StatusCode::BAD_REQUEST,
                    &serde_json::json!({
                        "error": "invalid_format",
                        "message": format!("Unknown format: {}", format_str)
                    }),
                );
            }
        };
        
        match self.federation.get_federated(&format) {
            Ok(schema) => {
                let content_type = match format {
                    SchemaFormat::OpenApi | SchemaFormat::AsyncApi => "application/json",
                    _ => "text/plain; charset=utf-8",
                };
                
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", content_type)
                    .body(Full::new(Bytes::from(schema.content)))
                    .map_err(|e| Error::Internal(format!("Failed to build response: {}", e)))
            }
            Err(e) => self.json_response(
                StatusCode::NOT_FOUND,
                &serde_json::json!({
                    "error": "not_found",
                    "message": e.to_string()
                }),
            ),
        }
    }

    /// Serve Swagger UI
    async fn swagger_ui(&self) -> Result<Response<Full<Bytes>>> {
        let html = r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <title>FARP API Documentation - Octopus Gateway</title>
    <link rel="stylesheet" type="text/css" href="https://unpkg.com/swagger-ui-dist@5/swagger-ui.css" />
    <link rel="icon" type="image/png" href="https://unpkg.com/swagger-ui-dist@5/favicon-32x32.png" sizes="32x32" />
    <link rel="icon" type="image/png" href="https://unpkg.com/swagger-ui-dist@5/favicon-16x16.png" sizes="16x16" />
    <style>
        html { box-sizing: border-box; overflow: -moz-scrollbars-vertical; overflow-y: scroll; }
        *, *:before, *:after { box-sizing: inherit; }
        body { margin: 0; padding: 0; }
        .topbar { display: none; }
    </style>
</head>
<body>
    <div id="swagger-ui"></div>
    <script src="https://unpkg.com/swagger-ui-dist@5/swagger-ui-bundle.js" charset="UTF-8"></script>
    <script src="https://unpkg.com/swagger-ui-dist@5/swagger-ui-standalone-preset.js" charset="UTF-8"></script>
    <script>
        window.onload = function() {
            window.ui = SwaggerUIBundle({
                url: "/farp/openapi.json",
                dom_id: '#swagger-ui',
                deepLinking: true,
                presets: [
                    SwaggerUIBundle.presets.apis,
                    SwaggerUIStandalonePreset
                ],
                plugins: [
                    SwaggerUIBundle.plugins.DownloadUrl
                ],
                layout: "StandaloneLayout",
                defaultModelsExpandDepth: 1,
                defaultModelExpandDepth: 1,
                docExpansion: "list",
                filter: true,
                tryItOutEnabled: true
            });
        };
    </script>
</body>
</html>"#;

        Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/html; charset=utf-8")
            .body(Full::new(Bytes::from(html)))
            .map_err(|e| Error::Internal(format!("Failed to build response: {}", e)))
    }

    /// Serve ReDoc UI
    async fn redoc_ui(&self) -> Result<Response<Full<Bytes>>> {
        let html = r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <title>FARP API Documentation - Octopus Gateway</title>
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <link href="https://fonts.googleapis.com/css?family=Montserrat:300,400,700|Roboto:300,400,700" rel="stylesheet">
    <style>
        body { margin: 0; padding: 0; }
    </style>
</head>
<body>
    <redoc spec-url='/farp/openapi.json'></redoc>
    <script src="https://cdn.redoc.ly/redoc/latest/bundles/redoc.standalone.js"></script>
</body>
</html>"#;

        Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/html; charset=utf-8")
            .body(Full::new(Bytes::from(html)))
            .map_err(|e| Error::Internal(format!("Failed to build response: {}", e)))
    }

    /// Trigger federation of all registered service schemas
    async fn trigger_federation(&self) -> Result<Response<Full<Bytes>>> {
        info!("Triggering schema federation");
        
        // Collect all schemas from registered services
        let service_names = self.registry.list_services();
        let mut all_schemas = Vec::new();
        
        for service_name in &service_names {
            if let Ok(registration) = self.registry.get_service(service_name) {
                all_schemas.extend(registration.schemas.clone());
            }
        }
        
        if all_schemas.is_empty() {
            return self.json_response(
                StatusCode::OK,
                &serde_json::json!({
                    "success": true,
                    "message": "No schemas to federate",
                    "schemas_processed": 0
                }),
            );
        }
        
        // Perform federation
        self.federation.federate_schemas(all_schemas.clone())?;
        
        let formats = self.federation.list_formats();
        
        self.json_response(
            StatusCode::OK,
            &serde_json::json!({
                "success": true,
                "message": "Schema federation completed",
                "schemas_processed": all_schemas.len(),
                "formats_generated": formats.iter().map(|f| f.to_string()).collect::<Vec<_>>(),
                "services": service_names
            }),
        )
    }

    /// Return a 404 Not Found response
    fn not_found(&self) -> Result<Response<Full<Bytes>>> {
        let response = serde_json::json!({
            "error": "not_found",
            "message": "Endpoint not found"
        });

        self.json_response(StatusCode::NOT_FOUND, &response)
    }

    /// Create a JSON response
    fn json_response<T: Serialize>(
        &self,
        status: StatusCode,
        body: &T,
    ) -> Result<Response<Full<Bytes>>> {
        let json = serde_json::to_string(body)
            .map_err(|e| Error::Internal(format!("Failed to serialize response: {}", e)))?;

        Response::builder()
            .status(status)
            .header("content-type", "application/json")
            .body(Full::new(Bytes::from(json)))
            .map_err(|e| Error::Internal(format!("Failed to build response: {}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::ServiceInfo as ManifestServiceInfo;

    #[tokio::test]
    async fn test_register_service() {
        let registry = Arc::new(SchemaRegistry::new());
        let handler = FarpApiHandler::new(registry);

        let manifest = SchemaManifest {
            service: ManifestServiceInfo {
                name: "test-service".to_string(),
                version: "1.0.0".to_string(),
                description: "Test service".to_string(),
                base_url: "http://localhost:8080".to_string(),
                metadata: std::collections::HashMap::new(),
            },
            capabilities: crate::manifest::ServiceCapabilities::default(),
            schemas: vec![],
            checksum: None,
        };

        let registration = RegistrationRequest {
            manifest,
            schemas: None,
        };

        let json = serde_json::to_string(&registration).unwrap();
        let req = Request::builder()
            .method(Method::POST)
            .uri("/farp/register")
            .body(Full::new(Bytes::from(json)))
            .unwrap();

        let res = handler.handle(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn test_list_services() {
        let registry = Arc::new(SchemaRegistry::new());
        let handler = FarpApiHandler::new(Arc::clone(&registry));

        // Register a service first
        let manifest = SchemaManifest {
            service: ManifestServiceInfo {
                name: "test-service".to_string(),
                version: "1.0.0".to_string(),
                description: "Test service".to_string(),
                base_url: "http://localhost:8080".to_string(),
                metadata: std::collections::HashMap::new(),
            },
            capabilities: crate::manifest::ServiceCapabilities::default(),
            schemas: vec![],
            checksum: None,
        };

        registry.register_service(manifest).unwrap();

        // List services
        let req = Request::builder()
            .method(Method::GET)
            .uri("/farp/services")
            .body(Full::new(Bytes::new()))
            .unwrap();

        let res = handler.handle(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_federated_openapi_empty() {
        let registry = Arc::new(SchemaRegistry::new());
        let handler = FarpApiHandler::new(registry);

        let req = Request::builder()
            .method(Method::GET)
            .uri("/openapi.json")
            .body(Full::new(Bytes::new()))
            .unwrap();

        let res = handler.handle(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            res.headers().get("content-type").unwrap(),
            "application/json"
        );

        // Should return empty schema
        let body = res.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["openapi"], "3.0.0");
        assert_eq!(json["paths"], serde_json::json!({}));
    }

    #[tokio::test]
    async fn test_get_federated_openapi_with_schema() {
        let registry = Arc::new(SchemaRegistry::new());
        let federation = Arc::new(SchemaFederation::new());
        let handler = FarpApiHandler::with_federation(Arc::clone(&registry), Arc::clone(&federation));

        // Create and register a test schema
        let schema = SchemaDescriptor::new(
            "test-schema",
            "test-service",
            SchemaFormat::OpenApi,
            "3.0.0",
            r#"{
                "openapi": "3.0.0",
                "info": {"title": "Test API", "version": "1.0.0"},
                "paths": {
                    "/users": {
                        "get": {"summary": "Get users"}
                    }
                }
            }"#,
        );

        federation.federate_schemas(vec![schema]).unwrap();

        let req = Request::builder()
            .method(Method::GET)
            .uri("/openapi.json")
            .body(Full::new(Bytes::new()))
            .unwrap();

        let res = handler.handle(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let body = res.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["openapi"], "3.0.0");
        assert!(json["paths"].is_object());
    }

    #[tokio::test]
    async fn test_get_federated_asyncapi() {
        let registry = Arc::new(SchemaRegistry::new());
        let handler = FarpApiHandler::new(registry);

        let req = Request::builder()
            .method(Method::GET)
            .uri("/asyncapi.json")
            .body(Full::new(Bytes::new()))
            .unwrap();

        let res = handler.handle(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            res.headers().get("content-type").unwrap(),
            "application/json"
        );

        let body = res.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["asyncapi"], "2.6.0");
    }

    #[tokio::test]
    async fn test_get_federated_graphql() {
        let registry = Arc::new(SchemaRegistry::new());
        let handler = FarpApiHandler::new(registry);

        let req = Request::builder()
            .method(Method::GET)
            .uri("/graphql")
            .body(Full::new(Bytes::new()))
            .unwrap();

        let res = handler.handle(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            res.headers().get("content-type").unwrap(),
            "text/plain; charset=utf-8"
        );

        let body = res.into_body().collect().await.unwrap().to_bytes();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(text.contains("GraphQL Schema"));
    }

    #[tokio::test]
    async fn test_get_federated_grpc() {
        let registry = Arc::new(SchemaRegistry::new());
        let handler = FarpApiHandler::new(registry);

        let req = Request::builder()
            .method(Method::GET)
            .uri("/grpc")
            .body(Full::new(Bytes::new()))
            .unwrap();

        let res = handler.handle(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            res.headers().get("content-type").unwrap(),
            "text/plain; charset=utf-8"
        );

        let body = res.into_body().collect().await.unwrap().to_bytes();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(text.contains("proto3"));
    }

    #[tokio::test]
    async fn test_list_federated_schemas() {
        let registry = Arc::new(SchemaRegistry::new());
        let federation = Arc::new(SchemaFederation::new());
        let handler = FarpApiHandler::with_federation(Arc::clone(&registry), Arc::clone(&federation));

        // Federate some schemas
        let schema1 = SchemaDescriptor::new(
            "schema1",
            "service1",
            SchemaFormat::OpenApi,
            "3.0.0",
            r#"{"openapi":"3.0.0","paths":{}}"#,
        );
        let schema2 = SchemaDescriptor::new(
            "schema2",
            "service2",
            SchemaFormat::AsyncApi,
            "2.0.0",
            r#"{"asyncapi":"2.0.0","channels":{}}"#,
        );

        federation.federate_schemas(vec![schema1, schema2]).unwrap();

        let req = Request::builder()
            .method(Method::GET)
            .uri("/farp/schemas")
            .body(Full::new(Bytes::new()))
            .unwrap();

        let res = handler.handle(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let body = res.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["schemas"].is_array());
        let schemas = json["schemas"].as_array().unwrap();
        assert_eq!(schemas.len(), 2);
    }

    #[tokio::test]
    async fn test_get_federated_schema_by_format() {
        let registry = Arc::new(SchemaRegistry::new());
        let federation = Arc::new(SchemaFederation::new());
        let handler = FarpApiHandler::with_federation(Arc::clone(&registry), Arc::clone(&federation));

        let schema = SchemaDescriptor::new(
            "test",
            "service",
            SchemaFormat::OpenApi,
            "3.0.0",
            r#"{"openapi":"3.0.0","paths":{}}"#,
        );
        federation.federate_schemas(vec![schema]).unwrap();

        let req = Request::builder()
            .method(Method::GET)
            .uri("/farp/schema/openapi")
            .body(Full::new(Bytes::new()))
            .unwrap();

        let res = handler.handle(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            res.headers().get("content-type").unwrap(),
            "application/json"
        );
    }

    #[tokio::test]
    async fn test_get_federated_schema_invalid_format() {
        let registry = Arc::new(SchemaRegistry::new());
        let handler = FarpApiHandler::new(registry);

        let req = Request::builder()
            .method(Method::GET)
            .uri("/farp/schema/invalid")
            .body(Full::new(Bytes::new()))
            .unwrap();

        let res = handler.handle(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);

        let body = res.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "invalid_format");
    }

    #[tokio::test]
    async fn test_trigger_federation() {
        let registry = Arc::new(SchemaRegistry::new());
        let handler = FarpApiHandler::new(Arc::clone(&registry));

        // Register a service with schemas
        let manifest = SchemaManifest {
            service: ManifestServiceInfo {
                name: "test-service".to_string(),
                version: "1.0.0".to_string(),
                description: "Test".to_string(),
                base_url: "http://localhost:8080".to_string(),
                metadata: std::collections::HashMap::new(),
            },
            capabilities: crate::manifest::ServiceCapabilities::default(),
            schemas: vec![],
            checksum: None,
        };

        registry.register_service(manifest).unwrap();

        let req = Request::builder()
            .method(Method::POST)
            .uri("/farp/federate")
            .body(Full::new(Bytes::new()))
            .unwrap();

        let res = handler.handle(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let body = res.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["success"], true);
    }

    #[tokio::test]
    async fn test_swagger_ui() {
        let registry = Arc::new(SchemaRegistry::new());
        let handler = FarpApiHandler::new(registry);

        let req = Request::builder()
            .method(Method::GET)
            .uri("/docs")
            .body(Full::new(Bytes::new()))
            .unwrap();

        let res = handler.handle(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            res.headers().get("content-type").unwrap(),
            "text/html; charset=utf-8"
        );

        let body = res.into_body().collect().await.unwrap().to_bytes();
        let html = String::from_utf8(body.to_vec()).unwrap();
        assert!(html.contains("Swagger"));
        assert!(html.contains("/openapi.json"));
    }

    #[tokio::test]
    async fn test_redoc_ui() {
        let registry = Arc::new(SchemaRegistry::new());
        let handler = FarpApiHandler::new(registry);

        let req = Request::builder()
            .method(Method::GET)
            .uri("/redoc")
            .body(Full::new(Bytes::new()))
            .unwrap();

        let res = handler.handle(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            res.headers().get("content-type").unwrap(),
            "text/html; charset=utf-8"
        );

        let body = res.into_body().collect().await.unwrap().to_bytes();
        let html = String::from_utf8(body.to_vec()).unwrap();
        assert!(html.contains("redoc"));
        assert!(html.contains("/openapi.json"));
    }

    #[tokio::test]
    async fn test_multiple_services_federation() {
        let registry = Arc::new(SchemaRegistry::new());
        let federation = Arc::new(SchemaFederation::new());
        let handler = FarpApiHandler::with_federation(Arc::clone(&registry), Arc::clone(&federation));

        // Register multiple services with OpenAPI schemas
        let schema1 = SchemaDescriptor::new(
            "users-schema",
            "users-service",
            SchemaFormat::OpenApi,
            "3.0.0",
            r#"{
                "openapi": "3.0.0",
                "paths": {
                    "/users": {"get": {"summary": "Get users"}}
                }
            }"#,
        );

        let schema2 = SchemaDescriptor::new(
            "posts-schema",
            "posts-service",
            SchemaFormat::OpenApi,
            "3.0.0",
            r#"{
                "openapi": "3.0.0",
                "paths": {
                    "/posts": {"get": {"summary": "Get posts"}}
                }
            }"#,
        );

        federation.federate_schemas(vec![schema1, schema2]).unwrap();

        let req = Request::builder()
            .method(Method::GET)
            .uri("/openapi.json")
            .body(Full::new(Bytes::new()))
            .unwrap();

        let res = handler.handle(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let body = res.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        
        // Both services should be in the federated schema with prefixed paths
        let paths = json["paths"].as_object().unwrap();
        assert!(paths.contains_key("/users-service/users"));
        assert!(paths.contains_key("/posts-service/posts"));
    }
}

