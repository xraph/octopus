#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![allow(
    clippy::module_name_repetitions,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc
)]

//! Admin dashboard for Octopus API Gateway (Askama + HTMX + Alpine.js)
//!
//! This crate provides a server-side rendered admin dashboard with real-time updates.
//! Uses Askama for templates, HTMX for dynamic content, and Alpine.js for interactions.

pub mod api_handlers;
pub mod handlers;
pub mod models;
pub mod plugin;
pub mod router;

pub use api_handlers::*;
pub use handlers::*;
pub use models::*;
pub use plugin::*;
pub use router::DashboardRouter;

/// Custom Askama filters
pub mod filters {
    use serde::Serialize;

    /// Serialize a value to JSON for use in templates
    pub fn json<T: Serialize>(value: &T) -> askama::Result<String> {
        serde_json::to_string(value).map_err(|e| askama::Error::Custom(Box::new(e)))
    }
}
