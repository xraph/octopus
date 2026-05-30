#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
// Subjective pedantic/nursery lints are muted; substantive lints stay active.
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
    clippy::cast_sign_loss
)]

//! Admin dashboard for Octopus API Gateway (Askama + HTMX + Alpine.js)
//!
//! This crate provides a server-side rendered admin dashboard with real-time updates.
//! Uses Askama for templates, HTMX for dynamic content, and Alpine.js for interactions.

pub mod api_handlers;
pub mod handlers;
pub mod models;
pub mod octopus_ui_handlers;
pub mod octopus_ui_handlers_pure;
pub mod plugin;
pub mod router;
pub mod ui_components;
pub mod websocket;

pub use api_handlers::*;
pub use handlers::*;
pub use models::*;
pub use plugin::*;
pub use router::DashboardRouter;
pub use websocket::{WsHub, WsMessage};

/// Custom Askama filters
pub mod filters {
    use serde::Serialize;

    /// Serialize a value to JSON for use in templates
    pub fn json<T: Serialize>(value: &T) -> askama::Result<String> {
        serde_json::to_string(value).map_err(|e| askama::Error::Custom(Box::new(e)))
    }
}
