//! Path matching utilities

use crate::route::Route;
use regex::Regex;
use std::collections::HashMap;

/// Result of a successful route match
#[derive(Debug, Clone)]
pub struct Match {
    /// The matched route
    pub route: Route,
    
    /// Extracted path parameters
    pub params: HashMap<String, String>,
    
    /// Wildcard match (if any)
    pub wildcard: Option<String>,
}

/// Path pattern matcher
#[derive(Debug)]
pub struct PathMatcher {
    /// Original pattern
    pattern: String,
    
    /// Compiled regex (if dynamic)
    regex: Option<Regex>,
    
    /// Parameter names in order
    param_names: Vec<String>,
    
    /// Is this a static path (no params)?
    is_static: bool,
    
    /// Has wildcard (*)?
    has_wildcard: bool,
}

impl PathMatcher {
    /// Create a new path matcher from a pattern
    ///
    /// Patterns:
    /// - `/users` - static path
    /// - `/users/:id` - dynamic path with parameter
    /// - `/users/:id/posts/:post_id` - multiple parameters
    /// - `/static/*filepath` - wildcard (must be at end)
    pub fn new(pattern: impl Into<String>) -> Self {
        let pattern = pattern.into();
        let mut param_names = Vec::new();
        let mut has_wildcard = false;
        let mut is_static = true;

        // Check for parameters and wildcards
        let segments: Vec<&str> = pattern.split('/').collect();
        for segment in &segments {
            if segment.starts_with(':') {
                is_static = false;
                param_names.push(segment[1..].to_string());
            } else if segment.starts_with('*') {
                is_static = false;
                has_wildcard = true;
                param_names.push(segment[1..].to_string());
            }
        }

        // Build regex for dynamic paths
        let regex = if !is_static {
            Some(Self::pattern_to_regex(&pattern))
        } else {
            None
        };

        Self {
            pattern,
            regex,
            param_names,
            is_static,
            has_wildcard,
        }
    }

    /// Convert path pattern to regex
    fn pattern_to_regex(pattern: &str) -> Regex {
        let mut regex_str = String::from("^");
        
        for segment in pattern.split('/') {
            if segment.is_empty() {
                continue;
            }
            
            regex_str.push('/');
            
            if segment.starts_with(':') {
                // Named parameter - match anything except /
                regex_str.push_str("([^/]+)");
            } else if segment.starts_with('*') {
                // Wildcard - match everything including /
                regex_str.push_str("(.*)");
            } else {
                // Static segment
                regex_str.push_str(&regex::escape(segment));
            }
        }
        
        regex_str.push('$');
        
        Regex::new(&regex_str).expect("Invalid regex pattern")
    }

    /// Match a path against this pattern
    pub fn matches(&self, path: &str) -> Option<HashMap<String, String>> {
        if self.is_static {
            // Fast path for static routes
            if path == self.pattern {
                Some(HashMap::new())
            } else {
                None
            }
        } else {
            // Dynamic matching with regex
            self.regex
                .as_ref()
                .and_then(|re| re.captures(path))
                .map(|captures| {
                    let mut params = HashMap::new();
                    
                    for (i, name) in self.param_names.iter().enumerate() {
                        if let Some(matched) = captures.get(i + 1) {
                            params.insert(name.clone(), matched.as_str().to_string());
                        }
                    }
                    
                    params
                })
        }
    }

    /// Get the pattern
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    /// Is this a static path?
    pub fn is_static(&self) -> bool {
        self.is_static
    }

    /// Has wildcard?
    pub fn has_wildcard(&self) -> bool {
        self.has_wildcard
    }

    /// Get parameter names
    pub fn param_names(&self) -> &[String] {
        &self.param_names
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_static_path() {
        let matcher = PathMatcher::new("/users");
        assert!(matcher.is_static());
        assert!(!matcher.has_wildcard());
        
        assert!(matcher.matches("/users").is_some());
        assert!(matcher.matches("/users/123").is_none());
    }

    #[test]
    fn test_single_param() {
        let matcher = PathMatcher::new("/users/:id");
        assert!(!matcher.is_static());
        assert_eq!(matcher.param_names(), &["id"]);
        
        let params = matcher.matches("/users/123").unwrap();
        assert_eq!(params.get("id"), Some(&"123".to_string()));
        
        assert!(matcher.matches("/users").is_none());
    }

    #[test]
    fn test_multiple_params() {
        let matcher = PathMatcher::new("/users/:user_id/posts/:post_id");
        assert_eq!(matcher.param_names(), &["user_id", "post_id"]);
        
        let params = matcher.matches("/users/42/posts/100").unwrap();
        assert_eq!(params.get("user_id"), Some(&"42".to_string()));
        assert_eq!(params.get("post_id"), Some(&"100".to_string()));
    }

    #[test]
    fn test_wildcard() {
        let matcher = PathMatcher::new("/static/*filepath");
        assert!(matcher.has_wildcard());
        assert_eq!(matcher.param_names(), &["filepath"]);
        
        let params = matcher.matches("/static/css/main.css").unwrap();
        assert_eq!(params.get("filepath"), Some(&"css/main.css".to_string()));
    }

    #[test]
    fn test_complex_pattern() {
        let matcher = PathMatcher::new("/api/v1/users/:id/documents/:doc_id");
        
        let params = matcher.matches("/api/v1/users/alice/documents/report").unwrap();
        assert_eq!(params.get("id"), Some(&"alice".to_string()));
        assert_eq!(params.get("doc_id"), Some(&"report".to_string()));
        
        assert!(matcher.matches("/api/v2/users/alice/documents/report").is_none());
    }

    #[test]
    fn test_no_match() {
        let matcher = PathMatcher::new("/users/:id");
        
        assert!(matcher.matches("/posts/123").is_none());
        assert!(matcher.matches("/users").is_none());
        assert!(matcher.matches("/users/123/extra").is_none());
    }
}


