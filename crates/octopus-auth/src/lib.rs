//! Authentication and authorization for Octopus API Gateway
//!
//! Provides:
//! - Named auth provider registry with token caching
//! - JWT, OIDC, API key, forward auth, mTLS providers
//! - Authorization engine with Rhai scripts and OPA integration
//! - Legacy RBAC, session management, and API key store

#![warn(
    clippy::all,
    clippy::pedantic,
    clippy::nursery,
    clippy::cargo,
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

// Auth provider system
pub mod apikey_provider;
pub mod authz;
pub mod forward_provider;
pub mod jwt_provider;
pub mod mtls_provider;
pub mod oidc;
pub mod opa;
pub mod registry;

// Legacy modules (still functional)
pub mod provider;
pub mod rbac;
pub mod session;
pub mod token;

// Re-exports: new provider system
pub use authz::{AuthzEvaluator, RouteAuthzContext};
pub use opa::{AuthzContext, AuthzDecision, OpaClient};
pub use registry::{
    AuthProviderInstance, AuthProviderRegistry, AuthRequest, AuthResult, Principal,
};

// Re-exports: legacy
pub use provider::{AuthProvider, User, UserStore};
pub use rbac::{Permission, Role, RoleBasedAccessControl};
pub use session::{Session, SessionManager};
pub use token::{ApiKey, ApiKeyStore};

// Re-exports: providers
pub use apikey_provider::ApiKeyProvider;
pub use forward_provider::ForwardAuthProvider;
pub use jwt_provider::JwtProvider;
pub use mtls_provider::MtlsProvider;
pub use oidc::OidcProvider;
