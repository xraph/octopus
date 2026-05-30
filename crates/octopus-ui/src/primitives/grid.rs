//! Grid primitive for CSS Grid layouts

use crate::core::{Node, Render};

/// Grid primitive
#[derive(Debug, Clone)]
pub struct Grid {
    cols: Option<u8>,
    gap: String,
    class: String,
    children: Vec<Node>,
}

impl Grid {
    /// Create a new grid
    #[must_use]
    pub const fn new() -> Self {
        Self {
            cols: None,
            gap: String::new(),
            class: String::new(),
            children: Vec::new(),
        }
    }

    /// Set number of columns
    #[must_use]
    pub const fn cols(mut self, cols: u8) -> Self {
        self.cols = Some(cols);
        self
    }

    /// Set gap between items
    #[must_use]
    pub fn gap(mut self, gap: impl Into<String>) -> Self {
        self.gap = gap.into();
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

impl Default for Grid {
    fn default() -> Self {
        Self::new()
    }
}

impl Render for Grid {
    fn render(&self) -> Node {
        let mut classes = vec!["grid".to_string()];

        if let Some(cols) = self.cols {
            classes.push(format!("grid-cols-{cols}"));
        }

        if !self.gap.is_empty() {
            classes.push(format!("gap-{}", self.gap));
        }

        if !self.class.is_empty() {
            classes.push(self.class.clone());
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
    fn test_grid() {
        let grid = Grid::new().cols(3).gap("4").child(Node::text("Item 1"));

        let html = grid.render_to_string();
        assert!(html.contains("grid"));
        assert!(html.contains("grid-cols-3"));
        assert!(html.contains("gap-4"));
    }
}
