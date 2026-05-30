//! Shared schema fetching and federation operations.
//!
//! These functions encapsulate the core FARP schema lifecycle:
//! fetch schema content from services, store in registry, and trigger federation.
//! Used by both `FarpApiHandler` (push registration) and `DiscoveryWatcher` (auto-discovery).

use crate::client::FarpClient;
use crate::federation::SchemaFederation;
use crate::manifest::SchemaManifest;
use crate::registry::SchemaRegistry;
use crate::schema::{SchemaDescriptor, SchemaFormat};
use crate::types::{LocationType, SchemaType};
use octopus_core::{Error, Result};
use tracing::{debug, info, warn};

/// Check if a path is a standard introspection endpoint that should be filtered
/// from federated specs by default.
pub fn is_introspection_path(path: &str) -> bool {
    path == "/" || path == "/_/info" || path == "/health"
        || path.starts_with("/docs") || path.starts_with("/openapi")
        || path.starts_with("/asyncapi")
        || path.starts_with("/_farp/")
}

/// Returns true if introspection endpoints should be excluded from federated specs.
/// By default they are excluded. Set `FARP_INCLUDE_INTROSPECTION_ENDPOINTS=1` to include them.
pub fn should_exclude_introspection() -> bool {
    std::env::var("FARP_INCLUDE_INTROSPECTION_ENDPOINTS").is_err()
}

/// Map a manifest `SchemaType` to a registry `SchemaFormat`.
fn schema_type_to_format(schema_type: &SchemaType) -> SchemaFormat {
    match schema_type {
        SchemaType::OpenAPI => SchemaFormat::OpenApi,
        SchemaType::AsyncAPI => SchemaFormat::AsyncApi,
        SchemaType::GRPC => SchemaFormat::Grpc,
        SchemaType::GraphQL => SchemaFormat::GraphQL,
        _ => SchemaFormat::Custom,
    }
}

/// Fetch a single schema from a URL, validate, and store in the registry.
pub async fn fetch_and_store_schema(
    client: &FarpClient,
    registry: &SchemaRegistry,
    service_name: &str,
    url: &str,
    format: SchemaFormat,
    spec_version: &str,
) -> Result<()> {
    debug!(service = %service_name, url = %url, "Fetching schema content");

    let content = client.fetch_schema(url).await?;

    // Validate JSON
    let _: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| Error::Farp(format!("Invalid schema JSON from {url}: {e}")))?;

    let mut schema = SchemaDescriptor::new(
        format!("{service_name}-{format:?}"),
        service_name,
        format,
        spec_version,
        content,
    );
    schema.calculate_checksum();

    registry.add_schema(service_name, schema)?;

    info!(service = %service_name, "Schema stored in registry");
    Ok(())
}

/// Iterate a manifest's schema descriptors, fetch each HTTP-located schema,
/// and store them in the registry.
pub async fn fetch_manifest_schemas(
    client: &FarpClient,
    registry: &SchemaRegistry,
    manifest: &SchemaManifest,
) {
    let service_name = &manifest.service_name;

    for manifest_schema in &manifest.schemas {
        if manifest_schema.location.location_type != LocationType::HTTP {
            continue;
        }

        if let Some(ref url) = manifest_schema.location.url {
            let format = schema_type_to_format(&manifest_schema.schema_type);

            if let Err(e) = fetch_and_store_schema(
                client,
                registry,
                service_name,
                url,
                format,
                &manifest_schema.spec_version,
            )
            .await
            {
                warn!(
                    service = %service_name,
                    url = %url,
                    error = %e,
                    "Failed to fetch schema content, skipping"
                );
            }
        }
    }
}

/// Collect all schemas from registered services and trigger federation.
pub fn trigger_federation(
    registry: &SchemaRegistry,
    federation: &SchemaFederation,
) -> Result<()> {
    debug!("Triggering schema federation");

    let service_names = registry.list_services();
    let mut all_schemas = Vec::new();

    for service_name in &service_names {
        if let Ok(registration) = registry.get_service(service_name) {
            all_schemas.extend(registration.schemas.clone());
        }
    }

    if all_schemas.is_empty() {
        debug!("No schemas to federate");
        return Ok(());
    }

    let schema_count = all_schemas.len();
    federation.federate_schemas(all_schemas)?;

    info!(schema_count = schema_count, "Schema federation completed");
    Ok(())
}
