//! Utility functions for the core module

/// Escape HTML special characters
#[must_use]
pub fn escape_html(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '&' => "&amp;".to_string(),
            '<' => "&lt;".to_string(),
            '>' => "&gt;".to_string(),
            '"' => "&quot;".to_string(),
            '\'' => "&#39;".to_string(),
            _ => c.to_string(),
        })
        .collect()
}

/// Combine multiple class names, filtering out empty strings
#[must_use]
pub fn class_names(classes: &[&str]) -> String {
    classes
        .iter()
        .filter(|s| !s.is_empty())
        .map(|s| s.trim())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Merge class strings with deduplication
#[must_use]
pub fn merge_classes(base: &str, additional: &str) -> String {
    if additional.is_empty() {
        return base.to_string();
    }
    if base.is_empty() {
        return additional.to_string();
    }
    format!("{base} {additional}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_html() {
        assert_eq!(escape_html("<div>"), "&lt;div&gt;");
        assert_eq!(escape_html("&"), "&amp;");
        assert_eq!(escape_html("\"test\""), "&quot;test&quot;");
    }

    #[test]
    fn test_class_names() {
        assert_eq!(
            class_names(&["btn", "btn-primary", ""]),
            "btn btn-primary"
        );
        assert_eq!(class_names(&["", "", ""]), "");
    }

    #[test]
    fn test_merge_classes() {
        assert_eq!(merge_classes("btn", "btn-lg"), "btn btn-lg");
        assert_eq!(merge_classes("", "btn"), "btn");
        assert_eq!(merge_classes("btn", ""), "btn");
    }
}
