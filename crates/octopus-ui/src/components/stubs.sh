#!/bin/bash
# Create stub component files

components=(
    "avatar" "alert" "separator" "empty_state" "list"
    "form" "label" "input" "textarea" "checkbox" "radio" "switch" "select" "slider"
    "navbar" "breadcrumb" "tabs" "menu" "sidebar" "pagination"
    "modal" "dialog" "drawer" "sheet" "dropdown" "popover" "tooltip" "toast"
    "spinner" "skeleton" "progress"
    "table"
)

for comp in "${components[@]}"; do
    cat > "/Users/rexraphael/Work/xraph/octopus/crates/octopus-ui/src/components/${comp}.rs" << COMPONENT
//! ${comp^} component

use crate::core::{Node, Render};

/// ${comp^} component
#[derive(Debug, Clone)]
pub struct ${comp^} {
    class: String,
    children: Vec<Node>,
}

impl ${comp^} {
    /// Create a new ${comp}
    #[must_use]
    pub fn new() -> Self {
        Self {
            class: String::new(),
            children: Vec::new(),
        }
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

    /// Add a child
    #[must_use]
    pub fn child(mut self, node: Node) -> Self {
        self.children.push(node);
        self
    }

    /// Add multiple children
    #[must_use]
    pub fn children(mut self, nodes: Vec<Node>) -> Self {
        self.children.extend(nodes);
        self
    }
}

impl Default for ${comp^} {
    fn default() -> Self {
        Self::new()
    }
}

impl Render for ${comp^} {
    fn render(&self) -> Node {
        let mut classes = vec!["${comp}"];

        if !self.class.is_empty() {
            classes.push(&self.class);
        }

        Node::element("div")
            .attr("class", classes.join(" "))
            .children(self.children.clone())
    }
}
COMPONENT
done
