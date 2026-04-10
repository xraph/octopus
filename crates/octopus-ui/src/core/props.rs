//! Props trait for component configuration

use std::collections::HashMap;

/// Props trait for component configuration
pub trait Props {
    /// Get the class attribute value
    fn class(&self) -> String {
        String::new()
    }

    /// Get additional HTML attributes
    fn attrs(&self) -> HashMap<String, String> {
        HashMap::new()
    }

    /// Merge with additional classes
    fn with_class(&mut self, class: impl Into<String>);

    /// Add an attribute
    fn with_attr(&mut self, key: impl Into<String>, value: impl Into<String>);
}

/// Base props that can be embedded in component props
#[derive(Debug, Clone, Default)]
pub struct BaseProps {
    /// CSS classes
    pub class: String,
    /// HTML attributes
    pub attrs: HashMap<String, String>,
}

#[allow(dead_code)]
impl BaseProps {
    /// Create new base props
    #[must_use]
    pub fn new() -> Self {
        Self {
            class: String::new(),
            attrs: HashMap::new(),
        }
    }

    /// Add a class
    pub fn add_class(&mut self, class: impl Into<String>) {
        let new_class = class.into();
        if !new_class.is_empty() {
            if self.class.is_empty() {
                self.class = new_class;
            } else {
                self.class.push(' ');
                self.class.push_str(&new_class);
            }
        }
    }

    /// Add an attribute
    pub fn add_attr(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.attrs.insert(key.into(), value.into());
    }

    /// Get the combined class string
    #[must_use]
    pub fn class_string(&self) -> String {
        self.class.clone()
    }

    /// Get all attributes
    #[must_use]
    pub fn attributes(&self) -> &HashMap<String, String> {
        &self.attrs
    }
}

impl Props for BaseProps {
    fn class(&self) -> String {
        self.class.clone()
    }

    fn attrs(&self) -> HashMap<String, String> {
        self.attrs.clone()
    }

    fn with_class(&mut self, class: impl Into<String>) {
        self.add_class(class);
    }

    fn with_attr(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.add_attr(key, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base_props() {
        let mut props = BaseProps::new();
        props.add_class("btn");
        props.add_class("btn-primary");
        props.add_attr("id", "my-button");

        assert_eq!(props.class_string(), "btn btn-primary");
        assert_eq!(props.attributes().get("id"), Some(&"my-button".to_string()));
    }

    #[test]
    fn test_empty_class() {
        let mut props = BaseProps::new();
        props.add_class("");
        assert_eq!(props.class_string(), "");
    }
}
