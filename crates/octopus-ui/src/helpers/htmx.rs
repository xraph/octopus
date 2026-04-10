//! HTMX attribute helpers

/// HTMX helper for generating attributes
pub struct Htmx;

impl Htmx {
    /// Generate hx-get attribute
    #[must_use]
    pub fn hx_get(url: impl Into<String>) -> (String, String) {
        ("hx-get".to_string(), url.into())
    }

    /// Generate hx-post attribute
    #[must_use]
    pub fn hx_post(url: impl Into<String>) -> (String, String) {
        ("hx-post".to_string(), url.into())
    }

    /// Generate hx-put attribute
    #[must_use]
    pub fn hx_put(url: impl Into<String>) -> (String, String) {
        ("hx-put".to_string(), url.into())
    }

    /// Generate hx-delete attribute
    #[must_use]
    pub fn hx_delete(url: impl Into<String>) -> (String, String) {
        ("hx-delete".to_string(), url.into())
    }

    /// Generate hx-patch attribute
    #[must_use]
    pub fn hx_patch(url: impl Into<String>) -> (String, String) {
        ("hx-patch".to_string(), url.into())
    }

    /// Generate hx-target attribute
    #[must_use]
    pub fn hx_target(selector: impl Into<String>) -> (String, String) {
        ("hx-target".to_string(), selector.into())
    }

    /// Generate hx-swap attribute
    #[must_use]
    pub fn hx_swap(strategy: impl Into<String>) -> (String, String) {
        ("hx-swap".to_string(), strategy.into())
    }

    /// Generate hx-trigger attribute
    #[must_use]
    pub fn hx_trigger(event: impl Into<String>) -> (String, String) {
        ("hx-trigger".to_string(), event.into())
    }

    /// Generate hx-push-url attribute
    #[must_use]
    pub fn hx_push_url(value: impl Into<String>) -> (String, String) {
        ("hx-push-url".to_string(), value.into())
    }

    /// Generate hx-select attribute
    #[must_use]
    pub fn hx_select(selector: impl Into<String>) -> (String, String) {
        ("hx-select".to_string(), selector.into())
    }

    /// Generate hx-indicator attribute
    #[must_use]
    pub fn hx_indicator(selector: impl Into<String>) -> (String, String) {
        ("hx-indicator".to_string(), selector.into())
    }

    /// Generate hx-confirm attribute
    #[must_use]
    pub fn hx_confirm(message: impl Into<String>) -> (String, String) {
        ("hx-confirm".to_string(), message.into())
    }

    /// Generate hx-boost attribute
    #[must_use]
    pub fn hx_boost(value: bool) -> (String, String) {
        ("hx-boost".to_string(), value.to_string())
    }

    /// Generate hx-vals attribute
    #[must_use]
    pub fn hx_vals(json: impl Into<String>) -> (String, String) {
        ("hx-vals".to_string(), json.into())
    }
}
