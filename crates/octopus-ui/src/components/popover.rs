//! Popover component

use crate::core::{Node, Render};

/// Popover component
#[derive(Debug, Clone)]
pub struct Popover {
    class: String,
    children: Vec<Node>,
}

impl Popover {
    #[must_use]
    pub fn new() -> Self {
        Self {
            class: String::new(),
            children: Vec::new(),
        }
    }

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

    #[must_use]
    pub fn child(mut self, node: Node) -> Self {
        self.children.push(node);
        self
    }

    #[must_use]
    pub fn children(mut self, nodes: Vec<Node>) -> Self {
        self.children.extend(nodes);
        self
    }
}

impl Default for Popover {
    fn default() -> Self {
        Self::new()
    }
}

impl Render for Popover {
    fn render(&self) -> Node {
        let mut classes = vec!["popover"];
        if !self.class.is_empty() {
            classes.push(&self.class);
        }
        Node::element("div")
            .attr("class", classes.join(" "))
            .children(self.children.clone())
    }
}
