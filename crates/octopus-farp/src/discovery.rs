//! Discovery integration for FARP
//!
//! Watches discovery backends and automatically registers discovered services

use crate::client::FarpClient;
use crate::federation::SchemaFederation;
use crate::manifest::SchemaManifest;
use crate::registry::SchemaRegistry;
use crate::schema::SchemaFormat;
use http::Method;
use octopus_core::{Error, Result, UpstreamCluster, UpstreamInstance};
use octopus_discovery::{DiscoveryProvider, ServiceInstance};
use octopus_router::{RouteBuilder, Router};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::time;
use tracing::{debug, error, info, warn};

/// Service tracking information
#[derive(Debug, Clone)]
struct TrackedService {
    instance_id: String,
    missed_count: u32,
}

/// Discovery watcher that monitors service discovery and registers services
pub struct DiscoveryWatcher {
    registry: Arc<SchemaRegistry>,
    providers: Vec<Arc<dyn DiscoveryProvider>>,
    watch_interval: Duration,
    /// Track which services we've discovered and registered
    tracked_services: Arc<tokio::sync::RwLock<HashMap<String, TrackedService>>>,
    /// Number of consecutive misses before deregistering (default: 3 = 15 seconds grace)
    max_missed_discoveries: u32,
    /// FARP client for fetching schemas
    farp_client: FarpClient,
    /// Schema federation for merging schemas
    federation: Arc<SchemaFederation>,
    /// Router for registering dynamic routes
    router: Option<Arc<Router>>,
    /// Cached routes checksums for atomic route swap detection (v1.1.0)
    routes_checksums: Arc<dashmap::DashMap<String, String>>,
    /// Readiness flag flipped to `true` after the first full discovery sync,
    /// so the gateway's `/readyz` probe can wait for discovery to converge.
    readiness_flag: Option<Arc<AtomicBool>>,
}

impl std::fmt::Debug for DiscoveryWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiscoveryWatcher")
            .field("registry", &self.registry)
            .field("provider_count", &self.providers.len())
            .field("watch_interval", &self.watch_interval)
            .field("max_missed_discoveries", &self.max_missed_discoveries)
            .field("farp_client", &self.farp_client)
            .field("federation", &self.federation)
            .field("router_configured", &self.router.is_some())
            .finish()
    }
}

impl DiscoveryWatcher {
    /// Create a new discovery watcher with default grace period (3 misses = 15 seconds)
    #[must_use]
    pub fn new(registry: Arc<SchemaRegistry>, watch_interval: Duration) -> Self {
        Self::with_grace_period(registry, watch_interval, 3)
    }

    /// Create a new discovery watcher with custom grace period
    ///
    /// `max_missed_discoveries`: Number of consecutive missed discoveries before deregistering
    /// For example, with 5s `watch_interval` and `max_missed=3`, a service must be missing for
    /// 15 seconds before being deregistered
    #[must_use]
    pub fn with_grace_period(
        registry: Arc<SchemaRegistry>,
        watch_interval: Duration,
        max_missed_discoveries: u32,
    ) -> Self {
        Self {
            registry,
            providers: Vec::new(),
            watch_interval,
            tracked_services: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            max_missed_discoveries,
            farp_client: FarpClient::default(),
            federation: Arc::new(SchemaFederation::new()),
            router: None,
            routes_checksums: Arc::new(dashmap::DashMap::new()),
            readiness_flag: None,
        }
    }

    /// Create a new discovery watcher with custom federation
    #[must_use]
    pub fn with_federation(
        registry: Arc<SchemaRegistry>,
        watch_interval: Duration,
        max_missed_discoveries: u32,
        federation: Arc<SchemaFederation>,
    ) -> Self {
        Self {
            registry,
            providers: Vec::new(),
            watch_interval,
            tracked_services: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            max_missed_discoveries,
            farp_client: FarpClient::default(),
            federation,
            router: None,
            routes_checksums: Arc::new(dashmap::DashMap::new()),
            readiness_flag: None,
        }
    }

