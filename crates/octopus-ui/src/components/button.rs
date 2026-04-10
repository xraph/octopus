//! Button component with multiple variants and sizes

use crate::core::{Node, Render, Size, Variant};
use crate::cva::CVA;
use std::collections::HashMap;

/// Button component
#[derive(Debug, Clone)]
pub struct Button {
    variant: Variant,
    size: Size,
    button_type: String,
    disabled: bool,
    loading: bool,
    class: String,
    children: Vec<Node>,
    attrs: Vec<(String, String)>,
}

impl Button {
    /// Create a new button
    #[must_use]
    pub fn new() -> Self {
        Self {
            variant: Variant::Default,
            size: Size::MD,
            button_type: "button".to_string(),
            disabled: false,
            loading: false,
            class: String::new(),
            children: Vec::new(),
            attrs: Vec::new(),
        }
    }

    /// Create a button with text content
    #[must_use]
    pub fn with_text(text: impl Into<String>) -> Self {
        Self::new().child(Node::text(text))
    }

    /// Set the variant
    #[must_use]
    pub fn variant(mut self, variant: Variant) -> Self {
        self.variant = variant;
        self
    }

    /// Set the size
    #[must_use]
    pub fn size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }

    /// Set the button type
    #[must_use]
    pub fn button_type(mut self, button_type: impl Into<String>) -> Self {
        self.button_type = button_type.into();
        self
    }

    /// Set disabled state
    #[must_use]
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Set loading state
    #[must_use]
    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
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

    /// Get the button CVA
    fn button_cva() -> CVA {
        let mut variant_map = HashMap::new();
        variant_map.insert(
            "default".to_string(),
            vec![
                "bg-primary".to_string(),
                "text-primary-foreground".to_string(),
                "shadow-sm".to_string(),
                "hover:bg-primary/90".to_string(),
                "hover:shadow-md".to_string(),
            ],
        );
        variant_map.insert(
            "destructive".to_string(),
            vec![
                "bg-destructive".to_string(),
                "text-destructive-foreground".to_string(),
                "shadow-sm".to_string(),
                "hover:bg-destructive/90".to_string(),
                "hover:shadow-md".to_string(),
            ],
        );
        variant_map.insert(
            "outline".to_string(),
            vec![
                "border".to_string(),
                "border-input".to_string(),
                "bg-background".to_string(),
                "hover:bg-accent".to_string(),
                "hover:text-accent-foreground".to_string(),
                "hover:border-accent-foreground/20".to_string(),
            ],
        );
        variant_map.insert(
            "secondary".to_string(),
            vec![
                "bg-secondary".to_string(),
                "text-secondary-foreground".to_string(),
                "shadow-sm".to_string(),
                "hover:bg-secondary/80".to_string(),
            ],
        );
        variant_map.insert(
            "ghost".to_string(),
            vec![
                "hover:bg-accent".to_string(),
                "hover:text-accent-foreground".to_string(),
            ],
        );
        variant_map.insert(
            "link".to_string(),
            vec![
                "text-primary".to_string(),
                "underline-offset-4".to_string(),
                "hover:underline".to_string(),
            ],
        );

        let mut size_map = HashMap::new();
        size_map.insert(
            "sm".to_string(),
            vec![
                "h-8".to_string(),
                "rounded-md".to_string(),
                "gap-1.5".to_string(),
                "px-3".to_string(),
                "text-xs".to_string(),
            ],
        );
        size_map.insert(
            "md".to_string(),
            vec!["h-9".to_string(), "px-4".to_string(), "py-2".to_string()],
        );
        size_map.insert(
            "lg".to_string(),
            vec![
                "h-10".to_string(),
                "rounded-md".to_string(),
                "px-8".to_string(),
            ],
        );
        size_map.insert("icon".to_string(), vec!["size-9".to_string()]);
        size_map.insert("icon-sm".to_string(), vec!["size-8".to_string()]);
        size_map.insert("icon-lg".to_string(), vec!["size-10".to_string()]);

        CVA::new(&[
            "inline-flex",
            "items-center",
            "justify-center",
            "gap-2",
            "whitespace-nowrap",
            "rounded-md",
            "text-sm",
            "font-medium",
            "font-semibold",
            "transition-all",
            "duration-200",
            "shrink-0",
            "outline-none",
            "ring-offset-background",
            "focus-visible:outline-none",
            "focus-visible:ring-2",
            "focus-visible:ring-ring",
            "focus-visible:ring-offset-2",
            "disabled:pointer-events-none",
            "disabled:opacity-50",
        ])
        .variant("variant", variant_map)
        .variant("size", size_map)
        .default("variant", "default")
        .default("size", "md")
    }
}

impl Default for Button {
    fn default() -> Self {
        Self::new()
    }
}

