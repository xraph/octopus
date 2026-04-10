//! Gomponents-inspired helpers for conditional rendering and mapping

use super::Node;

/// Conditional rendering - renders node if condition is true
#[must_use]
pub fn if_node(condition: bool, node: Node) -> Node {
    if condition {
        node
    } else {
        Node::empty()
    }
}

/// Conditional rendering with lazy evaluation - avoids evaluating expensive nodes
#[must_use]
pub fn if_lazy<F>(condition: bool, f: F) -> Node
where
    F: FnOnce() -> Node,
{
    if condition {
        f()
    } else {
        Node::empty()
    }
}

/// Map a collection to nodes
#[must_use]
pub fn map<T, F>(items: &[T], f: F) -> Node
where
    F: Fn(&T) -> Node,
{
    Node::group(items.iter().map(f).collect())
}

/// Map a collection with index
#[must_use]
pub fn map_indexed<T, F>(items: &[T], f: F) -> Node
where
    F: Fn(usize, &T) -> Node,
{
    Node::group(items.iter().enumerate().map(|(i, item)| f(i, item)).collect())
}

/// Conditional class helper - similar to gomponents Classes
#[derive(Debug, Clone, Default)]
pub struct Classes {
    classes: Vec<(String, bool)>,
}

impl Classes {
    /// Create a new Classes helper
    #[must_use]
    pub fn new() -> Self {
        Self {
            classes: Vec::new(),
        }
    }

    /// Add a class with a condition
    #[must_use]
    pub fn add(mut self, class: impl Into<String>, condition: bool) -> Self {
        self.classes.push((class.into(), condition));
        self
    }

    /// Build the final class string
    #[must_use]
    pub fn build(&self) -> String {
        self.classes
            .iter()
            .filter_map(|(class, condition)| {
                if *condition {
                    Some(class.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

impl From<Classes> for (String, String) {
    fn from(classes: Classes) -> Self {
        ("class".to_string(), classes.build())
    }
}

/// Fragment - render multiple nodes without a wrapper (alias for Group)
#[must_use]
pub fn fragment(nodes: Vec<Node>) -> Node {
    Node::group(nodes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_if_node() {
        let node = if_node(true, Node::text("Visible"));
        assert_eq!(node.render(), "Visible");

        let node = if_node(false, Node::text("Hidden"));
        assert_eq!(node.render(), "");
    }

    #[test]
    fn test_map() {
        let items = vec!["A", "B", "C"];
        let node = map(&items, |item| Node::text(item));
        assert_eq!(node.render(), "ABC");
    }

    #[test]
    fn test_classes() {
        let classes = Classes::new()
            .add("btn", true)
            .add("active", true)
            .add("disabled", false)
            .build();

        assert_eq!(classes, "btn active");
    }

    #[test]
    fn test_fragment() {
        let node = fragment(vec![
            Node::text("Hello"),
            Node::text(" "),
            Node::text("World"),
        ]);
        assert_eq!(node.render(), "Hello World");
    }
}
