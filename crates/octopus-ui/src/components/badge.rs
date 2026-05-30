//! Badge component for status indicators and labels

use crate::core::{Node, Render, Variant};
use crate::cva::CVA;
use std::collections::HashMap;

/// Badge component
#[derive(Debug, Clone)]
pub struct Badge {
    variant: Variant,
    text: String,
    class: String,
}

impl Badge {
    /// Create a new badge
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            variant: Variant::Default,
            text: text.into(),
            class: String::new(),
        }
    }

    /// Set the variant
    #[must_use]
    pub fn variant(mut self, variant: Variant) -> Self {
        self.variant = variant;
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

    fn badge_cva() -> CVA {
        let mut variant_map = HashMap::new();
        variant_map.insert(
            "default".to_string(),
            vec![
                "border-transparent".to_string(),
                "bg-primary".to_string(),
                "text-primary-foreground".to_string(),
                "hover:bg-primary/80".to_string(),
            ],
        );
        variant_map.insert(
            "secondary".to_string(),
            vec![
                "border-transparent".to_string(),
                "bg-secondary".to_string(),
                "text-secondary-foreground".to_string(),
                "hover:bg-secondary/80".to_string(),
            ],
        );
        variant_map.insert(
            "destructive".to_string(),
            vec![
                "border-transparent".to_string(),
                "bg-destructive".to_string(),
                "text-destructive-foreground".to_string(),
                "hover:bg-destructive/80".to_string(),
            ],
        );
        variant_map.insert("outline".to_string(), vec!["text-foreground".to_string()]);

        CVA::new(&[
            "inline-flex",
            "items-center",
            "rounded-full",
            "border",
            "px-2.5",
            "py-0.5",
            "text-xs",
            "font-semibold",
            "transition-colors",
            "focus:outline-none",
            "focus:ring-2",
            "focus:ring-ring",
            "focus:ring-offset-2",
        ])
        .variant("variant", variant_map)
        .default("variant", "default")
    }
}

impl Render for Badge {
    fn render(&self) -> Node {
        let cva = Self::badge_cva();
        let mut selections = HashMap::new();
        selections.insert("variant".to_string(), self.variant.to_string());

        let classes = cva.classes_with(&selections, &self.class);

        Node::element("span")
            .attr("class", classes)
            .child(Node::text(&self.text))
    }
}
