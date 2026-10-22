//! FARP Registration API for service registration

use crate::client::FarpClient;
use crate::federation::SchemaFederation;
use crate::manifest::{manifest_from_metadata, SchemaManifest};
use crate::registry::SchemaRegistry;
use crate::route_generator::{GeneratedRoute, RouteGenerator};
use crate::schema::{SchemaDescriptor, SchemaFormat};
use crate::validation::ManifestValidator;
use bytes::Bytes;
use http::{Method, Request, Response, StatusCode};
use http_body_util::{BodyExt, Full};
use octopus_core::{Error, Result};
use octopus_router::Router;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Header that carries the FARP registration token, for clients whose
/// `Authorization` header does not survive the trip (the Kubernetes API service
/// proxy strips it after authenticating the caller).
pub const FARP_TOKEN_HEADER: &str = "x-octopus-farp-token";

/// SHA-256 digest of the FARP registration token. Only the digest is kept, and
/// the `Debug` output never shows it.
#[derive(Clone, Copy)]
struct TokenDigest([u8; 32]);

impl std::fmt::Debug for TokenDigest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TokenDigest(..)")
    }
}

fn sha256(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

/// The credential of an `Authorization: Bearer <credential>` value.
fn bearer_token(value: &str) -> Option<&str> {
    let (scheme, credential) = value.trim().split_once(' ')?;
    let credential = credential.trim();
    (scheme.eq_ignore_ascii_case("bearer") && !credential.is_empty()).then_some(credential)
}

/// FARP API handler
#[derive(Clone, Debug)]
pub struct FarpApiHandler {
    registry: Arc<SchemaRegistry>,
    route_generator: Arc<RouteGenerator>,
    federation: Arc<SchemaFederation>,
    validator: Arc<ManifestValidator>,
    /// HTTP client for fetching manifests from services (FARP spec 17.4)
    farp_client: Arc<FarpClient>,
    /// Optional router for supplementing the federated spec with registered routes
    router: Option<Arc<Router>>,
    /// Hot-swappable virtual-gateway binding: scope all FARP routes to a hostname
    /// (e.g. `api.example.cloud`) and attach them to that gateway. Shared cell so
    /// the k8s controller can update it from the `OctopusGateway` CRD at runtime.
    binding: crate::binding::BindingCell,
    /// SHA-256 digest of the registration token. When set, every request other
    /// than GET and HEAD must present the token.
    registration_token: Option<TokenDigest>,
}

/// Registration request
#[derive(Debug, Serialize, Deserialize)]
pub struct RegistrationRequest {
    /// Service manifest
    pub manifest: SchemaManifest,

    /// Optional schemas (if not using location strategy)
    pub schemas: Option<Vec<SchemaDescriptor>>,
}

/// FARP v1 push registration request (per FARP spec section 17.4)
/// Payload: {instance: `ServiceInstance`, manifest: `SchemaManifest`}
#[derive(Debug, Serialize, Deserialize)]
pub struct PushRegistrationRequest {
    /// Service instance info
    pub instance: PushInstanceInfo,

    /// Optional manifest (may be fetched from service later)
    pub manifest: Option<SchemaManifest>,
}

/// Instance info for push-based registration
#[derive(Debug, Serialize, Deserialize)]
pub struct PushInstanceInfo {
    /// Unique instance ID
    pub id: String,

    /// Service name
    pub service_name: String,

    /// Service version
    #[serde(default)]
    pub service_version: Option<String>,

    /// Instance address
    pub address: String,

    /// Instance port
    pub port: u16,

    /// Service tags
    #[serde(default)]
    pub tags: Option<Vec<String>>,

    /// Metadata (e.g., farp.openapi, farp.manifest URLs)
    #[serde(default)]
    pub metadata: Option<HashMap<String, String>>,

    /// Instance status
    #[serde(default)]
    pub status: Option<String>,
}

/// Heartbeat request (§17.4.1 — `routes_checksum` is optional for reconciliation)
#[derive(Debug, Serialize, Deserialize)]
pub struct HeartbeatRequest {
    /// Instance status
    pub status: String,
    /// Service's expected routes checksum (optional, for reconciliation)
    #[serde(default)]
    pub routes_checksum: Option<String>,
}

/// Push registration acknowledgement (§17.4.1)
#[derive(Debug, Serialize, Deserialize)]
pub struct PushRegistrationAck {
    /// Status
    pub status: String,
    /// Routes checksum the gateway applied
    pub routes_checksum: String,
    /// Number of schemas the gateway fetched
    pub schemas_applied: usize,
}

/// Heartbeat acknowledgement (§17.4.1)
#[derive(Debug, Serialize, Deserialize)]
pub struct HeartbeatAck {
    /// Status
    pub status: String,
    /// Routes checksum the gateway holds
    pub routes_checksum: String,
    /// Number of schemas applied
    pub schemas_applied: usize,
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
    #[must_use]
    pub fn new(registry: Arc<SchemaRegistry>) -> Self {
        Self {
            registry,
            route_generator: Arc::new(RouteGenerator::new()),
            federation: Arc::new(SchemaFederation::new()),
            validator: Arc::new(ManifestValidator::default()),
            farp_client: Arc::new(FarpClient::default()),
            router: None,
            binding: crate::binding::new_binding_cell(),
            registration_token: None,
        }
    }

    /// Create a new FARP API handler with custom federation
    #[must_use]
    pub fn with_federation(
        registry: Arc<SchemaRegistry>,
        federation: Arc<SchemaFederation>,
    ) -> Self {
        Self {
            registry,
            route_generator: Arc::new(RouteGenerator::new()),
            federation,
            validator: Arc::new(ManifestValidator::default()),
            farp_client: Arc::new(FarpClient::default()),
            router: None,
            binding: crate::binding::new_binding_cell(),
            registration_token: None,
        }
    }

    /// Require a token on every request that changes state (anything other than
    /// GET and HEAD). The token travels as `Authorization: Bearer <token>` or in
    /// the [`FARP_TOKEN_HEADER`] header. `None` or an empty string leaves
    /// registration open. Only the SHA-256 digest is stored.
    #[must_use]
    pub fn with_registration_token(mut self, token: Option<&str>) -> Self {
        self.registration_token = token
            .filter(|t| !t.is_empty())
            .map(|t| TokenDigest(sha256(t.as_bytes())));
        self
    }

    /// Whether `req` presents the configured registration token.
    fn token_presented(&self, req: &Request<Full<Bytes>>, expected: &TokenDigest) -> bool {
        let headers = req.headers();
        let bearer = headers
            .get(http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(bearer_token);
        let custom = headers
            .get(FARP_TOKEN_HEADER)
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .filter(|t| !t.is_empty());
        // Fixed-size digest comparison avoids leaking the token length.
        [bearer, custom]
            .into_iter()
            .flatten()
            .any(|presented| sha256(presented.as_bytes()) == expected.0)
    }

    /// The 401 answer for a request that lacks a valid registration token.
    fn unauthorized(&self) -> Result<Response<Full<Bytes>>> {
        let json = serde_json::json!({
            "error": "unauthorized",
            "message": "FARP registration requires a token"
        });
        let mut response = self.json_response(StatusCode::UNAUTHORIZED, &json)?;
        response.headers_mut().insert(
            http::header::WWW_AUTHENTICATE,
            http::HeaderValue::from_static("Bearer realm=\"octopus-farp\""),
        );
        Ok(response)
    }

    /// Set the router for supplementing the federated spec with registered routes
    #[must_use]
    pub fn with_router(mut self, router: Arc<Router>) -> Self {
        self.router = Some(router);
        self
    }

    /// Bind all FARP-discovered routes to a virtual gateway (scope them to its
    /// hostname and attach them for policy inheritance).
    #[must_use]
    pub fn with_binding(self, binding: crate::binding::GatewayBinding) -> Self {
        self.binding.store(Arc::new(Some(binding)));
        self
    }

    /// Update (or clear) the gateway binding at runtime — used by the k8s
    /// controller to drive the binding from the `OctopusGateway` CRD.
    pub fn set_binding(&self, binding: Option<crate::binding::GatewayBinding>) {
        self.binding.store(Arc::new(binding));
    }

    /// A clone of the shared binding cell, so the discovery watcher can observe
    /// the same (possibly CRD-updated) binding the push handler uses.
    #[must_use]
    pub fn binding_handle(&self) -> crate::binding::BindingCell {
        Arc::clone(&self.binding)
    }

    /// Get a reference to the schema registry
    #[must_use]
    pub const fn registry(&self) -> &Arc<SchemaRegistry> {
        &self.registry
    }

    /// Get a reference to the schema federation
    #[must_use]
    pub const fn federation(&self) -> &Arc<SchemaFederation> {
        &self.federation
    }

    /// Handle FARP API requests
    pub async fn handle(&self, req: Request<Full<Bytes>>) -> Result<Response<Full<Bytes>>> {
        let method = req.method().clone();
        let path = req.uri().path().to_string();

        debug!(method = %method, path = %path, "FARP API request");

        // Every alias (`/_farp/v1/*`, `/__farp/*`, `/farp/*`) is rewritten to
        // `/farp/*` before it gets here, so this one check covers them all.
        if let Some(expected) = &self.registration_token {
            if method != Method::GET
                && method != Method::HEAD
                && !self.token_presented(&req, expected)
            {
                debug!(method = %method, path = %path, "FARP request rejected: missing or wrong token");
                return self.unauthorized();
            }
        }

        match (method, path.as_str()) {
            // Service registration (supports both legacy and FARP v1 push payloads)
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

            // FARP v1 push protocol endpoints (per spec section 17.4)
            (Method::PUT, p) if p.starts_with("/farp/heartbeat/") => {
                let instance_id = p.trim_start_matches("/farp/heartbeat/");
                self.heartbeat(instance_id, req).await
            }
            (Method::DELETE, p) if p.starts_with("/farp/deregister/") => {
                let instance_id = p.trim_start_matches("/farp/deregister/");
                self.deregister_by_id(instance_id).await
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

    /// Register a service (supports both legacy and FARP v1 push payloads)
    async fn register_service(&self, req: Request<Full<Bytes>>) -> Result<Response<Full<Bytes>>> {
        let body_bytes = req
            .into_body()
            .collect()
            .await
            .map_err(|e| Error::Farp(format!("Failed to read request body: {e}")))?
            .to_bytes();

        // Try FARP v1 push format first: {instance, manifest}
        // Then fall back to legacy format: {manifest, schemas}
        let raw: serde_json::Value = serde_json::from_slice(&body_bytes)
            .map_err(|e| Error::Farp(format!("Invalid JSON: {e}")))?;

        if raw.get("instance").is_some() {
            // FARP v1 push protocol (spec section 17.4)
            // Payload: {instance: ServiceInstance, manifest?: SchemaManifest}
            // If manifest is omitted, fetch it from the service's /_farp/manifest endpoint.
            let push_req: PushRegistrationRequest = serde_json::from_value(raw)
                .map_err(|e| Error::Farp(format!("Invalid push registration request: {e}")))?;

            let instance = &push_req.instance;
            let service_name = instance.service_name.clone();

            let manifest = if let Some(m) = push_req.manifest {
                m
            } else {
                // Per FARP spec 17.5: services expose GET /_farp/manifest
                // Resolve the manifest URL: prefer metadata "farp.manifest" (already
                // a full URL), otherwise build from instance address.
                // Note: Address may be "host:port" (port field = 0) or just "host"
                // (port field > 0). Avoid double-port like "host:7900:0".
                let metadata = instance.metadata.as_ref();
                let manifest_url = metadata
                    .and_then(|m| m.get("farp.manifest"))
                    .cloned()
                    .unwrap_or_else(|| {
                        let base = if instance.port > 0 && !instance.address.contains(':') {
                            format!("http://{}:{}", instance.address, instance.port)
                        } else {
                            format!("http://{}", instance.address)
                        };
                        format!("{base}/_farp/manifest")
                    });

                info!(
                    service = %service_name,
                    instance_id = %instance.id,
                    url = %manifest_url,
                    "Fetching manifest from service"
                );

                match self.farp_client.fetch_manifest(&manifest_url).await {
                    Ok(m) => m,
                    Err(e) => {
                        warn!(
                            service = %service_name,
                            error = %e,
                            "Failed to fetch manifest from service, registering with instance info only"
                        );
                        let version = instance.service_version.as_deref().unwrap_or("0.0.0");
                        let base = if instance.port > 0 && !instance.address.contains(':') {
                            format!("http://{}:{}", instance.address, instance.port)
                        } else {
                            format!("http://{}", instance.address)
                        };
                        // A service that opted in with `farp.openapi.path` gets the
                        // same manifest discovery would build from that metadata.
                        // Without the key, only the instance info is registered.
                        if let Some(m) = metadata.filter(|m| m.contains_key("farp.openapi.path")) {
                            manifest_from_metadata(&service_name, version, &instance.id, &base, m)?
                        } else {
                            let mut m = SchemaManifest::new(&service_name, version, &instance.id);
                            m.endpoints.health = format!("{base}/_farp/health");
                            m
                        }
                    }
                }
            };

            self.validator.validate(&manifest)?;
            info!(service = %service_name, instance_id = %instance.id, "Registering service (FARP v1 push)");

            // Check if service requests tag collapsing via instance metadata
            if let Some(ref metadata) = instance.metadata {
                if metadata
                    .get("farp.collapse_service_tags")
                    .is_some_and(|v| v == "true")
                {
                    self.federation.set_collapse_service_tags(true);
                }
            }

            self.registry.register_service(manifest.clone()).await?;

            // Store manifest_url for heartbeat retry (§17.4.1)
            let manifest_url = instance
                .metadata
                .as_ref()
                .and_then(|m| m.get("farp.manifest"))
                .cloned()
                .unwrap_or_else(|| {
                    let base = if instance.port > 0 && !instance.address.contains(':') {
                        format!("http://{}:{}", instance.address, instance.port)
                    } else {
                        format!("http://{}", instance.address)
                    };
                    format!("{base}/_farp/manifest")
                });
            if let Some(mut reg) = self.registry.services_mut().get_mut(&service_name) {
                reg.manifest_url = Some(manifest_url);
            }

            // Fetch actual schema content from the service's endpoints
            // (per FARP spec, the manifest lists schema URLs; the gateway fetches them)
            crate::schema_ops::fetch_manifest_schemas(&self.farp_client, &self.registry, &manifest)
                .await;

            // Register upstream and routes in the gateway router (if available)
            if let Some(ref router) = self.router {
                // Register upstream cluster
                let mut cluster = octopus_core::UpstreamCluster::new(&service_name);
                let inst_address = if instance.port > 0 && !instance.address.contains(':') {
                    instance.address.clone()
                } else {
                    instance
                        .address
                        .split(':')
                        .next()
                        .unwrap_or(&instance.address)
                        .to_string()
                };
                let inst_port = if instance.port > 0 {
                    instance.port
                } else {
                    instance
                        .address
                        .split(':')
                        .nth(1)
                        .and_then(|p| p.parse().ok())
                        .unwrap_or(80)
                };
                let upstream_inst =
                    octopus_core::UpstreamInstance::new(&instance.id, &inst_address, inst_port);
                cluster.add_instance(upstream_inst);
                router.register_upstream(cluster);

                // Prefer pre-computed route table from manifest (FARP v1.1.0).
                // This uses the exact routes the service declared, already validated.
                let mut builders = Vec::new();
                if manifest.has_route_table() {
                    for route_desc in &manifest.route_table {
                        // Skip introspection endpoints
                        if crate::schema_ops::should_exclude_introspection()
                            && crate::schema_ops::is_introspection_path(&route_desc.path)
                        {
                            continue;
                        }
                        for method_str in &route_desc.methods {
                            let Some(method) = crate::routes::http_method(method_str) else {
                                continue;
                            };
                            builders.push(crate::routes::service_route_builder(
                                &service_name,
                                &route_desc.path,
                                method,
                            ));
                        }
                    }
                } else if let Ok(schemas) = self.registry.get_schemas(&service_name) {
                    // Fallback: parse OpenAPI schema paths
                    for schema in &schemas {
                        if schema.format != SchemaFormat::OpenApi {
                            continue;
                        }
                        let Ok(schema_json) =
                            serde_json::from_str::<serde_json::Value>(&schema.content)
                        else {
                            continue;
                        };
                        let Some(paths) = schema_json.get("paths").and_then(|p| p.as_object())
                        else {
                            continue;
                        };
                        for (path, operations) in paths {
                            if crate::schema_ops::should_exclude_introspection()
                                && crate::schema_ops::is_introspection_path(path)
                            {
                                continue;
                            }
                            let Some(ops) = operations.as_object() else {
                                continue;
                            };
                            for method in ops.keys().filter_map(|m| crate::routes::http_method(m)) {
                                builders.push(crate::routes::service_route_builder(
                                    &service_name,
                                    path,
                                    method,
                                ));
                            }
                        }
                    }
                }

                // Replace the service's FARP routes in one step, so a
                // re-registration drops routes the service no longer declares.
                let binding = self.binding.load();
                let route_count = crate::routes::replace_service_routes(
                    router,
                    &service_name,
                    builders,
                    (**binding).as_ref(),
                    false,
                );
                debug!(
                    service = %service_name,
                    route_count,
                    from_route_table = manifest.has_route_table(),
                    "FARP routes registered"
                );

                info!(service = %service_name, "Upstream and routes registered in gateway router");
            }

            // Trigger federation to merge all registered schemas
            if let Err(e) = crate::schema_ops::trigger_federation(&self.registry, &self.federation)
            {
                warn!(service = %service_name, error = %e, "Schema federation failed");
            }

            // Return ack with gateway state (§17.4.1)
            let (routes_checksum, schemas_applied) = self.get_instance_state(&manifest.instance_id);

            return self.json_response(
                StatusCode::CREATED,
                &PushRegistrationAck {
                    status: "registered".to_string(),
                    routes_checksum,
                    schemas_applied,
                },
            );
        }

        // Legacy format: {manifest, schemas}
        let registration: RegistrationRequest = serde_json::from_value(raw)
            .map_err(|e| Error::Farp(format!("Invalid registration request: {e}")))?;

        let service_name = registration.manifest.service_name.clone();

        // Validate manifest before storing
        self.validator.validate(&registration.manifest)?;

        info!(service = %service_name, "Registering service");

        // Register with registry
        self.registry
            .register_service(registration.manifest.clone())
            .await?;

        // Store schemas and trigger federation
        if let Some(schemas) = &registration.schemas {
            // Add schemas to registry for federation
            for schema in schemas {
                if let Ok(mut reg) = self.registry.get_service(&service_name) {
                    reg.schemas.push(schema.clone());
                }
            }

            // Trigger federation automatically
            let all_service_schemas: Vec<_> = self
                .registry
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
            message: format!("Service '{service_name}' registered successfully"),
            routes: all_routes,
        };

        self.json_response(StatusCode::CREATED, &response)
    }

    /// Handle heartbeat from a service instance (FARP v1 push protocol, §17.4.1)
    async fn heartbeat(
        &self,
        instance_id: &str,
        req: Request<Full<Bytes>>,
    ) -> Result<Response<Full<Bytes>>> {
        let body_bytes = req
            .into_body()
            .collect()
            .await
            .map_err(|e| Error::Farp(format!("Failed to read request body: {e}")))?
            .to_bytes();

        let heartbeat: HeartbeatRequest = serde_json::from_slice(&body_bytes)
            .map_err(|e| Error::Farp(format!("Invalid heartbeat request: {e}")))?;

        debug!(instance_id = %instance_id, "Heartbeat received");

        // Update last-seen in registry.
        // If the instance is unknown (e.g., gateway restarted and lost state),
        // return 404 with a re-register hint so the service pushes its full manifest.
        if self.registry.heartbeat(instance_id).is_err() {
            debug!(instance_id = %instance_id, "Heartbeat for unknown instance, requesting re-registration");
            return self.json_response(
                StatusCode::NOT_FOUND,
                &serde_json::json!({
                    "error": "instance not found",
                    "action": "re-register",
                    "message": "Gateway has no record of this instance. Please re-register with full manifest."
                }),
            );
        }

        // §17.4.1 Reconciliation: if service sent a checksum and it doesn't
        // match what the gateway has (or gateway has 0 schemas), retry the
        // manifest fetch so schemas converge.
        let (mut gw_checksum, mut gw_schemas) = self.get_instance_state(instance_id);

        if let Some(ref service_checksum) = heartbeat.routes_checksum {
            if !service_checksum.is_empty()
                && (gw_checksum.is_empty() || gw_checksum != *service_checksum)
            {
                info!(
                    instance_id = %instance_id,
                    service_checksum = %service_checksum,
                    gateway_checksum = %gw_checksum,
                    "Checksum mismatch, retrying manifest fetch"
                );
                self.retry_manifest_fetch(instance_id).await;
                // Re-read state after fetch
                let updated = self.get_instance_state(instance_id);
                gw_checksum = updated.0;
                gw_schemas = updated.1;
            }
        }

        self.json_response(
            StatusCode::OK,
            &HeartbeatAck {
                status: "ok".to_string(),
                routes_checksum: gw_checksum,
                schemas_applied: gw_schemas,
            },
        )
    }

    /// Get the `routes_checksum` and schema count for an instance.
    fn get_instance_state(&self, instance_id: &str) -> (String, usize) {
        for name in self.registry.list_services() {
            if let Ok(reg) = self.registry.get_service(&name) {
                if reg.manifest.instance_id == instance_id {
                    let checksum = reg.manifest.routes_checksum.clone().unwrap_or_default();
                    return (checksum, reg.schemas.len());
                }
            }
        }
        (String::new(), 0)
    }

    /// Re-fetch manifest and schemas from a service instance (§17.4.1).
    async fn retry_manifest_fetch(&self, instance_id: &str) {
        // Find the service and its manifest URL
        let info: Option<(String, String)> =
            self.registry.list_services().iter().find_map(|name| {
                self.registry.get_service(name).ok().and_then(|reg| {
                    if reg.manifest.instance_id == instance_id {
                        reg.manifest_url.map(|url| (name.clone(), url))
                    } else {
                        None
                    }
                })
            });

        let Some((service_name, manifest_url)) = info else {
            warn!(instance_id = %instance_id, "No manifest URL for retry");
            return;
        };

        match self.farp_client.fetch_manifest(&manifest_url).await {
            Ok(new_manifest) => {
                info!(
                    service = %service_name,
                    instance_id = %instance_id,
                    "Manifest retry succeeded"
                );
                if let Err(e) = self.registry.update_service(new_manifest.clone()).await {
                    warn!(error = %e, "Failed to update manifest after retry");
                    return;
                }
                // Preserve the manifest_url
                if let Some(mut reg) = self.registry.services_mut().get_mut(&service_name) {
                    reg.manifest_url = Some(manifest_url);
                }
                crate::schema_ops::fetch_manifest_schemas(
                    &self.farp_client,
                    &self.registry,
                    &new_manifest,
                )
                .await;
                if let Err(e) =
                    crate::schema_ops::trigger_federation(&self.registry, &self.federation)
                {
                    warn!(error = %e, "Federation failed after manifest retry");
                }
            }
            Err(e) => {
                debug!(
                    instance_id = %instance_id,
                    error = %e,
                    "Manifest retry fetch failed (service may still be starting)"
                );
            }
        }
    }

    /// Deregister a service instance by ID (FARP v1 push protocol)
    async fn deregister_by_id(&self, instance_id: &str) -> Result<Response<Full<Bytes>>> {
        info!(instance_id = %instance_id, "Deregistering service instance");

        if let Ok(service_name) = self.registry.deregister_by_instance_id(instance_id) {
            self.remove_service_from_router(&service_name);
            self.json_response(
                StatusCode::OK,
                &serde_json::json!({"status": "deregistered"}),
            )
        } else {
            self.json_response(
                StatusCode::NOT_FOUND,
                &serde_json::json!({"error": "instance not found"}),
            )
        }
    }

    /// Drop a deregistered service's FARP routes and its upstream.
    fn remove_service_from_router(&self, service_name: &str) {
        if let Some(ref router) = self.router {
            let removed = crate::routes::remove_service_routes(router, service_name);
            router.remove_upstream(service_name);
            info!(service = %service_name, routes_removed = removed, "Service removed from gateway router");
        }
    }

    /// Update a service
    async fn update_service(
        &self,
        service_name: &str,
        req: Request<Full<Bytes>>,
    ) -> Result<Response<Full<Bytes>>> {
        let body_bytes = req
            .into_body()
            .collect()
            .await
            .map_err(|e| Error::Farp(format!("Failed to read request body: {e}")))?
            .to_bytes();
        let registration: RegistrationRequest = serde_json::from_slice(&body_bytes)
            .map_err(|e| Error::Farp(format!("Invalid registration request: {e}")))?;

        // Validate manifest before storing
        self.validator.validate(&registration.manifest)?;

        info!(service = %service_name, "Updating service");

        self.registry.update_service(registration.manifest).await?;

        let response = serde_json::json!({
            "success": true,
            "message": format!("Service '{}' updated successfully", service_name)
        });

        self.json_response(StatusCode::OK, &response)
    }

    /// Deregister a service
    async fn deregister_service(&self, service_name: &str) -> Result<Response<Full<Bytes>>> {
        info!(service = %service_name, "Deregistering service");

        self.registry.deregister_service(service_name).await?;
        self.remove_service_from_router(service_name);

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
                    schema_count: reg.manifest.schemas.len(), // ← Fixed: read from manifest
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

    /// Get federated `OpenAPI` schema
    async fn get_federated_openapi(&self) -> Result<Response<Full<Bytes>>> {
        info!("Serving federated OpenAPI schema");

        if let Ok(schema) = self.federation.get_federated(&SchemaFormat::OpenApi) {
            // Supplement the federated spec with routes from the router
            let content = if let Some(ref router) = self.router {
                self.supplement_spec_with_routes(&schema.content, router)
            } else {
                schema.content
            };

            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .body(Full::new(Bytes::from(content)))
                .map_err(|e| Error::Internal(format!("Failed to build response: {e}")))
        } else {
            // No federated schema available — build one from router routes as fallback
            let empty_schema = serde_json::json!({
                "openapi": "3.0.0",
                "info": {
                    "title": "Federated API",
                    "version": "1.0.0",
                    "description": "API specification from registered services"
                },
                "servers": [{"url": "/", "description": "Gateway"}],
                "paths": {}
            });

            let content = if let Some(ref router) = self.router {
                self.supplement_spec_with_routes(
                    &serde_json::to_string_pretty(&empty_schema).unwrap(),
                    router,
                )
            } else {
                serde_json::to_string_pretty(&empty_schema).unwrap()
            };

            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .body(Full::new(Bytes::from(content)))
                .map_err(|e| Error::Internal(format!("Failed to build response: {e}")))
        }
    }

    /// Supplement the federated `OpenAPI` spec with routes from the gateway router.
    /// Adds any gateway routes that are missing from the federated spec.
    fn supplement_spec_with_routes(&self, spec_content: &str, router: &Router) -> String {
        let mut spec: serde_json::Value = match serde_json::from_str(spec_content) {
            Ok(v) => v,
            Err(_) => return spec_content.to_string(),
        };

        let all_routes = router.get_all_routes();
        let exclude_introspection = crate::schema_ops::should_exclude_introspection();

        // Build a map of new routes to add
        let mut new_paths: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();

        for route in &all_routes {
            // Skip FARP internal routes
            if route.path.starts_with("/farp")
                || route.path.starts_with("/__")
                || route.path.starts_with("/_farp")
            {
                continue;
            }

            // Skip introspection endpoints if filtering is enabled
            if exclude_introspection {
                let segments: Vec<&str> =
                    route.path.trim_start_matches('/').splitn(2, '/').collect();
                if segments.len() == 2 {
                    let service_path = format!("/{}", segments[1]);
                    if crate::schema_ops::is_introspection_path(&service_path) {
                        continue;
                    }
                } else if segments.len() == 1 {
                    continue;
                }
            }

            let method_str = route.method.as_str().to_lowercase();

            // Check if this path+method already exists in the current spec
            let already_exists = spec
                .get("paths")
                .and_then(|p| p.get(&route.path))
                .and_then(|item| item.get(&method_str))
                .is_some();

            if already_exists {
                continue;
            }

            let operation = serde_json::json!({
                "tags": [&route.upstream_name],
                "summary": format!("{} {}", route.method.as_str(), route.path),
                "responses": {
                    "200": {
                        "description": "Successful response"
                    }
                }
            });

            let path_item = new_paths
                .entry(route.path.clone())
                .or_insert_with(|| serde_json::json!({}));

            if let Some(obj) = path_item.as_object_mut() {
                obj.insert(method_str, operation);
            }
        }

        // Merge new paths into the spec
        if !new_paths.is_empty() {
            if let Some(obj) = spec.as_object_mut() {
                let paths = obj.entry("paths").or_insert_with(|| serde_json::json!({}));

                if let Some(paths_obj) = paths.as_object_mut() {
                    for (path, path_item) in new_paths {
                        if let Some(existing) = paths_obj.get_mut(&path) {
                            // Merge methods into existing path item
                            if let (Some(existing_obj), Some(new_obj)) =
                                (existing.as_object_mut(), path_item.as_object())
                            {
                                for (method, op) in new_obj {
                                    existing_obj.entry(method).or_insert(op.clone());
                                }
                            }
                        } else {
                            paths_obj.insert(path, path_item);
                        }
                    }
                }
            }
        }

        serde_json::to_string_pretty(&spec).unwrap_or_else(|_| spec_content.to_string())
    }

    /// Get federated `AsyncAPI` schema
    async fn get_federated_asyncapi(&self) -> Result<Response<Full<Bytes>>> {
        info!("Serving federated AsyncAPI schema");

        if let Ok(schema) = self.federation.get_federated(&SchemaFormat::AsyncApi) {
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .body(Full::new(Bytes::from(schema.content)))
                .map_err(|e| Error::Internal(format!("Failed to build response: {e}")))
        } else {
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
                .body(Full::new(Bytes::from(
                    serde_json::to_string_pretty(&empty_schema).unwrap(),
                )))
                .map_err(|e| Error::Internal(format!("Failed to build response: {e}")))
        }
    }

    /// Get federated GraphQL schema
    async fn get_federated_graphql(&self) -> Result<Response<Full<Bytes>>> {
        info!("Serving federated GraphQL schema");

        if let Ok(schema) = self.federation.get_federated(&SchemaFormat::GraphQL) {
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "text/plain; charset=utf-8")
                .body(Full::new(Bytes::from(schema.content)))
                .map_err(|e| Error::Internal(format!("Failed to build response: {e}")))
        } else {
            let empty_schema =
                "# Federated GraphQL Schema (No Services)\ntype Query { _empty: String }";

            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "text/plain; charset=utf-8")
                .body(Full::new(Bytes::from(empty_schema)))
                .map_err(|e| Error::Internal(format!("Failed to build response: {e}")))
        }
    }

    /// Get federated gRPC schema (protobuf)
    async fn get_federated_grpc(&self) -> Result<Response<Full<Bytes>>> {
        info!("Serving federated gRPC schema");

        if let Ok(schema) = self.federation.get_federated(&SchemaFormat::Grpc) {
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "text/plain; charset=utf-8")
                .body(Full::new(Bytes::from(schema.content)))
                .map_err(|e| Error::Internal(format!("Failed to build response: {e}")))
        } else {
            let empty_schema = "// Federated gRPC Schema (No Services)\nsyntax = \"proto3\";";

            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "text/plain; charset=utf-8")
                .body(Full::new(Bytes::from(empty_schema)))
                .map_err(|e| Error::Internal(format!("Failed to build response: {e}")))
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
                    .map_err(|e| Error::Internal(format!("Failed to build response: {e}")))
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
            .map_err(|e| Error::Internal(format!("Failed to build response: {e}")))
    }

    /// Serve `ReDoc` UI
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
            .map_err(|e| Error::Internal(format!("Failed to build response: {e}")))
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
                "formats_generated": formats.iter().map(std::string::ToString::to_string).collect::<Vec<_>>(),
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
            .map_err(|e| Error::Internal(format!("Failed to serialize response: {e}")))?;

        Response::builder()
            .status(status)
            .header("content-type", "application/json")
            .body(Full::new(Bytes::from(json)))
            .map_err(|e| Error::Internal(format!("Failed to build response: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_manifest(name: &str, version: &str, instance: &str) -> SchemaManifest {
        let mut manifest = SchemaManifest::new(name, version, instance);
        // Add required health endpoint for external registry validation
        manifest.endpoints.health = "http://localhost:8080/health".to_string();
        // Attach a schema and compute the checksum so the manifest satisfies strict
        // JSON-schema validation, which requires a 64-char SHA256 checksum.
        manifest.add_schema(crate::types::SchemaDescriptor::new(
            crate::types::SchemaType::OpenAPI,
            "3.1.0",
            crate::types::SchemaLocation::http("http://localhost:8080/openapi.json"),
            "application/json",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            1024,
        ));
        manifest
            .update_checksum()
            .expect("failed to compute manifest checksum");
        manifest
    }

    #[tokio::test]
    async fn test_register_service() {
        let registry = Arc::new(SchemaRegistry::new());
        let handler = FarpApiHandler::new(registry);

        let manifest = create_test_manifest("test-service", "1.0.0", "test-instance");

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
        let manifest = create_test_manifest("test-service", "1.0.0", "test-instance");

        registry.register_service(manifest).await.unwrap();

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
        let handler =
            FarpApiHandler::with_federation(Arc::clone(&registry), Arc::clone(&federation));

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
        // External farp merger uses OpenAPI 3.1.0
        assert_eq!(json["openapi"], "3.1.0");
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
        let handler =
            FarpApiHandler::with_federation(Arc::clone(&registry), Arc::clone(&federation));

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
        let handler =
            FarpApiHandler::with_federation(Arc::clone(&registry), Arc::clone(&federation));

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
        let manifest = create_test_manifest("test-service", "1.0.0", "test-instance");

        registry.register_service(manifest).await.unwrap();

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
        let handler =
            FarpApiHandler::with_federation(Arc::clone(&registry), Arc::clone(&federation));

        // Register multiple services with complete OpenAPI schemas
        let schema1 = SchemaDescriptor::new(
            "users-schema",
            "users-service",
            SchemaFormat::OpenApi,
            "3.0.0",
            r#"{
                "openapi": "3.0.0",
                "info": {
                    "title": "Users API",
                    "version": "1.0.0"
                },
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
                "info": {
                    "title": "Posts API",
                    "version": "1.0.0"
                },
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

        // Both services should be in the federated schema
        // The external farp merger includes both services
        let paths = json["paths"].as_object().unwrap();
        // Verify we have paths from both services
        assert!(!paths.is_empty(), "Federated schema should have paths");
        // Verify it's a valid OpenAPI 3.1.0 schema
        assert_eq!(json["openapi"], "3.1.0");
    }

    fn token_handler(token: &str) -> FarpApiHandler {
        FarpApiHandler::new(Arc::new(SchemaRegistry::new())).with_registration_token(Some(token))
    }

    fn register_request(header: Option<(&str, &str)>) -> Request<Full<Bytes>> {
        let registration = RegistrationRequest {
            manifest: create_test_manifest("test-service", "1.0.0", "test-instance"),
            schemas: None,
        };
        let mut builder = Request::builder()
            .method(Method::POST)
            .uri("/farp/register");
        if let Some((name, value)) = header {
            builder = builder.header(name, value);
        }
        builder
            .body(Full::new(Bytes::from(
                serde_json::to_string(&registration).unwrap(),
            )))
            .unwrap()
    }

    #[tokio::test]
    async fn register_requires_token_when_configured() {
        let res = token_handler("t")
            .handle(register_request(None))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            res.headers().get("www-authenticate").unwrap(),
            "Bearer realm=\"octopus-farp\""
        );
        assert_eq!(
            res.headers().get("content-type").unwrap(),
            "application/json"
        );
        let body = res.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "error": "unauthorized",
                "message": "FARP registration requires a token"
            })
        );
    }

    #[tokio::test]
    async fn register_accepts_bearer_token() {
        let res = token_handler("t")
            .handle(register_request(Some(("authorization", "Bearer t"))))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn register_accepts_custom_header_token() {
        let res = token_handler("t")
            .handle(register_request(Some((FARP_TOKEN_HEADER, "t"))))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn wrong_token_is_rejected() {
        let handler = token_handler("t");
        for header in [
            ("authorization", "Bearer x"),
            ("authorization", "Basic t"),
            ("authorization", "Bearer "),
            (FARP_TOKEN_HEADER, "x"),
            (FARP_TOKEN_HEADER, ""),
        ] {
            let res = handler
                .handle(register_request(Some(header)))
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "{header:?}");
        }
    }

    #[tokio::test]
    async fn every_write_method_needs_the_token() {
        let handler = token_handler("t");
        for (method, uri) in [
            (Method::PUT, "/farp/services/svc"),
            (Method::DELETE, "/farp/services/svc"),
            (Method::PUT, "/farp/heartbeat/inst"),
            (Method::DELETE, "/farp/deregister/inst"),
            (Method::POST, "/farp/federate"),
            (Method::PATCH, "/farp/services/svc"),
        ] {
            let req = Request::builder()
                .method(method.clone())
                .uri(uri)
                .body(Full::new(Bytes::new()))
                .unwrap();
            let res = handler.handle(req).await.unwrap();
            assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "{method} {uri}");
        }
    }

    #[tokio::test]
    async fn reads_stay_open_with_token_configured() {
        let handler = token_handler("t");
        let get = Request::builder()
            .method(Method::GET)
            .uri("/farp/services")
            .body(Full::new(Bytes::new()))
            .unwrap();
        assert_eq!(handler.handle(get).await.unwrap().status(), StatusCode::OK);

        // HEAD is not routed to any endpoint, but the token check lets it through.
        let head = Request::builder()
            .method(Method::HEAD)
            .uri("/farp/services")
            .body(Full::new(Bytes::new()))
            .unwrap();
        assert_ne!(
            handler.handle(head).await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn no_token_configured_keeps_registration_open() {
        let handler = FarpApiHandler::new(Arc::new(SchemaRegistry::new()));
        let res = handler.handle(register_request(None)).await.unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn empty_token_means_no_token() {
        let handler =
            FarpApiHandler::new(Arc::new(SchemaRegistry::new())).with_registration_token(Some(""));
        let res = handler.handle(register_request(None)).await.unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn heartbeat_for_unknown_instance_returns_404() {
        let req = Request::builder()
            .method(Method::PUT)
            .uri("/farp/heartbeat/nope")
            .header("authorization", "Bearer t")
            .body(Full::new(Bytes::from(r#"{"status":"healthy"}"#)))
            .unwrap();
        let res = token_handler("t").handle(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        let body = res.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["action"], "re-register");
    }

    /// The `billing` manifest a push registration sends: one route-table entry
    /// (`GET /invoices`) and a schema on a closed port, so the schema fetch
    /// fails fast and the route table drives the routes.
    fn billing_manifest() -> SchemaManifest {
        let mut manifest = SchemaManifest::new("billing", "1.0.0", "billing-1");
        manifest.endpoints.health = "http://127.0.0.1:1/health".to_string();
        manifest.add_schema(crate::types::SchemaDescriptor::new(
            crate::types::SchemaType::OpenAPI,
            "3.1.0",
            crate::types::SchemaLocation::http("http://127.0.0.1:1/openapi.json"),
            "application/json",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            1024,
        ));
        manifest.set_route_table(vec![serde_json::from_value(serde_json::json!({
            "path": "/invoices",
            "methods": ["GET"],
            "protocol": "rest"
        }))
        .unwrap()]);
        manifest
            .update_checksum()
            .expect("failed to compute manifest checksum");
        manifest
    }

    /// A handler wired to `router` with FARP routes bound to `a.dev` and
    /// `*.b.dev`.
    fn bound_handler(router: &Arc<Router>) -> FarpApiHandler {
        FarpApiHandler::new(Arc::new(SchemaRegistry::new()))
            .with_router(Arc::clone(router))
            .with_binding(crate::binding::GatewayBinding::for_hostnames(&[
                "a.dev".into(),
                "*.b.dev".into(),
            ]))
    }

    async fn push_register(handler: &FarpApiHandler) -> StatusCode {
        let body = serde_json::json!({
            "instance": {
                "id": "billing-1",
                "service_name": "billing",
                "address": "127.0.0.1",
                "port": 1
            },
            "manifest": billing_manifest(),
        });
        let req = Request::builder()
            .method(Method::POST)
            .uri("/farp/register")
            .body(Full::new(Bytes::from(body.to_string())))
            .unwrap();
        handler.handle(req).await.unwrap().status()
    }

    #[tokio::test]
    async fn push_registration_binds_routes_to_every_hostname() {
        let router = Arc::new(Router::new());
        let handler = bound_handler(&router);
        assert_eq!(push_register(&handler).await, StatusCode::CREATED);

        for host in ["a.dev", "x.b.dev"] {
            let m = router
                .match_route(host, &Method::GET, "/billing/invoices")
                .unwrap_or_else(|_| panic!("no FARP route on {host}"));
            assert_eq!(m.route.upstream_name, "billing");
            assert_eq!(m.route.origin, octopus_router::RouteOrigin::Farp);
            assert_eq!(m.route.id.as_deref(), Some("farp:billing"));
            assert_eq!(m.route.strip_prefix.as_deref(), Some("/billing"));
            assert_eq!(m.route.priority, 100);
        }
        assert!(router
            .match_route("c.dev", &Method::GET, "/billing/invoices")
            .is_err());
    }

    #[tokio::test]
    async fn reregistration_keeps_manual_route_at_the_same_path() {
        let router = Arc::new(Router::new());
        router.register_upstream(octopus_core::UpstreamCluster::new("lp-manual"));
        router
            .add_route(
                octopus_router::RouteBuilder::new()
                    .id(Some("lpr-1"))
                    .origin(octopus_router::RouteOrigin::Admin)
                    .method(Method::GET)
                    .path("/billing/invoices")
                    .host(octopus_router::HostMatch::Exact("a.dev".into()))
                    .upstream_name("lp-manual")
                    .build()
                    .unwrap(),
            )
            .unwrap();
        let handler = bound_handler(&router);

        for _ in 0..2 {
            assert_eq!(push_register(&handler).await, StatusCode::CREATED);
            let m = router
                .match_route("a.dev", &Method::GET, "/billing/invoices")
                .unwrap();
            assert_eq!(m.route.upstream_name, "lp-manual");
            assert_eq!(router.routes_by_id("lpr-1").len(), 1);
            // The FARP route still serves the other bound host, and the
            // re-registration replaced it instead of adding a second copy.
            assert_eq!(router.routes_by_id("farp:billing").len(), 2);
        }
    }

    #[tokio::test]
    async fn deregister_removes_service_routes_and_upstream() {
        let router = Arc::new(Router::new());
        let handler = bound_handler(&router);
        assert_eq!(push_register(&handler).await, StatusCode::CREATED);
        assert!(router.get_upstream("billing").is_some());

        let req = Request::builder()
            .method(Method::DELETE)
            .uri("/farp/deregister/billing-1")
            .body(Full::new(Bytes::new()))
            .unwrap();
        assert_eq!(handler.handle(req).await.unwrap().status(), StatusCode::OK);

        assert!(router
            .match_route("a.dev", &Method::GET, "/billing/invoices")
            .is_err());
        assert!(router.routes_by_id("farp:billing").is_empty());
        assert!(router.get_upstream("billing").is_none());
    }

    #[tokio::test]
    async fn deregister_service_by_name_removes_routes_and_upstream() {
        let router = Arc::new(Router::new());
        let handler = bound_handler(&router);
        assert_eq!(push_register(&handler).await, StatusCode::CREATED);

        let req = Request::builder()
            .method(Method::DELETE)
            .uri("/farp/services/billing")
            .body(Full::new(Bytes::new()))
            .unwrap();
        assert_eq!(handler.handle(req).await.unwrap().status(), StatusCode::OK);

        assert!(router.routes_by_id("farp:billing").is_empty());
        assert!(router.get_upstream("billing").is_none());
    }

    // Paused time lets the client's retry backoff elapse instantly.
    #[tokio::test(start_paused = true)]
    async fn push_without_manifest_uses_openapi_path_metadata() {
        let registry = Arc::new(SchemaRegistry::new());
        let handler = FarpApiHandler::new(Arc::clone(&registry));
        // Nothing listens on 127.0.0.1:1, so the manifest fetch fails at once
        // and the handler falls back to the opt-in metadata.
        let body = serde_json::json!({
            "instance": {
                "id": "web-1",
                "service_name": "web",
                "service_version": "2.0.0",
                "address": "127.0.0.1",
                "port": 1,
                "metadata": {"farp.openapi.path": "/openapi.json"}
            }
        });
        let req = Request::builder()
            .method(Method::POST)
            .uri("/farp/register")
            .body(Full::new(Bytes::from(body.to_string())))
            .unwrap();
        assert_eq!(
            handler.handle(req).await.unwrap().status(),
            StatusCode::CREATED
        );

        let registered = registry.get_service("web").unwrap();
        assert_eq!(registered.manifest.service_version, "2.0.0");
        assert_eq!(registered.manifest.schemas.len(), 1);
        assert_eq!(
            registered.manifest.schemas[0].location.url.as_deref(),
            Some("http://127.0.0.1:1/openapi.json")
        );
    }

    /// Push a registration with no manifest and metadata that has no
    /// `farp.openapi.path`.
    fn push_without_openapi_path() -> Request<Full<Bytes>> {
        let body = serde_json::json!({
            "instance": {
                "id": "web-1",
                "service_name": "web",
                "address": "127.0.0.1",
                "port": 1,
                "metadata": {"unrelated": "x"}
            }
        });
        Request::builder()
            .method(Method::POST)
            .uri("/farp/register")
            .body(Full::new(Bytes::from(body.to_string())))
            .unwrap()
    }

    // With the `validation` feature the empty manifest (no checksum) is
    // rejected, as it was before the metadata fallback existed.
    #[cfg(feature = "validation")]
    #[tokio::test(start_paused = true)]
    async fn push_without_manifest_or_openapi_path_is_rejected_with_validation() {
        let registry = Arc::new(SchemaRegistry::new());
        let handler = FarpApiHandler::new(Arc::clone(&registry));
        assert!(handler.handle(push_without_openapi_path()).await.is_err());
        assert!(registry.get_service("web").is_err());
    }

    // Without it the empty manifest registers and no schema is invented from
    // metadata that has no OpenAPI path.
    #[cfg(not(feature = "validation"))]
    #[tokio::test(start_paused = true)]
    async fn push_without_manifest_or_openapi_path_registers_the_empty_manifest() {
        let registry = Arc::new(SchemaRegistry::new());
        let handler = FarpApiHandler::new(Arc::clone(&registry));
        let res = handler.handle(push_without_openapi_path()).await.unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);
        assert!(registry
            .get_service("web")
            .unwrap()
            .manifest
            .schemas
            .is_empty());
    }

    #[tokio::test]
    async fn push_fallback_serves_routes_from_the_metadata_openapi_path() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_farp/manifest"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/openapi.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "openapi": "3.0.0",
                "info": {"title": "Web", "version": "1.0.0"},
                "paths": {"/items": {"get": {"summary": "List items"}}}
            })))
            .mount(&server)
            .await;

        let router = Arc::new(Router::new());
        let registry = Arc::new(SchemaRegistry::new());
        let mut handler =
            FarpApiHandler::new(Arc::clone(&registry)).with_router(Arc::clone(&router));
        // Retry the 404 manifest fetch without the default one second backoff.
        handler.farp_client = Arc::new(FarpClient::new(crate::client::FarpClientConfig {
            retry_backoff: std::time::Duration::from_millis(1),
            ..Default::default()
        }));

        let addr = server.address();
        let body = serde_json::json!({
            "instance": {
                "id": "Web-1",
                "service_name": "Web",
                "address": addr.ip().to_string(),
                "port": addr.port(),
                "metadata": {"farp.openapi.path": "/openapi.json"}
            },
            "manifest": null
        });
        let req = Request::builder()
            .method(Method::POST)
            .uri("/farp/register")
            .body(Full::new(Bytes::from(body.to_string())))
            .unwrap();
        assert_eq!(
            handler.handle(req).await.unwrap().status(),
            StatusCode::CREATED
        );

        let registered = registry.get_service("Web").unwrap();
        assert_eq!(
            registered.manifest.schemas[0].location.url,
            Some(format!("http://{addr}/openapi.json"))
        );
        // The OpenAPI document was fetched and turned into a FARP route under
        // the lowercase service prefix.
        let m = router
            .match_route("any.host", &Method::GET, "/web/items")
            .expect("FARP route for /web/items");
        assert_eq!(m.route.upstream_name, "Web");
        assert_eq!(m.route.origin, octopus_router::RouteOrigin::Farp);
        assert_eq!(m.route.id.as_deref(), Some("farp:Web"));
    }
}
