//! Center primitive for centering content

use crate::core::{Node, Render};

/// Center primitive
#[derive(Debug, Clone)]
pub struct Center {
    class: String,
    children: Vec<Node>,
}

impl Center {
    /// Create a new center container
    #[must_use]
    pub fn new() -> Self {
        Self {
            class: String::new(),
            children: Vec::new(),
        }
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

impl Default for Center {
    fn default() -> Self {
        Self::new()
    }
}

impl Render for Center {
    fn render(&self) -> Node {
        let mut classes = vec!["flex", "items-center", "justify-center"];

        if !self.class.is_empty() {
            classes.push(&self.class);
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
    fn test_center() {
        let center = Center::new().child(Node::text("Centered"));
        let html = center.render_to_string();
        assert!(html.contains("flex"));
        assert!(html.contains("items-center"));
        assert!(html.contains("justify-center"));
    }
}
