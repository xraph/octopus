//! Scope resolution — maps routes to dot-separated namespace paths
//!
//! Scopes like `twinos.users.list` are resolved from:
//! 1. Manual mappings in octopus-gen.yaml (highest priority)
//! 2. OpenAPI operationId (e.g., "listUsers" → "users.list")
//! 3. Path + method heuristics (e.g., GET /api/v1/users → "users.list")

use std::collections::HashMap;

/// Resolve a scope path for a given route
pub fn resolve_scope(
    service_name: &str,
    path: &str,
    method: &str,
    operation_id: Option<&str>,
    manual_scopes: &HashMap<String, HashMap<String, String>>,
) -> String {
    // 1. Check manual mapping first
    if let Some(path_scopes) = manual_scopes.get(path) {
        if let Some(scope) = path_scopes.get(method) {
            return scope.clone();
        }
    }

    // 2. Try to derive from operationId
    if let Some(op_id) = operation_id {
        let scope = scope_from_operation_id(service_name, op_id);
        if !scope.is_empty() {
            return scope;
        }
    }

    // 3. Derive from path + method
    scope_from_path(service_name, path, method)
}

/// Derive scope from operationId
///
/// Examples:
///   "listUsers" → "service.users.list"
///   "getUserById" → "service.users.getById"
///   "createPost" → "service.posts.create"
///   "users_list" → "service.users.list"
fn scope_from_operation_id(service_name: &str, operation_id: &str) -> String {
    // Handle snake_case: "users_list" → ["users", "list"]
    if operation_id.contains('_') {
        let parts: Vec<&str> = operation_id.split('_').collect();
        if parts.len() >= 2 {
            return format!("{}.{}", service_name, parts.join("."));
        }
    }

    // Handle camelCase: "listUsers" → ["list", "Users"] → "users.list"
    let parts = split_camel_case(operation_id);
    if parts.len() >= 2 {
        // Common patterns: verbNoun → noun.verb
        let verb = parts[0].to_lowercase();
        let noun = parts[1..].join("").to_lowercase();

        // Pluralize common resource verbs
        let noun = if noun.ends_with('s') {
            noun
        } else {
            format!("{noun}s")
        };

        return format!("{service_name}.{noun}.{verb}");
    }

    format!("{service_name}.{}", operation_id.to_lowercase())
}

/// Derive scope from path + method
///
/// GET /api/v1/users → "service.users.list"
/// POST /api/v1/users → "service.users.create"
/// GET /api/v1/users/{id} → "service.users.get"
/// PUT /api/v1/users/{id} → "service.users.update"
/// DELETE /api/v1/users/{id} → "service.users.delete"
/// GET /api/v1/users/{userId}/posts → "service.users.posts.list"
fn scope_from_path(service_name: &str, path: &str, method: &str) -> String {
    // Strip common prefixes
    let clean_path = path.trim_start_matches('/').to_string();

    // Split path and extract resource segments (skip version prefixes and params)
    let segments: Vec<&str> = clean_path
        .split('/')
        .filter(|s| {
            !s.is_empty() && !s.starts_with('{') && !s.starts_with("api") && !is_version_segment(s)
        })
        .collect();

    if segments.is_empty() {
        return format!("{service_name}.root.{}", method_to_verb(method));
    }

    // Check if the last segment is a param (e.g., /users/{id})
    let path_parts: Vec<&str> = clean_path.split('/').filter(|s| !s.is_empty()).collect();
    let ends_with_param = path_parts
        .last()
        .map(|s| s.starts_with('{'))
        .unwrap_or(false);

    let verb = if ends_with_param {
        method_to_singular_verb(method)
    } else {
        method_to_verb(method)
    };

    let resource_path = segments.join(".");
    format!("{service_name}.{resource_path}.{verb}")
}

/// Map HTTP method to a verb for collection endpoints
fn method_to_verb(method: &str) -> &str {
    match method.to_uppercase().as_str() {
        "GET" => "list",
        "POST" => "create",
        "PUT" => "replaceAll",
        "DELETE" => "deleteAll",
        "PATCH" => "updateAll",
        _ => "call",
    }
}

/// Map HTTP method to a verb for single-resource endpoints
fn method_to_singular_verb(method: &str) -> &str {
    match method.to_uppercase().as_str() {
        "GET" => "get",
        "POST" => "create",
        "PUT" => "update",
        "DELETE" => "delete",
        "PATCH" => "patch",
        _ => "call",
    }
}

/// Check if a path segment is a version prefix (v1, v2, etc.)
fn is_version_segment(s: &str) -> bool {
    s.starts_with('v') && s[1..].chars().all(|c| c.is_ascii_digit())
}

/// Split a camelCase string into parts
fn split_camel_case(s: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();

    for ch in s.chars() {
        if ch.is_uppercase() && !current.is_empty() {
            parts.push(current.clone());
            current.clear();
        }
        current.push(ch);
    }

    if !current.is_empty() {
        parts.push(current);
    }

    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scope_from_path() {
        assert_eq!(
            scope_from_path("svc", "/api/v1/users", "GET"),
            "svc.users.list"
        );
        assert_eq!(
            scope_from_path("svc", "/api/v1/users", "POST"),
            "svc.users.create"
        );
        assert_eq!(
            scope_from_path("svc", "/api/v1/users/{id}", "GET"),
            "svc.users.get"
        );
        assert_eq!(
            scope_from_path("svc", "/api/v1/users/{id}", "PUT"),
            "svc.users.update"
        );
        assert_eq!(
            scope_from_path("svc", "/api/v1/users/{id}", "DELETE"),
            "svc.users.delete"
        );
        assert_eq!(
            scope_from_path("svc", "/api/v1/users/{userId}/posts", "GET"),
            "svc.users.posts.list"
        );
    }

    #[test]
    fn test_scope_from_operation_id() {
        assert_eq!(
            scope_from_operation_id("svc", "listUsers"),
            "svc.users.list"
        );
        assert_eq!(
            scope_from_operation_id("svc", "createUser"),
            "svc.users.create"
        );
        assert_eq!(
            scope_from_operation_id("svc", "users_list"),
            "svc.users.list"
        );
    }

    #[test]
    fn test_manual_scope_override() {
        let mut manual = HashMap::new();
        let mut path_scopes = HashMap::new();
        path_scopes.insert("GET".to_string(), "custom.scope.name".to_string());
        manual.insert("/users".to_string(), path_scopes);

        let scope = resolve_scope("svc", "/users", "GET", None, &manual);
        assert_eq!(scope, "custom.scope.name");
    }

    #[test]
    fn test_split_camel_case() {
        assert_eq!(split_camel_case("listUsers"), vec!["list", "Users"]);
        assert_eq!(
            split_camel_case("getUserById"),
            vec!["get", "User", "By", "Id"]
        );
    }
}