    /// Set the router for dynamic route registration
    #[must_use]
    pub fn with_router(mut self, router: Arc<Router>) -> Self {
        self.router = Some(router);
        self
    }

    /// Set a readiness flag that is flipped to `true` after the first full
    /// discovery sync completes. Used to gate the gateway's `/readyz` probe so
    /// it only reports ready once discovery has converged.
    #[must_use]
    pub fn with_readiness_flag(mut self, flag: Arc<AtomicBool>) -> Self {
        self.readiness_flag = Some(flag);
        self
    }

    /// Add a discovery provider
    pub fn add_provider(&mut self, provider: Arc<dyn DiscoveryProvider>) {
        self.providers.push(provider);
    }

    /// Start watching for service changes
    pub async fn watch(self: Arc<Self>) -> Result<()> {
        if self.providers.is_empty() {
            warn!("No discovery providers configured, FARP discovery watcher is idle");
            return Ok(());
        }

        info!(
            "Starting FARP discovery watcher with {} provider(s)",
            self.providers.len()
        );

        let mut interval = time::interval(self.watch_interval);
        interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

        let mut first_pass = true;

        loop {
            interval.tick().await;

            for provider in &self.providers {
                if let Err(e) = self.sync_provider(provider.clone()).await {
                    error!(error = %e, "Failed to sync discovery provider");
                }
            }

            // After the first complete pass over all providers, signal readiness
            // so the gateway's `/readyz` probe can report ready. We flip even if
            // some providers errored — the system has converged as far as it can
            // and should accept traffic rather than stay NotReady indefinitely.
            if first_pass {
                first_pass = false;
                if let Some(ref flag) = self.readiness_flag {
                    flag.store(true, Ordering::Release);
                    info!("Initial discovery sync complete; gateway is ready");
                }
            }
        }
    }

    /// Synchronize services from a discovery provider
    async fn sync_provider(&self, provider: Arc<dyn DiscoveryProvider>) -> Result<()> {
        debug!("Syncing services from discovery provider");

        // Discover all services
        let instances = provider
            .discover_services()
            .await
            .map_err(|e| Error::Discovery(format!("Failed to discover services: {e}")))?;

        debug!(count = instances.len(), "Discovered services");

        // Track current instances from this sync
        let mut current_instance_ids = HashMap::new();

        for instance in instances {
            current_instance_ids.insert(instance.id.clone(), instance.name.clone());

            // Check if we've already registered this service
            let mut tracked = self.tracked_services.write().await;
            if let Some(tracked_service) = tracked.get_mut(&instance.name) {
                // Service found - reset missed count
                tracked_service.missed_count = 0;
                drop(tracked);

                // Check if schemas need refreshing (cache TTL expired)
                if self.registry.needs_schema_refresh(&instance.name) {
                    debug!(
                        service = %instance.name,
                        "Schema cache expired, refreshing from service"
                    );
                    if let Err(e) = self.refresh_service_schemas(&instance).await {
                        warn!(
                            service = %instance.name,
                            error = %e,
                            "Failed to refresh stale schemas"
                        );
                    }
                }

                continue;
            }
            drop(tracked);

            // Register the service
            if let Err(e) = self.register_discovered_service(&instance).await {
                error!(
                    service = %instance.name,
                    error = %e,
                    "Failed to register discovered service"
                );
            }
        }

        // Check for services that are no longer discovered
        // Increment their missed count and deregister if threshold exceeded
        let mut tracked = self.tracked_services.write().await;
        let mut to_remove = Vec::new();

        for (service_name, tracked_service) in tracked.iter_mut() {
            if !current_instance_ids.contains_key(&tracked_service.instance_id) {
                // Service not seen in this discovery cycle
                tracked_service.missed_count += 1;

                if tracked_service.missed_count >= self.max_missed_discoveries {
                    info!(
                        service = %service_name,
                        missed_count = tracked_service.missed_count,
                        max_allowed = self.max_missed_discoveries,
                        "Service exceeded missed discovery threshold, deregistering"
                    );
                    to_remove.push(service_name.clone());
                } else {
                    debug!(
                        service = %service_name,
                        missed_count = tracked_service.missed_count,
                        max_allowed = self.max_missed_discoveries,
                        "Service not discovered, incrementing missed count"
                    );
                }
            }
        }

        // Remove services that exceeded threshold
        for service_name in to_remove {
            if let Err(e) = self.registry.deregister_service(&service_name).await {
                error!(service = %service_name, error = %e, "Failed to deregister service");
            }
            tracked.remove(&service_name);
        }
        drop(tracked);

        Ok(())
    }

