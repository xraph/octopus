//! FARP (Forge API Gateway Registration Protocol) implementation for Octopus
//!
//! This crate provides API Gateway integration for the FARP protocol, built on top
//! of the external `farp` crate for protocol-level types and abstractions.

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

// Adapter layer for bridging external farp crate with Octopus gateway
pub mod adapter;

pub mod api;
pub mod client;
pub mod discovery;
pub mod federation;
pub mod fetcher;
pub mod manifest;
pub mod registry;
pub mod route_generator;
pub mod schema;
pub mod schema_ops;
pub mod types;
pub mod validation;

// Re-export key adapter types (avoiding conflicts with local types)
pub use adapter::{
    EventType, ManifestEvent, RegistryAdapter, SchemaManifestAdapter,
};

pub use api::{FarpApiHandler, RegistrationRequest, RegistrationResponse};
pub use client::{FarpClient, FarpClientConfig};
pub use discovery::DiscoveryWatcher;
pub use federation::{FederatedSchema, SchemaFederation};
pub use fetcher::{RegistryBackend, SchemaFetcher};
pub use manifest::{
    calculate_schema_checksum, diff_manifests, ManifestDiff, SchemaChangeDiff, SchemaManifest,
};
pub use registry::{SchemaRegistry, ServiceRegistration};
pub use route_generator::{GeneratedRoute, RouteGenerator, RouteMetadata};
pub use schema::{SchemaDescriptor as LegacySchemaDescriptor, SchemaFormat, SchemaProvider};
pub use types::{
    Capability, LocationType, SchemaDescriptor, SchemaEndpoints, SchemaLocation, SchemaType,
    PROTOCOL_VERSION,
};
pub use validation::ManifestValidator;

// Re-export v1.1.0 types from external farp crate for gateway-level configuration
pub use farp::types::{
    RouteDescriptor, RateLimitConfig as FarpRateLimitConfig,
    CircuitBreakerConfig as FarpCircuitBreakerConfig, CORSConfig, CacheConfig,
    LoadBalancingConfig, LoadBalancingStrategy, MiddlewareDeclaration,
    TransformationConfig, APIVersioningConfig, ObservabilityConfig, TracingConfig,
    GracefulShutdownConfig, InstanceRole, DeploymentStrategy, InstanceMetadata,
    RouteMetadata as FarpRouteMetadata, RoutingConfig, MountStrategy,
    WebhookConfig, WebhookEventType, CommunicationRouteType,
    RateLimitStrategy as FarpRateLimitStrategy, RateLimitKey,
    StickySessionConfig, VersioningStrategy, VersionDeprecationPolicy,
};

/// Re-export commonly used types
pub mod prelude {
    pub use crate::api::{FarpApiHandler, RegistrationRequest, RegistrationResponse};
    pub use crate::client::{FarpClient, FarpClientConfig};
    pub use crate::federation::{FederatedSchema, SchemaFederation};
    pub use crate::manifest::{ManifestDiff, SchemaChangeDiff, SchemaManifest};
    pub use crate::registry::{SchemaRegistry, ServiceRegistration};
    pub use crate::route_generator::{GeneratedRoute, RouteGenerator, RouteMetadata};
    pub use crate::schema::{SchemaDescriptor, SchemaFormat, SchemaProvider};
}