impl Render for Button {
    fn render(&self) -> Node {
        let cva = Self::button_cva();
        let mut selections = HashMap::new();
        selections.insert("variant".to_string(), self.variant.to_string());
        selections.insert("size".to_string(), self.size.to_string());

        let classes = cva.classes_with(&selections, &self.class);

        let mut node = Node::element("button").attr("class", classes).attr("type", &self.button_type);

        if self.disabled || self.loading {
            node = node.attr("disabled", "");
        }

        if self.loading {
            node = node.attr("aria-busy", "true");
        }

        for (key, value) in &self.attrs {
            node = node.attr(key, value);
        }

        // Add spinner if loading
        let mut content = self.children.clone();
        if self.loading {
            content.insert(0, loading_spinner());
        }

        node.children(content)
    }
}

/// Create a loading spinner SVG
fn loading_spinner() -> Node {
    Node::element("svg")
        .attr("class", "animate-spin -ml-1 mr-2 h-4 w-4")
        .attr("xmlns", "http://www.w3.org/2000/svg")
        .attr("fill", "none")
        .attr("viewBox", "0 0 24 24")
        .child(
            Node::element("circle")
                .attr("class", "opacity-25")
                .attr("cx", "12")
                .attr("cy", "12")
                .attr("r", "10")
                .attr("stroke", "currentColor")
                .attr("stroke-width", "4"),
        )
        .child(
            Node::element("path")
                .attr("class", "opacity-75")
                .attr("fill", "currentColor")
                .attr(
                    "d",
                    "M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z",
                ),
        )
}

/// Button group for grouping buttons
#[derive(Debug, Clone)]
pub struct ButtonGroup {
    gap: String,
    class: String,
    children: Vec<Node>,
}

impl ButtonGroup {
    /// Create a new button group
    #[must_use]
    pub fn new() -> Self {
        Self {
            gap: "2".to_string(),
            class: String::new(),
            children: Vec::new(),
        }
    }

    /// Set gap between buttons
    #[must_use]
    pub fn gap(mut self, gap: impl Into<String>) -> Self {
        self.gap = gap.into();
        self
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

impl Default for ButtonGroup {
    fn default() -> Self {
        Self::new()
    }
}

impl Render for ButtonGroup {
    fn render(&self) -> Node {
        let mut classes = vec!["flex".to_string(), "items-center".to_string()];
        classes.push(format!("gap-{}", self.gap));

        if !self.class.is_empty() {
            classes.push(self.class.clone());
        }

        Node::element("div")
            .attr("class", classes.join(" "))
            .children(self.children.clone())
    }
}

/// Icon button (square button for icons)
#[derive(Debug, Clone)]
pub struct IconButton {
    inner: Button,
}

impl IconButton {
    /// Create a new icon button
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Button::new().size(Size::Icon),
        }
    }

    /// Set the variant
    #[must_use]
    pub fn variant(mut self, variant: Variant) -> Self {
        self.inner = self.inner.variant(variant);
        self
    }

    /// Set the size
    #[must_use]
    pub fn size(mut self, size: Size) -> Self {
        self.inner = self.inner.size(size);
        self
    }

    /// Set disabled state
    #[must_use]
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.inner = self.inner.disabled(disabled);
        self
    }

    /// Add custom classes
    #[must_use]
    pub fn class(mut self, class: impl Into<String>) -> Self {
        self.inner = self.inner.class(class);
        self
    }

    /// Add a child node
    #[must_use]
    pub fn child(mut self, node: Node) -> Self {
        self.inner = self.inner.child(node);
        self
    }

    /// Add an attribute
    #[must_use]
    pub fn attr(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.inner = self.inner.attr(key, value);
        self
    }
}

impl Default for IconButton {
    fn default() -> Self {
        Self::new()
    }
}

impl Render for IconButton {
    fn render(&self) -> Node {
        self.inner.render()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_button_basic() {
        let button = Button::with_text("Click me");
        let html = button.render_to_string();
        assert!(html.contains("button"));
        assert!(html.contains("Click me"));
    }

    #[test]
    fn test_button_variants() {
        let button = Button::with_text("Delete").variant(Variant::Destructive);
        let html = button.render_to_string();
        assert!(html.contains("bg-destructive"));
    }

    #[test]
    fn test_button_disabled() {
        let button = Button::with_text("Disabled").disabled(true);
        let html = button.render_to_string();
        assert!(html.contains("disabled"));
    }

    #[test]
    fn test_button_loading() {
        let button = Button::with_text("Loading").loading(true);
        let html = button.render_to_string();
        assert!(html.contains("aria-busy"));
        assert!(html.contains("animate-spin"));
    }

    #[test]
    fn test_button_group() {
        let group = ButtonGroup::new()
            .child(Button::with_text("Save").render())
            .child(Button::with_text("Cancel").render());
        let html = group.render_to_string();
        assert!(html.contains("flex"));
        assert!(html.contains("gap-2"));
    }

    #[test]
    fn test_icon_button() {
        let button = IconButton::new().child(Node::text("×"));
        let html = button.render_to_string();
        assert!(html.contains("size-9"));
    }
}
