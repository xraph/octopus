//! Authentication and authorization for Octopus API Gateway

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

pub mod provider;
pub mod rbac;
pub mod session;
pub mod token;

pub use provider::{AuthProvider, User, UserStore};
pub use rbac::{Permission, Role, RoleBasedAccessControl};
pub use session::{Session, SessionManager};
pub use token::{ApiKey, ApiKeyStore};

/// Re-export commonly used types
pub mod prelude {
    pub use crate::provider::{AuthProvider, User, UserStore};
    pub use crate::rbac::{Permission, Role, RoleBasedAccessControl};
    pub use crate::session::{Session, SessionManager};
    pub use crate::token::{ApiKey, ApiKeyStore};
}
