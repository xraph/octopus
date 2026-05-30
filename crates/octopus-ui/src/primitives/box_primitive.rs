//! Box primitive - polymorphic container component

use crate::core::{Node, Render};

/// Box primitive - a polymorphic container
#[derive(Debug, Clone)]
pub struct Box {
    /// HTML tag to use
    tag: String,
    /// CSS classes
    class: String,
    /// Children nodes
    children: Vec<Node>,
    /// Additional attributes
    attrs: Vec<(String, String)>,
}

impl Box {
    /// Create a new Box with default div tag
    #[must_use]
    pub fn new() -> Self {
        Self {
            tag: "div".to_string(),
            class: String::new(),
            children: Vec::new(),
            attrs: Vec::new(),
        }
    }

    /// Set the HTML tag
    #[must_use]
    pub fn tag(mut self, tag: impl Into<String>) -> Self {
        self.tag = tag.into();
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

    /// Add a child node
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

    /// Add an attribute
    #[must_use]
    pub fn attr(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attrs.push((key.into(), value.into()));
        self
    }
}

impl Default for Box {
    fn default() -> Self {
        Self::new()
    }
}

impl Render for Box {
    fn render(&self) -> Node {
        let mut node = Node::element(&self.tag);

        if !self.class.is_empty() {
            node = node.attr("class", &self.class);
        }

        for (key, value) in &self.attrs {
            node = node.attr(key, value);
        }

        node.children(self.children.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_box_default() {
        let b = Box::new().child(Node::text("Content"));
        assert_eq!(b.render_to_string(), "<div>Content</div>");
    }

    #[test]
    fn test_box_with_class() {
        let b = Box::new().class("container").child(Node::text("Content"));
        assert_eq!(
            b.render_to_string(),
            "<div class=\"container\">Content</div>"
        );
    }

    #[test]
    fn test_box_custom_tag() {
        let b = Box::new().tag("section").child(Node::text("Content"));
        assert_eq!(b.render_to_string(), "<section>Content</section>");
    }
}
