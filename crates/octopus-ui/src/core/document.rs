//! HTML document helpers for creating complete pages

use super::{Node, Render};

/// Create a complete HTML5 document
pub struct Document {
    title: String,
    head_nodes: Vec<Node>,
    body_nodes: Vec<Node>,
    body_class: String,
    html_class: String,
}

impl Document {
    /// Create a new document with a title
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            head_nodes: Vec::new(),
            body_nodes: Vec::new(),
            body_class: String::new(),
            html_class: String::new(),
        }
    }

    /// Add a node to the head
    pub fn head(mut self, node: Node) -> Self {
        self.head_nodes.push(node);
        self
    }

    /// Add a node to the body
    pub fn body(mut self, node: Node) -> Self {
        self.body_nodes.push(node);
        self
    }

    /// Set body class
    pub fn body_class(mut self, class: impl Into<String>) -> Self {
        self.body_class = class.into();
        self
    }

    /// Set html class
    pub fn html_class(mut self, class: impl Into<String>) -> Self {
        self.html_class = class.into();
        self
    }

    /// Add stylesheet link
    pub fn stylesheet(self, href: &str) -> Self {
        self.head(
            Node::element("link")
                .attr("rel", "stylesheet")
                .attr("href", href),
        )
    }

    /// Add script
    pub fn script(self, src: &str) -> Self {
        self.head(Node::element("script").attr("src", src).attr("defer", ""))
    }

    /// Add inline script
    pub fn inline_script(self, content: &str) -> Self {
        self.head(Node::element("script").child(Node::raw(content)))
    }

    /// Add meta tag
    pub fn meta(self, name: &str, content: &str) -> Self {
        self.head(
            Node::element("meta")
                .attr("name", name)
                .attr("content", content),
        )
    }

    /// Build the complete HTML document
    pub fn build(self) -> Node {
        let mut head = Node::element("head")
            .child(Node::element("meta").attr("charset", "UTF-8"))
            .child(
                Node::element("meta")
                    .attr("name", "viewport")
                    .attr("content", "width=device-width, initial-scale=1.0"),
            )
            .child(Node::element("title").child(Node::text(&self.title)));

        for node in self.head_nodes {
            head = head.child(node);
        }

        let mut body = Node::element("body");
        if !self.body_class.is_empty() {
            body = body.attr("class", &self.body_class);
        }
        for node in self.body_nodes {
            body = body.child(node);
        }

        let mut html = Node::element("html").attr("lang", "en");
        if !self.html_class.is_empty() {
            html = html.attr("class", &self.html_class);
        }

        Node::group(vec![
            Node::raw("<!DOCTYPE html>"),
            html.child(head).child(body),
        ])
    }

    /// Render to string
    pub fn render_to_string(self) -> String {
        self.build().render_to_string()
    }
}

/// Quick helper to create a document
pub fn document(title: impl Into<String>) -> Document {
    Document::new(title)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_document() {
        let doc = document("Test Page")
            .stylesheet("/styles.css")
            .body(Node::element("h1").child(Node::text("Hello")))
            .render_to_string();

        assert!(doc.contains("<!DOCTYPE html>"));
        assert!(doc.contains("<title>Test Page</title>"));
        assert!(doc.contains("stylesheet"));
        assert!(doc.contains("Hello"));
    }
}
