//! Text primitive for typography

use crate::core::{Node, Render};

/// Text primitive
#[derive(Debug, Clone)]
pub struct Text {
    tag: String,
    class: String,
    content: String,
}

impl Text {
    /// Create a new text element
    #[must_use]
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            tag: "span".to_string(),
            class: String::new(),
            content: content.into(),
        }
    }

    /// Set the HTML tag (p, span, h1, h2, etc.)
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

    /// Set text size
    #[must_use]
    pub fn size(self, size: impl Into<String>) -> Self {
        self.class(format!("text-{}", size.into()))
    }

    /// Set text weight
    #[must_use]
    pub fn weight(self, weight: impl Into<String>) -> Self {
        self.class(format!("font-{}", weight.into()))
    }

    /// Set text color
    #[must_use]
    pub fn color(self, color: impl Into<String>) -> Self {
        self.class(format!("text-{}", color.into()))
    }
}

impl Render for Text {
    fn render(&self) -> Node {
        let mut node = Node::element(&self.tag).child(Node::text(&self.content));

        if !self.class.is_empty() {
            node = node.attr("class", &self.class);
        }

        node
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text() {
        let text = Text::new("Hello").size("lg").weight("bold");
        let html = text.render_to_string();
        assert!(html.contains("text-lg"));
        assert!(html.contains("font-bold"));
        assert!(html.contains("Hello"));
    }

    #[test]
    fn test_text_heading() {
        let text = Text::new("Title").tag("h1");
        assert_eq!(text.render_to_string(), "<h1>Title</h1>");
    }
}
