//! Spacer primitive for flexible spacing

use crate::core::{Node, Render};

/// Spacer primitive
#[derive(Debug, Clone)]
pub struct Spacer {
    class: String,
}

impl Spacer {
    /// Create a new spacer
    #[must_use]
    pub fn new() -> Self {
        Self {
            class: String::new(),
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
}

impl Default for Spacer {
    fn default() -> Self {
        Self::new()
    }
}

impl Render for Spacer {
    fn render(&self) -> Node {
        let mut classes = vec!["flex-1"];

        if !self.class.is_empty() {
            classes.push(&self.class);
        }

        Node::element("div").attr("class", classes.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spacer() {
        let spacer = Spacer::new();
        let html = spacer.render_to_string();
        assert!(html.contains("flex-1"));
    }
}