    /// Register a discovered service with FARP
    async fn register_discovered_service(&self, instance: &ServiceInstance) -> Result<()> {
        info!(
            service = %instance.name,
            address = %instance.address,
            "Registering discovered service with FARP"
        );

        // Build service base URL
        let base_url = format!("http://{}:{}", instance.address, instance.port);

        // Extract metadata
        let metadata = &instance.metadata;

        // Check if FARP is enabled via standard metadata key
        let farp_enabled = metadata
            .custom
            .get("farp.enabled")
            .is_some_and(|v| v == "true");

        if !farp_enabled {
            debug!(
                service = %instance.name,
                "Service does not have FARP enabled, skipping registration"
            );
            return Ok(());
        }

        // Extract version (FARP v1.0.0 standard or legacy)
        let version = metadata
            .version
            .clone()
            .or_else(|| metadata.custom.get("version").cloned())
            .unwrap_or_else(|| "unknown".to_string());

        // Create schema manifest using FARP v1.0.0 signature
        let mut manifest = SchemaManifest::new(instance.name.clone(), version, instance.id.clone());

        // Set up endpoints using FARP v1.0.0 standard metadata keys
        let health_endpoint = metadata
            .custom
            .get("farp.health.path")
            .or_else(|| metadata.custom.get("health"))
            .cloned()
            .unwrap_or_else(|| "/health".to_string());

        // Try FARP standard key first, then fall back to legacy
        let openapi_path = metadata
            .custom
            .get("farp.openapi.path")
            .or_else(|| metadata.custom.get("openapi"))
            .cloned()
            .unwrap_or_else(|| "/openapi.json".to_string());

        manifest.endpoints = crate::types::SchemaEndpoints {
            health: health_endpoint,
            metrics: metadata
                .custom
                .get("farp.metrics.path")
                .or_else(|| metadata.custom.get("metrics"))
                .cloned(),
            openapi: Some(openapi_path.clone()),
            asyncapi: metadata
                .custom
                .get("farp.asyncapi.path")
                .or_else(|| metadata.custom.get("asyncapi"))
                .cloned(),
            grpc_reflection: metadata
                .custom
                .get("farp.grpc.reflection")
                .or_else(|| metadata.custom.get("grpc_reflection"))
                .and_then(|v| v.parse().ok())
                .unwrap_or(false),
            graphql: metadata
                .custom
                .get("farp.graphql.path")
                .or_else(|| metadata.custom.get("graphql"))
                .cloned(),
        };

        // Parse capabilities from FARP metadata (format: "[rest websocket grpc]")
        if let Some(caps_str) = metadata.custom.get("farp.capabilities") {
            // Remove brackets and split by spaces
            let caps_str = caps_str.trim_matches(|c| c == '[' || c == ']');
            for cap in caps_str.split_whitespace() {
                manifest.add_capability(cap);
            }
        } else {
            // Default capability if not specified
            manifest.add_capability("rest");
        }

        // Add OpenAPI schema location if available
        manifest.add_openapi_http(&format!("{base_url}{openapi_path}"));

        // Update checksum
        manifest.update_checksum()?;

        // Register with FARP
        self.registry.register_service(manifest).await?;

        // Register upstream in router if router is configured
        if let Some(ref router) = self.router {
            self.register_upstream(router, &instance.name, &instance.address, instance.port)?;
        }

        // Fetch and store the actual OpenAPI schema content
        let openapi_url = format!("{base_url}{openapi_path}");
        match self
            .fetch_and_store_schema(&instance.name, &openapi_url)
            .await
        {
            Ok(()) => {
                info!(
                    service = %instance.name,
                    openapi_url = %openapi_url,
                    "Successfully fetched and stored OpenAPI schema"
                );

                // Register routes from OpenAPI schema
                if let Some(ref router) = self.router {
                    if let Err(e) = self
                        .register_routes_from_schema(router, &instance.name)
                        .await
                    {
                        warn!(
                            service = %instance.name,
                            error = %e,
                            "Failed to register routes from OpenAPI schema"
                        );
                    }
                }
            }
            Err(e) => {
                warn!(
                    service = %instance.name,
                    openapi_url = %openapi_url,
                    error = %e,
                    "Failed to fetch OpenAPI schema, service registered without schema content"
                );
                // Continue even if schema fetch fails - service is still registered
            }
        }

        // Track the service with zero missed count (freshly registered)
        let mut tracked = self.tracked_services.write().await;
        tracked.insert(
            instance.name.clone(),
            TrackedService {
                instance_id: instance.id.clone(),
                missed_count: 0,
            },
        );

        info!(
            service = %instance.name,
            base_url = %base_url,
            "Successfully registered discovered service"
        );

        Ok(())
    }

