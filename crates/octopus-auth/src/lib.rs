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
