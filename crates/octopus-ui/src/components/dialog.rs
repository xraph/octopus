//! Dialog component

use crate::core::{Node, Render};

/// Dialog component
#[derive(Debug, Clone)]
pub struct Dialog {
    class: String,
    children: Vec<Node>,
}

impl Dialog {
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

impl Default for Dialog {
    fn default() -> Self {
        Self::new()
    }
}

impl Render for Dialog {
    fn render(&self) -> Node {
        let mut classes = vec!["dialog"];
        if !self.class.is_empty() {
            classes.push(&self.class);
        }
        Node::element("div")
            .attr("class", classes.join(" "))
            .children(self.children.clone())
    }
}
