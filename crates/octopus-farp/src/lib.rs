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
// Subjective pedantic/nursery/cargo lints are muted; substantive lints stay active.
#![allow(
    clippy::module_name_repetitions,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::must_use_candidate,
    clippy::missing_const_for_fn,
    clippy::option_if_let_else,
    clippy::similar_names,
    clippy::field_reassign_with_default,
    clippy::cognitive_complexity,
    clippy::too_many_lines,
    clippy::too_many_arguments,
    clippy::return_self_not_must_use,
    clippy::unnecessary_wraps,
    clippy::significant_drop_tightening,
    clippy::match_same_arms,
    clippy::manual_let_else,
    clippy::unused_self,
    clippy::unused_async,
    clippy::only_used_in_recursion,
    clippy::type_complexity,
    clippy::needless_pass_by_value,
    clippy::trivially_copy_pass_by_ref,
    clippy::missing_fields_in_debug,
    clippy::implicit_hasher,
    clippy::used_underscore_binding,
    clippy::struct_field_names,
    clippy::format_push_string,
    clippy::doc_markdown,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::multiple_crate_versions,
    clippy::cargo_common_metadata
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
pub use adapter::{EventType, ManifestEvent, RegistryAdapter, SchemaManifestAdapter};

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
    APIVersioningConfig, CORSConfig, CacheConfig, CircuitBreakerConfig as FarpCircuitBreakerConfig,
    CommunicationRouteType, DeploymentStrategy, GracefulShutdownConfig, InstanceMetadata,
    InstanceRole, LoadBalancingConfig, LoadBalancingStrategy, MiddlewareDeclaration, MountStrategy,
    ObservabilityConfig, RateLimitConfig as FarpRateLimitConfig, RateLimitKey,
    RateLimitStrategy as FarpRateLimitStrategy, RouteDescriptor,
    RouteMetadata as FarpRouteMetadata, RoutingConfig, StickySessionConfig, TracingConfig,
    TransformationConfig, VersionDeprecationPolicy, VersioningStrategy, WebhookConfig,
    WebhookEventType,
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
