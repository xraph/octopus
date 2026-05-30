//! Node type for representing HTML elements and content

use std::fmt;

/// Represents an HTML node (element, text, or group)
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    /// An HTML element with tag, attributes, and children
    Element {
        tag: String,
        attrs: Vec<(String, String)>,
        children: Vec<Self>,
        self_closing: bool,
    },
    /// Plain text content
    Text(String),
    /// Raw HTML (unescaped)
    Raw(String),
    /// Group of nodes (rendered sequentially without wrapper)
    Group(Vec<Self>),
    /// Empty node (renders nothing)
    Empty,
}

impl Node {
    /// Create a new element node
    #[must_use]
    pub fn element(tag: impl Into<String>) -> Self {
        Self::Element {
            tag: tag.into(),
            attrs: Vec::new(),
            children: Vec::new(),
            self_closing: false,
        }
    }

    /// Create a text node
    #[must_use]
    pub fn text(content: impl Into<String>) -> Self {
        Self::Text(content.into())
    }

    /// Create a raw HTML node (unescaped)
    #[must_use]
    pub fn raw(html: impl Into<String>) -> Self {
        Self::Raw(html.into())
    }

    /// Create a group of nodes
    #[must_use]
    pub const fn group(nodes: Vec<Self>) -> Self {
        Self::Group(nodes)
    }

    /// Create an empty node
    #[must_use]
    pub const fn empty() -> Self {
        Self::Empty
    }

    /// Add an attribute to an element
    #[must_use]
    pub fn attr(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        if let Self::Element { ref mut attrs, .. } = self {
            attrs.push((key.into(), value.into()));
        }
        self
    }

    /// Add a child node to an element
    #[must_use]
    pub fn child(mut self, node: Self) -> Self {
        if let Self::Element {
            ref mut children, ..
        } = self
        {
            children.push(node);
        }
        self
    }

    /// Add multiple child nodes to an element
    #[must_use]
    pub fn children(mut self, nodes: Vec<Self>) -> Self {
        if let Self::Element {
            ref mut children, ..
        } = self
        {
            children.extend(nodes);
        }
        self
    }

    /// Mark element as self-closing
    #[must_use]
    pub fn self_closing(mut self) -> Self {
        if let Self::Element {
            ref mut self_closing,
            ..
        } = self
        {
            *self_closing = true;
        }
        self
    }

    /// Render the node to HTML string
    #[must_use]
    pub fn render(&self) -> String {
        match self {
            Self::Element {
                tag,
                attrs,
                children,
                self_closing,
            } => {
                let mut html = format!("<{tag}");

                // Add attributes
                for (key, value) in attrs {
                    html.push_str(&format!(" {key}=\"{value}\""));
                }

                if *self_closing {
                    html.push_str(" />");
                } else {
                    html.push('>');

                    // Add children
                    for child in children {
                        html.push_str(&child.render());
                    }

                    html.push_str(&format!("</{tag}>"));
                }

                html
            }
            Self::Text(content) => super::escape_html(content),
            Self::Raw(html) => html.clone(),
            Self::Group(nodes) => nodes.iter().map(Self::render).collect::<String>(),
            Self::Empty => String::new(),
        }
    }
}

impl fmt::Display for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.render())
    }
}

impl From<&str> for Node {
    fn from(s: &str) -> Self {
        Self::text(s)
    }
}

impl From<String> for Node {
    fn from(s: String) -> Self {
        Self::text(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_element_creation() {
        let node = Node::element("div")
            .attr("class", "container")
            .child(Node::text("Hello"));

        assert_eq!(node.render(), "<div class=\"container\">Hello</div>");
    }

    #[test]
    fn test_self_closing_element() {
        let node = Node::element("img").attr("src", "image.jpg").self_closing();

        assert_eq!(node.render(), "<img src=\"image.jpg\" />");
    }

    #[test]
    fn test_group() {
        let group = Node::group(vec![
            Node::text("Hello"),
            Node::text(" "),
            Node::text("World"),
        ]);

        assert_eq!(group.render(), "Hello World");
    }

    #[test]
    fn test_empty() {
        let node = Node::empty();
        assert_eq!(node.render(), "");
    }
}