    /// Fetch `OpenAPI` schema from URL and store in registry
    async fn fetch_and_store_schema(&self, service_name: &str, url: &str) -> Result<()> {
        crate::schema_ops::fetch_and_store_schema(
            &self.farp_client,
            &self.registry,
            service_name,
            url,
            SchemaFormat::OpenApi,
            "3.0.0",
        )
        .await?;

        // Trigger federation to update the federated schema
        self.trigger_federation().await
    }

    /// Refresh schemas for an already-registered service (re-fetch from introspection endpoint)
    async fn refresh_service_schemas(&self, instance: &ServiceInstance) -> Result<()> {
        let base_url = format!("http://{}:{}", instance.address, instance.port);

        // Get the OpenAPI path from the existing registration's manifest
        let openapi_path = self
            .registry
            .get_service(&instance.name)
            .ok()
            .and_then(|reg| reg.manifest.endpoints.openapi)
            .unwrap_or_else(|| "/openapi.json".to_string());

        let openapi_url = format!("{base_url}{openapi_path}");

        info!(
            service = %instance.name,
            url = %openapi_url,
            "Refreshing stale schema cache"
        );

        // Clear existing schemas before re-fetch so add_schema resets updated_at
        if let Some(mut reg) = self.registry.services_mut().get_mut(&instance.name) {
            reg.schemas.clear();
        }

        // Re-fetch and store the schema (this calls add_schema which resets updated_at)
        self.fetch_and_store_schema(&instance.name, &openapi_url)
            .await?;

        // Re-generate routes if router is configured
        if let Some(ref router) = self.router {
            if let Err(e) = self
                .register_routes_from_schema(router, &instance.name)
                .await
            {
                warn!(
                    service = %instance.name,
                    error = %e,
                    "Failed to re-register routes after schema refresh"
                );
            }
        }

        info!(service = %instance.name, "Schema cache refreshed successfully");
        Ok(())
    }

    /// Trigger federation of all registered schemas
    async fn trigger_federation(&self) -> Result<()> {
        crate::schema_ops::trigger_federation(&self.registry, &self.federation)
    }

