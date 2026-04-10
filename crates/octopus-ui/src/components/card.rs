//! Card component with compound parts (Header, Title, Description, Content, Footer)

use crate::core::{Node, Render};

/// Card component
#[derive(Debug, Clone)]
pub struct Card {
    class: String,
    children: Vec<Node>,
}

impl Card {
    /// Create a new card
    #[must_use]
    pub fn new() -> Self {
        Self {
            class: String::new(),
            children: Vec::new(),
        }
    }

    /// Add custom classes
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

impl Default for Card {
    fn default() -> Self {
        Self::new()
    }
}

impl Render for Card {
    fn render(&self) -> Node {
        let mut classes = vec![
            "rounded-lg",
            "border",
            "bg-card",
            "text-card-foreground",
            "shadow-sm",
        ];

        if !self.class.is_empty() {
            classes.push(&self.class);
        }

        Node::element("div")
            .attr("class", classes.join(" "))
            .children(self.children.clone())
    }
}

/// Card header
#[derive(Debug, Clone)]
pub struct CardHeader {
    class: String,
    children: Vec<Node>,
}

impl CardHeader {
    /// Create a new card header
    #[must_use]
    pub fn new() -> Self {
        Self {
            class: String::new(),
            children: Vec::new(),
        }
    }

    /// Add custom classes
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

impl Default for CardHeader {
    fn default() -> Self {
        Self::new()
    }
}

impl Render for CardHeader {
    fn render(&self) -> Node {
        let mut classes = vec!["flex", "flex-col", "space-y-1.5", "p-6"];

        if !self.class.is_empty() {
            classes.push(&self.class);
        }

        Node::element("div")
            .attr("class", classes.join(" "))
            .children(self.children.clone())
    }
}

/// Card title
#[derive(Debug, Clone)]
pub struct CardTitle {
    text: String,
    class: String,
}

impl CardTitle {
    /// Create a new card title
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            class: String::new(),
        }
    }

    /// Add custom classes
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

impl Render for CardTitle {
    fn render(&self) -> Node {
        let mut classes = vec!["text-2xl", "font-semibold", "leading-none", "tracking-tight"];

        if !self.class.is_empty() {
            classes.push(&self.class);
        }

        Node::element("h3")
            .attr("class", classes.join(" "))
            .child(Node::text(&self.text))
    }
}

/// Card description
#[derive(Debug, Clone)]
pub struct CardDescription {
    text: String,
    class: String,
}

impl CardDescription {
    /// Create a new card description
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            class: String::new(),
        }
    }

    /// Add custom classes
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

impl Render for CardDescription {
    fn render(&self) -> Node {
        let mut classes = vec!["text-sm", "text-muted-foreground"];

        if !self.class.is_empty() {
            classes.push(&self.class);
        }

        Node::element("p")
            .attr("class", classes.join(" "))
            .child(Node::text(&self.text))
    }
}

/// Card content
#[derive(Debug, Clone)]
pub struct CardContent {
    class: String,
    children: Vec<Node>,
}

impl CardContent {
    /// Create a new card content
    #[must_use]
    pub fn new() -> Self {
        Self {
            class: String::new(),
            children: Vec::new(),
        }
    }

    /// Add custom classes
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

impl Default for CardContent {
    fn default() -> Self {
        Self::new()
    }
}

impl Render for CardContent {
    fn render(&self) -> Node {
        let mut classes = vec!["p-6", "pt-0"];

        if !self.class.is_empty() {
            classes.push(&self.class);
        }

        Node::element("div")
            .attr("class", classes.join(" "))
            .children(self.children.clone())
    }
}

/// Card footer
#[derive(Debug, Clone)]
pub struct CardFooter {
    class: String,
    children: Vec<Node>,
}

impl CardFooter {
    /// Create a new card footer
    #[must_use]
    pub fn new() -> Self {
        Self {
            class: String::new(),
            children: Vec::new(),
        }
    }

    /// Add custom classes
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

impl Default for CardFooter {
    fn default() -> Self {
        Self::new()
    }
}

impl Render for CardFooter {
    fn render(&self) -> Node {
        let mut classes = vec!["flex", "items-center", "p-6", "pt-0"];

        if !self.class.is_empty() {
            classes.push(&self.class);
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
    fn test_card() {
        let card = Card::new()
            .child(CardHeader::new().child(CardTitle::new("Title").render()).render())
            .child(CardContent::new().child(Node::text("Content")).render());

        let html = card.render_to_string();
        assert!(html.contains("rounded-lg"));
        assert!(html.contains("Title"));
        assert!(html.contains("Content"));
    }
}
