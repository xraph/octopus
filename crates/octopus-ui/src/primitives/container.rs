//! Container primitive for responsive max-width layouts

use crate::core::{Node, Render};

/// Container primitive
#[derive(Debug, Clone)]
pub struct Container {
    class: String,
    children: Vec<Node>,
}

impl Container {
    /// Create a new container
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

impl Default for Container {
    fn default() -> Self {
        Self::new()
    }
}

impl Render for Container {
    fn render(&self) -> Node {
        let mut classes = vec!["container", "mx-auto", "px-4"];

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
    fn test_container() {
        let container = Container::new().child(Node::text("Content"));
        let html = container.render_to_string();
        assert!(html.contains("container"));
        assert!(html.contains("mx-auto"));
    }
}
