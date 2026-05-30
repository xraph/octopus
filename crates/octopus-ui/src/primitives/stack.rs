//! Stack primitives for vertical and horizontal layouts

use crate::core::{Node, Render};

/// Stack direction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StackDirection {
    /// Vertical stack
    Vertical,
    /// Horizontal stack
    Horizontal,
}

/// Stack primitive for vertical/horizontal layouts
#[derive(Debug, Clone)]
pub struct Stack {
    direction: StackDirection,
    gap: String,
    class: String,
    children: Vec<Node>,
}

impl Stack {
    /// Create a new stack
    #[must_use]
    pub const fn new(direction: StackDirection) -> Self {
        Self {
            direction,
            gap: String::new(),
            class: String::new(),
            children: Vec::new(),
        }
    }

    /// Set gap between items (Tailwind spacing scale)
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

impl Render for Stack {
    fn render(&self) -> Node {
        let mut classes = vec!["flex".to_string()];

        match self.direction {
            StackDirection::Vertical => classes.push("flex-col".to_string()),
            StackDirection::Horizontal => classes.push("flex-row".to_string()),
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

/// Vertical stack
#[derive(Debug, Clone)]
pub struct VStack {
    inner: Stack,
}

impl VStack {
    /// Create a new vertical stack
    #[must_use]
    pub const fn new() -> Self {
        Self {
            inner: Stack::new(StackDirection::Vertical),
        }
    }

    /// Set gap
    #[must_use]
    pub fn gap(mut self, gap: impl Into<String>) -> Self {
        self.inner = self.inner.gap(gap);
        self
    }

    /// Add class
    #[must_use]
    pub fn class(mut self, class: impl Into<String>) -> Self {
        self.inner = self.inner.class(class);
        self
    }

    /// Add child
    #[must_use]
    pub fn child(mut self, node: Node) -> Self {
        self.inner = self.inner.child(node);
        self
    }

    /// Add children
    #[must_use]
    pub fn children(mut self, nodes: Vec<Node>) -> Self {
        self.inner = self.inner.children(nodes);
        self
    }
}

impl Default for VStack {
    fn default() -> Self {
        Self::new()
    }
}

impl Render for VStack {
    fn render(&self) -> Node {
        self.inner.render()
    }
}

/// Horizontal stack
#[derive(Debug, Clone)]
pub struct HStack {
    inner: Stack,
}

impl HStack {
    /// Create a new horizontal stack
    #[must_use]
    pub const fn new() -> Self {
        Self {
            inner: Stack::new(StackDirection::Horizontal),
        }
    }

    /// Set gap
    #[must_use]
    pub fn gap(mut self, gap: impl Into<String>) -> Self {
        self.inner = self.inner.gap(gap);
        self
    }

    /// Add class
    #[must_use]
    pub fn class(mut self, class: impl Into<String>) -> Self {
        self.inner = self.inner.class(class);
        self
    }

    /// Add child
    #[must_use]
    pub fn child(mut self, node: Node) -> Self {
        self.inner = self.inner.child(node);
        self
    }

    /// Add children
    #[must_use]
    pub fn children(mut self, nodes: Vec<Node>) -> Self {
        self.inner = self.inner.children(nodes);
        self
    }
}

impl Default for HStack {
    fn default() -> Self {
        Self::new()
    }
}

impl Render for HStack {
    fn render(&self) -> Node {
        self.inner.render()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vstack() {
        let stack = VStack::new()
            .gap("4")
            .child(Node::text("Item 1"))
            .child(Node::text("Item 2"));

        let html = stack.render_to_string();
        assert!(html.contains("flex-col"));
        assert!(html.contains("gap-4"));
    }

    #[test]
    fn test_hstack() {
        let stack = HStack::new()
            .gap("2")
            .child(Node::text("Item 1"))
            .child(Node::text("Item 2"));

        let html = stack.render_to_string();
        assert!(html.contains("flex-row"));
        assert!(html.contains("gap-2"));
    }
}
