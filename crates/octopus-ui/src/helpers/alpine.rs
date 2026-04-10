//! Alpine.js attribute helpers

/// Alpine.js helper for generating attributes
pub struct Alpine;

impl Alpine {
    /// Generate x-data attribute
    #[must_use]
    pub fn x_data(data: impl Into<String>) -> (String, String) {
        ("x-data".to_string(), data.into())
    }

    /// Generate x-on attribute
    #[must_use]
    pub fn x_on(event: impl Into<String>, handler: impl Into<String>) -> (String, String) {
        (format!("x-on:{}", event.into()), handler.into())
    }

    /// Generate @click shorthand
    #[must_use]
    pub fn at_click(handler: impl Into<String>) -> (String, String) {
        ("@click".to_string(), handler.into())
    }

    /// Generate x-show attribute
    #[must_use]
    pub fn x_show(condition: impl Into<String>) -> (String, String) {
        ("x-show".to_string(), condition.into())
    }

    /// Generate x-if attribute
    #[must_use]
    pub fn x_if(condition: impl Into<String>) -> (String, String) {
        ("x-if".to_string(), condition.into())
    }

    /// Generate x-text attribute
    #[must_use]
    pub fn x_text(expression: impl Into<String>) -> (String, String) {
        ("x-text".to_string(), expression.into())
    }

    /// Generate x-html attribute
    #[must_use]
    pub fn x_html(expression: impl Into<String>) -> (String, String) {
        ("x-html".to_string(), expression.into())
    }

    /// Generate x-model attribute
    #[must_use]
    pub fn x_model(property: impl Into<String>) -> (String, String) {
        ("x-model".to_string(), property.into())
    }

    /// Generate x-bind attribute
    #[must_use]
    pub fn x_bind(attribute: impl Into<String>, expression: impl Into<String>) -> (String, String) {
        (format!("x-bind:{}", attribute.into()), expression.into())
    }

    /// Generate :attribute shorthand
    #[must_use]
    pub fn bind(attribute: impl Into<String>, expression: impl Into<String>) -> (String, String) {
        (format!(":{}", attribute.into()), expression.into())
    }

    /// Generate x-init attribute
    #[must_use]
    pub fn x_init(expression: impl Into<String>) -> (String, String) {
        ("x-init".to_string(), expression.into())
    }

    /// Generate x-cloak attribute
    #[must_use]
    pub fn x_cloak() -> (String, String) {
        ("x-cloak".to_string(), String::new())
    }
}
