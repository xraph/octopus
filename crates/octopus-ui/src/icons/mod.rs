//! Icon system with SVG generation
//!
//! Provides a simple icon builder for creating SVG icons

use crate::core::{Node, Render};

/// Icon component
#[derive(Debug, Clone)]
pub struct Icon {
    #[allow(dead_code)]
    name: String,
    size: u32,
    color: Option<String>,
    stroke_width: u32,
    class: String,
}

impl Icon {
    /// Create a new icon
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            size: 24,
            color: None,
            stroke_width: 2,
            class: String::new(),
        }
    }

    /// Set icon size
    #[must_use]
    pub const fn size(mut self, size: u32) -> Self {
        self.size = size;
        self
    }

    /// Set icon color
    #[must_use]
    pub fn color(mut self, color: impl Into<String>) -> Self {
        self.color = Some(color.into());
        self
    }

    /// Set stroke width
    #[must_use]
    pub const fn stroke_width(mut self, width: u32) -> Self {
        self.stroke_width = width;
        self
    }

    /// Add custom classes
    #[must_use]
    pub fn class(mut self, class: impl Into<String>) -> Self {
        let new_class = class.into();
        if !new_class.is_empty() {
            if self.class.is_empty() {
                self.class = new_class;
            } else {
                self.class.push(' ');
                self.class.push_str(&new_class);
            }
        }
        self
    }
}

impl Render for Icon {
    fn render(&self) -> Node {
        let mut classes = vec!["icon"];
        if !self.class.is_empty() {
            classes.push(&self.class);
        }

        let mut svg = Node::element("svg")
            .attr("class", classes.join(" "))
            .attr("xmlns", "http://www.w3.org/2000/svg")
            .attr("width", self.size.to_string())
            .attr("height", self.size.to_string())
            .attr("viewBox", "0 0 24 24")
            .attr("fill", "none")
            .attr("stroke", "currentColor")
            .attr("stroke-width", self.stroke_width.to_string())
            .attr("stroke-linecap", "round")
            .attr("stroke-linejoin", "round");

        if let Some(ref color) = self.color {
            svg = svg.attr("color", color);
        }

        // For now, return a placeholder icon
        // In a full implementation, this would look up the icon path data
        svg.child(
            Node::element("circle")
                .attr("cx", "12")
                .attr("cy", "12")
                .attr("r", "10"),
        )
    }
}

// Common icon constructors
impl Icon {
    /// Check icon
    #[must_use]
    pub fn check() -> Self {
        Self::new("check")
    }

    /// X icon
    #[must_use]
    pub fn x() -> Self {
        Self::new("x")
    }

    /// Plus icon
    #[must_use]
    pub fn plus() -> Self {
        Self::new("plus")
    }

    /// Minus icon
    #[must_use]
    pub fn minus() -> Self {
        Self::new("minus")
    }

    /// Search icon
    #[must_use]
    pub fn search() -> Self {
        Self::new("search")
    }

    /// User icon
    #[must_use]
    pub fn user() -> Self {
        Self::new("user")
    }

    /// Settings icon
    #[must_use]
    pub fn settings() -> Self {
        Self::new("settings")
    }

    /// Loader icon (spinning)
    #[must_use]
    pub fn loader() -> Self {
        Self::new("loader").class("animate-spin")
    }
}
