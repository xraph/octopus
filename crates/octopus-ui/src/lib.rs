#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![allow(
    clippy::module_name_repetitions,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc
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
//! let button = Button::new("Click me")
//!     .variant(Variant::Primary)
//!     .size(Size::Large)
//!     .build();
//!
//! println!("{}", button.render());
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
