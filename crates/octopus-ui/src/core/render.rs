//! Render trait for converting components to HTML

use super::Node;

/// Trait for types that can be rendered to HTML
pub trait Render {
    /// Render the component to an HTML Node
    fn render(&self) -> Node;

    /// Render the component to an HTML string
    fn render_to_string(&self) -> String {
        self.render().render()
    }
}

impl Render for Node {
    fn render(&self) -> Node {
        self.clone()
    }
}

impl Render for String {
    fn render(&self) -> Node {
        Node::text(self)
    }
}

impl Render for &str {
    fn render(&self) -> Node {
        Node::text(*self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_string() {
        let s = "Hello World";
        assert_eq!(s.render_to_string(), "Hello World");
    }

    #[test]
    fn test_render_node() {
        let node = Node::element("div").child(Node::text("Test"));
        assert_eq!(node.render_to_string(), "<div>Test</div>");
    }
}
