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

//! # Octopus UI
//!
//! A type-safe UI component library for Rust web applications, inspired by forgeui.
//!
//! ## Features
//!
//! - **Type-Safe Builders**: Compile-time guarantees for component construction
//! - **CVA System**: Class Variance Authority for flexible variant management
//! - **35+ Components**: Production-ready UI components
//! - **Layout Primitives**: Container, Stack, Grid, Flex, and more
//! - **Icon System**: 1600+ Lucide icons with customization
//! - **Alpine.js/HTMX Helpers**: Attribute builders for interactivity
//! - **Theme System**: CSS variables and color token management
//!
//! ## Quick Start
//!
//! ```rust
//! use octopus_ui::prelude::*;
//!
//! let button = Button::with_text("Click me")
//!     .variant(Variant::Default)
//!     .size(Size::LG);
//!
//! println!("{}", button.render_to_string());
//! ```

// Core types and traits
pub mod core;
pub mod cva;

// Layout primitives
pub mod primitives;

// UI components
pub mod components;

// Icon system
pub mod icons;

// Helpers
pub mod helpers;

// Theme system
pub mod theme;

// Layouts
pub mod layouts;

// Prelude for common imports
pub mod prelude {
    pub use crate::components::*;
    pub use crate::core::{document, Document, Node, Props, Render, Size, Variant};
    pub use crate::cva::CVA;
    pub use crate::helpers::{alpine, htmx};
    pub use crate::icons::Icon;
    pub use crate::layouts::*;
    pub use crate::primitives::*;
    pub use crate::theme::Theme;
}