    /// Register upstream cluster for a discovered service
    fn register_upstream(
        &self,
        router: &Arc<Router>,
        service_name: &str,
        address: &str,
        port: u16,
    ) -> Result<()> {
        debug!(
            service = %service_name,
            address = %address,
            port = port,
            "Registering upstream cluster"
        );

        let mut cluster = UpstreamCluster::new(service_name);
        let instance = UpstreamInstance::new(format!("{service_name}-instance-1"), address, port);
        cluster.add_instance(instance);

        router.register_upstream(cluster);

        info!(
            service = %service_name,
            "Upstream cluster registered"
        );

        Ok(())
    }

    /// Register routes from `OpenAPI` schema or pre-computed route table
    async fn register_routes_from_schema(
        &self,
        router: &Arc<Router>,
        service_name: &str,
    ) -> Result<()> {
        debug!(
            service = %service_name,
            "Generating routes from schema or route_table"
        );

        // v1.1.0: Check routes checksum for atomic route swap detection
        if let Ok(registration) = self.registry.get_service(service_name) {
            if let Some(ref new_checksum) = registration.manifest.routes_checksum {
                if let Some(old) = self.routes_checksums.get(service_name) {
                    if old.value() == new_checksum {
                        debug!(
                            service = %service_name,
                            "Routes unchanged (checksum match), skipping"
                        );
                        return Ok(());
                    }
                }
                self.routes_checksums
                    .insert(service_name.to_string(), new_checksum.clone());
            }
        }

        // v1.1.0: Prefer route_table over schema parsing if available
        if let Ok(registration) = self.registry.get_service(service_name) {
            if registration.manifest.has_route_table() {
                let route_gen = crate::route_generator::RouteGenerator::new();
                let routes = route_gen
                    .generate_from_route_table(&registration.manifest.route_table, service_name);
                info!(
                    service = %service_name,
                    route_count = routes.len(),
                    "Generated routes from route_table (v1.1.0)"
                );
                // Apply auth config from manifest to generated routes
                let auth_config = registration.manifest.auth_config.as_ref();
                // Register each generated route with the router
                let service_prefix = format!("/{}", service_name.to_lowercase());
                for gen_route in &routes {
                    let mut builder = RouteBuilder::new()
                        .method(gen_route.method.parse().unwrap_or(Method::GET))
                        .path(format!("{}{}", service_prefix, gen_route.path))
                        .upstream_name(service_name)
                        .strip_prefix(&service_prefix)
                        .priority(100);

                    // Apply auth config from manifest
                    if let Some(auth) = auth_config {
                        builder = builder
                            .auth_provider(auth.auth_provider.as_deref())
                            .require_roles(&auth.require_roles)
                            .require_scopes(&auth.require_scopes);
                    }

                    let route = builder.build();
                    if let Ok(route) = route {
                        if let Err(e) = router.add_route(route) {
                            warn!(error = %e, "Failed to register route from route_table");
                        }
                    }
                }
                return Ok(());
            }
        }

        // Extract auth config from manifest for OpenAPI fallback routes
        let manifest_auth_config = self
            .registry
            .get_service(service_name)
            .ok()
            .and_then(|r| r.manifest.auth_config);

        // Get the schema from registry
        let schemas = self.registry.get_schemas(service_name)?;

        // Find OpenAPI schema
        let openapi_schema = schemas
            .iter()
            .find(|s| matches!(s.format, SchemaFormat::OpenApi))
            .ok_or_else(|| {
                Error::Farp(format!(
                    "No OpenAPI schema found for service {service_name}"
                ))
            })?;

        // Parse the OpenAPI schema
        let schema_json: serde_json::Value = serde_json::from_str(&openapi_schema.content)
            .map_err(|e| Error::Farp(format!("Failed to parse OpenAPI schema: {e}")))?;

        // Extract paths from OpenAPI schema
        let paths = schema_json
            .get("paths")
            .and_then(|p| p.as_object())
            .ok_or_else(|| Error::Farp("No paths found in OpenAPI schema".to_string()))?;

        let mut routes_added = 0;

        // Create routes for each path
        for (path, operations) in paths {
            // Filter out standard introspection endpoints by default.
            // Set FARP_INCLUDE_INTROSPECTION_ENDPOINTS=1 to include them.
            if crate::schema_ops::should_exclude_introspection()
                && crate::schema_ops::is_introspection_path(path.as_str())
            {
                continue;
            }

            let operations = operations
                .as_object()
                .ok_or_else(|| Error::Farp(format!("Invalid operations for path {path}")))?;

            // For each HTTP method in the path
            for (method_str, _operation) in operations {
                // Skip non-HTTP methods (like "parameters", "servers", etc.)
                let method = match method_str.to_uppercase().as_str() {
                    "GET" => Method::GET,
                    "POST" => Method::POST,
                    "PUT" => Method::PUT,
                    "DELETE" => Method::DELETE,
                    "PATCH" => Method::PATCH,
                    "HEAD" => Method::HEAD,
                    "OPTIONS" => Method::OPTIONS,
                    _ => continue, // Skip non-standard methods
                };

                // Create route with lowercase service prefix
                let service_prefix = format!("/{}", service_name.to_lowercase());
                let prefixed_path = format!("{service_prefix}{path}");

                let mut builder = RouteBuilder::new()
                    .path(&prefixed_path)
                    .method(method.clone())
                    .upstream_name(service_name)
                    .strip_prefix(&service_prefix)
                    .priority(100); // Default priority for FARP routes

                // Apply auth config from manifest
                if let Some(ref auth) = manifest_auth_config {
                    builder = builder
                        .auth_provider(auth.auth_provider.as_deref())
                        .require_roles(&auth.require_roles)
                        .require_scopes(&auth.require_scopes);
                }

                let route = builder.build()?;

                // Register the route
                if let Err(e) = router.add_route(route) {
                    warn!(
                        service = %service_name,
                        path = %prefixed_path,
                        method = %method,
                        error = %e,
                        "Failed to register route (may already exist)"
                    );
                } else {
                    debug!(
                        service = %service_name,
                        path = %prefixed_path,
                        method = %method,
                        "Route registered"
                    );
                    routes_added += 1;
                }
            }
        }

        info!(
            service = %service_name,
            routes_added = routes_added,
            "Routes registered from OpenAPI schema"
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use octopus_discovery::{ServiceHealth, ServiceMetadata};
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_discovery_watcher_creation() {
        let registry = Arc::new(SchemaRegistry::new());
        let watcher = DiscoveryWatcher::new(registry, Duration::from_secs(5));

        assert_eq!(watcher.providers.len(), 0);
    }

    #[tokio::test]
    async fn test_register_discovered_service() {
        let registry = Arc::new(SchemaRegistry::new());
        let watcher = DiscoveryWatcher::new(registry.clone(), Duration::from_secs(5));

        let mut custom_metadata = HashMap::new();
        custom_metadata.insert("version".to_string(), "1.0.0".to_string());
        custom_metadata.insert("openapi".to_string(), "/api/openapi.json".to_string());
        custom_metadata.insert("health".to_string(), "/health".to_string());
        custom_metadata.insert("farp.enabled".to_string(), "true".to_string());

        let metadata = ServiceMetadata {
            version: Some("1.0.0".to_string()),
            tags: vec![],
            datacenter: None,
            custom: custom_metadata,
        };

        let instance = ServiceInstance {
            id: "test-1".to_string(),
            name: "test-service".to_string(),
            address: "127.0.0.1".to_string(),
            port: 8080,
            health: ServiceHealth::Healthy,
            metadata,
            endpoints: vec![],
        };

        watcher
            .register_discovered_service(&instance)
            .await
            .unwrap();

        // Verify service was registered
        assert_eq!(registry.service_count(), 1);
        let service = registry.get_service("test-service").unwrap();
        assert_eq!(service.service_name, "test-service");
        assert_eq!(service.manifest.service_name, "test-service");
        assert_eq!(service.manifest.instance_id, "test-1");
    }
}
