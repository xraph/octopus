//! CVA (Class Variance Authority) - Type-safe variant management system
//!
//! Inspired by cva from JavaScript, this provides a type-safe way to manage
//! component variants and their associated CSS classes.

use std::collections::HashMap;

/// CVA builder for managing component variants
#[derive(Debug, Clone)]
pub struct CVA {
    /// Base classes always applied
    base: Vec<String>,
    /// Variant configurations
    variants: HashMap<String, HashMap<String, Vec<String>>>,
    /// Default values for variants
    defaults: HashMap<String, String>,
}

impl CVA {
    /// Create a new CVA instance with base classes
    #[must_use]
    pub fn new(base: &[&str]) -> Self {
        Self {
            base: base.iter().map(|s| (*s).to_string()).collect(),
            variants: HashMap::new(),
            defaults: HashMap::new(),
        }
    }

    /// Add a variant with its options
    #[must_use]
    pub fn variant(
        mut self,
        name: impl Into<String>,
        options: HashMap<String, Vec<String>>,
    ) -> Self {
        self.variants.insert(name.into(), options);
        self
    }

    /// Set a default value for a variant
    #[must_use]
    pub fn default(mut self, variant: impl Into<String>, value: impl Into<String>) -> Self {
        self.defaults.insert(variant.into(), value.into());
        self
    }

    /// Generate classes based on selected variants
    #[must_use]
    pub fn classes(&self, selections: &HashMap<String, String>) -> String {
        let mut classes = self.base.clone();

        // Apply variant classes
        for (variant_name, variant_options) in &self.variants {
            let selected = selections
                .get(variant_name)
                .or_else(|| self.defaults.get(variant_name));

            if let Some(selected_value) = selected {
                if let Some(variant_classes) = variant_options.get(selected_value) {
                    classes.extend(variant_classes.clone());
                }
            }
        }

        classes.join(" ")
    }

    /// Generate classes with a custom additional class
    #[must_use]
    pub fn classes_with(&self, selections: &HashMap<String, String>, additional: &str) -> String {
        let base_classes = self.classes(selections);
        if additional.is_empty() {
            base_classes
        } else {
            format!("{base_classes} {additional}")
        }
    }
}

/// Builder for creating CVA instances with a fluent API
pub struct CVABuilder {
    base: Vec<String>,
    variants: HashMap<String, HashMap<String, Vec<String>>>,
    defaults: HashMap<String, String>,
}

impl CVABuilder {
    /// Create a new CVA builder
    #[must_use]
    pub fn new() -> Self {
        Self {
            base: Vec::new(),
            variants: HashMap::new(),
            defaults: HashMap::new(),
        }
    }

    /// Add base classes
    #[must_use]
    pub fn base(mut self, classes: &[&str]) -> Self {
        self.base = classes.iter().map(|s| (*s).to_string()).collect();
        self
    }

    /// Add a variant
    #[must_use]
    pub fn variant(mut self, name: impl Into<String>, options: HashMap<&str, Vec<&str>>) -> Self {
        let converted_options: HashMap<String, Vec<String>> = options
            .into_iter()
            .map(|(k, v)| {
                (
                    k.to_string(),
                    v.into_iter().map(std::string::ToString::to_string).collect(),
                )
            })
            .collect();
        self.variants.insert(name.into(), converted_options);
        self
    }

    /// Set a default value for a variant
    #[must_use]
    pub fn default(mut self, variant: impl Into<String>, value: impl Into<String>) -> Self {
        self.defaults.insert(variant.into(), value.into());
        self
    }

    /// Build the CVA instance
    #[must_use]
    pub fn build(self) -> CVA {
        CVA {
            base: self.base,
            variants: self.variants,
            defaults: self.defaults,
        }
    }
}

impl Default for CVABuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cva_basic() {
        let cva = CVA::new(&["btn", "rounded"]);
        let classes = cva.classes(&HashMap::new());
        assert_eq!(classes, "btn rounded");
    }

    #[test]
    fn test_cva_with_variants() {
        let mut variant_options = HashMap::new();
        variant_options.insert("primary".to_string(), vec!["bg-blue-500".to_string()]);
        variant_options.insert("secondary".to_string(), vec!["bg-gray-500".to_string()]);

        let cva = CVA::new(&["btn"])
            .variant("variant", variant_options)
            .default("variant", "primary");

        let mut selections = HashMap::new();
        selections.insert("variant".to_string(), "primary".to_string());

        let classes = cva.classes(&selections);
        assert!(classes.contains("btn"));
        assert!(classes.contains("bg-blue-500"));
    }

    #[test]
    fn test_cva_with_default() {
        let mut variant_options = HashMap::new();
        variant_options.insert("sm".to_string(), vec!["text-sm".to_string()]);
        variant_options.insert("lg".to_string(), vec!["text-lg".to_string()]);

        let cva = CVA::new(&["btn"])
            .variant("size", variant_options)
            .default("size", "sm");

        // No selection, should use default
        let classes = cva.classes(&HashMap::new());
        assert!(classes.contains("text-sm"));
    }

    #[test]
    fn test_cva_builder() {
        let mut variant_options = HashMap::new();
        variant_options.insert("primary", vec!["bg-blue-500"]);

        let cva = CVABuilder::new()
            .base(&["btn"])
            .variant("variant", variant_options)
            .default("variant", "primary")
            .build();

        let classes = cva.classes(&HashMap::new());
        assert!(classes.contains("btn"));
        assert!(classes.contains("bg-blue-500"));
    }
}
