//! Flex primitive for flexbox layouts

use crate::core::{Node, Render};

/// Flex primitive
#[derive(Debug, Clone)]
pub struct Flex {
    direction: Option<String>,
    justify: Option<String>,
    align: Option<String>,
    gap: String,
    class: String,
    children: Vec<Node>,
}

impl Flex {
    /// Create a new flex container
    #[must_use]
    pub fn new() -> Self {
        Self {
            direction: None,
            justify: None,
            align: None,
            gap: String::new(),
            class: String::new(),
            children: Vec::new(),
        }
    }

    /// Set flex direction
    #[must_use]
    pub fn direction(mut self, direction: impl Into<String>) -> Self {
        self.direction = Some(direction.into());
        self
    }

    /// Set justify content
    #[must_use]
    pub fn justify(mut self, justify: impl Into<String>) -> Self {
        self.justify = Some(justify.into());
        self
    }

    /// Set align items
    #[must_use]
    pub fn align(mut self, align: impl Into<String>) -> Self {
        self.align = Some(align.into());
        self
    }

    /// Set gap
    #[must_use]
    pub fn gap(mut self, gap: impl Into<String>) -> Self {
        self.gap = gap.into();
        self
    }

    /// Add CSS classes
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

impl Default for Flex {
    fn default() -> Self {
        Self::new()
    }
}

impl Render for Flex {
    fn render(&self) -> Node {
        let mut classes = vec!["flex".to_string()];

        if let Some(ref direction) = self.direction {
            classes.push(format!("flex-{direction}"));
        }

        if let Some(ref justify) = self.justify {
            classes.push(format!("justify-{justify}"));
        }

        if let Some(ref align) = self.align {
            classes.push(format!("items-{align}"));
        }

        if !self.gap.is_empty() {
            classes.push(format!("gap-{}", self.gap));
        }

        if !self.class.is_empty() {
            classes.push(self.class.clone());
        }

        Node::element("div")
            .attr("class", classes.join(" "))
            .children(self.children.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flex() {
        let flex = Flex::new()
            .direction("row")
            .justify("center")
            .align("center")
            .gap("4")
            .child(Node::text("Item"));

        let html = flex.render_to_string();
        assert!(html.contains("flex"));
        assert!(html.contains("flex-row"));
        assert!(html.contains("justify-center"));
        assert!(html.contains("items-center"));
    }
}
