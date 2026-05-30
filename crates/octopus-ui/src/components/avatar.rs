//! Avatar component

use crate::core::{Node, Render};

/// Avatar component
#[derive(Debug, Clone)]
pub struct Avatar {
    class: String,
    children: Vec<Node>,
}

impl Avatar {
    #[must_use]
    pub const fn new() -> Self {
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
}

impl Default for Avatar {
    fn default() -> Self {
        Self::new()
    }
}

impl Render for Avatar {
    fn render(&self) -> Node {
        let mut classes = vec![
            "relative",
            "flex",
            "h-10",
            "w-10",
            "shrink-0",
            "overflow-hidden",
            "rounded-full",
        ];
        if !self.class.is_empty() {
            classes.push(&self.class);
        }
        Node::element("div")
            .attr("class", classes.join(" "))
            .children(self.children.clone())
    }
}
