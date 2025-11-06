//! FARP (Forge API Gateway Registration Protocol) implementation for Octopus

#![warn(
    clippy::all,
    clippy::pedantic,
    clippy::nursery,
    clippy::cargo,
    missing_docs,
    missing_debug_implementations,
    rust_2018_idioms,
    unreachable_pub
)]

pub mod api;
pub mod client;
pub mod discovery;
pub mod federation;
pub mod fetcher;
pub mod manifest;
pub mod registry;
pub mod route_generator;
pub mod schema;
pub mod types;
pub mod validation;

pub use api::{FarpApiHandler, RegistrationRequest, RegistrationResponse};
pub use client::{FarpClient, FarpClientConfig};
pub use discovery::DiscoveryWatcher;
pub use federation::{FederatedSchema, SchemaFederation};
pub use fetcher::{RegistryBackend, SchemaFetcher};
pub use manifest::{SchemaManifest, ManifestDiff, SchemaChangeDiff, calculate_schema_checksum, diff_manifests};
pub use registry::{SchemaRegistry, ServiceRegistration};
pub use route_generator::{GeneratedRoute, RouteGenerator, RouteMetadata};
pub use schema::{SchemaDescriptor as LegacySchemaDescriptor, SchemaFormat, SchemaProvider};
pub use types::{
    Capability, LocationType, SchemaDescriptor, SchemaEndpoints, SchemaLocation, SchemaType,
    PROTOCOL_VERSION,
};
pub use validation::ManifestValidator;

/// Re-export commonly used types
pub mod prelude {
    pub use crate::api::{FarpApiHandler, RegistrationRequest, RegistrationResponse};
    pub use crate::client::{FarpClient, FarpClientConfig};
    pub use crate::federation::{FederatedSchema, SchemaFederation};
    pub use crate::manifest::{SchemaManifest, ManifestDiff, SchemaChangeDiff};
    pub use crate::registry::{SchemaRegistry, ServiceRegistration};
    pub use crate::route_generator::{GeneratedRoute, RouteGenerator, RouteMetadata};
    pub use crate::schema::{SchemaDescriptor, SchemaFormat, SchemaProvider};
}

