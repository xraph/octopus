//! Role-Based Access Control (RBAC) for Octopus Gateway

use dashmap::DashMap;
use octopus_core::{Error, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Permission represents an action that can be performed on a resource
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct Permission {
    /// Resource identifier (e.g., "users", "posts", "api:v1:users")
    pub resource: String,
    /// Action (e.g., "read", "write", "delete")
    pub action: String,
}

impl Permission {
    /// Create a new permission
    pub fn new(resource: impl Into<String>, action: impl Into<String>) -> Self {
        Self {
            resource: resource.into(),
            action: action.into(),
        }
    }

    /// Create a wildcard permission (all actions on resource)
    pub fn wildcard(resource: impl Into<String>) -> Self {
        Self {
            resource: resource.into(),
            action: "*".to_string(),
        }
    }

    /// Check if this permission matches another (considering wildcards)
    pub fn matches(&self, other: &Permission) -> bool {
        let resource_match = self.resource == other.resource || self.resource == "*";
        let action_match = self.action == other.action || self.action == "*";
        resource_match && action_match
    }
}

/// Role represents a collection of permissions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Role {
    /// Role identifier
    pub name: String,
    /// Permissions granted by this role
    pub permissions: Vec<Permission>,
    /// Parent roles (for role hierarchy)
    pub inherits_from: Vec<String>,
}

impl Role {
    /// Create a new role
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            permissions: Vec::new(),
            inherits_from: Vec::new(),
        }
    }

    /// Add a permission to the role
    pub fn with_permission(mut self, permission: Permission) -> Self {
        self.permissions.push(permission);
        self
    }

    /// Add parent role
    pub fn inherits_from(mut self, role: impl Into<String>) -> Self {
        self.inherits_from.push(role.into());
        self
    }
}

/// Role-Based Access Control system
#[derive(Debug, Clone)]
pub struct RoleBasedAccessControl {
    roles: Arc<DashMap<String, Role>>,
}

impl Default for RoleBasedAccessControl {
    fn default() -> Self {
        Self::new()
    }
}

impl RoleBasedAccessControl {
    /// Create a new RBAC system
    pub fn new() -> Self {
        Self {
            roles: Arc::new(DashMap::new()),
        }
    }

    /// Register a role
    pub fn add_role(&self, role: Role) {
        self.roles.insert(role.name.clone(), role);
    }

    /// Get a role by name
    pub fn get_role(&self, name: &str) -> Option<Role> {
        self.roles.get(name).map(|r| r.clone())
    }

    /// Check if a user with given roles has permission
    pub fn has_permission(&self, user_roles: &[String], required: &Permission) -> Result<bool> {
        for role_name in user_roles {
            if let Some(role) = self.get_role(role_name) {
                if self.role_has_permission(&role, required)? {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// Check if a role has a specific permission (including inherited)
    fn role_has_permission(&self, role: &Role, required: &Permission) -> Result<bool> {
        // Check direct permissions
        if role.permissions.iter().any(|p| p.matches(required)) {
            return Ok(true);
        }

        // Check inherited permissions
        for parent_name in &role.inherits_from {
            if let Some(parent_role) = self.get_role(parent_name) {
                if self.role_has_permission(&parent_role, required)? {
                    return Ok(true);
                }
            }
        }

        Ok(false)
    }

    /// Require permission (returns error if not granted)
    pub fn require_permission(&self, user_roles: &[String], required: &Permission) -> Result<()> {
        if self.has_permission(user_roles, required)? {
            Ok(())
        } else {
            Err(Error::Authorization(format!(
                "Permission denied: {}:{}",
                required.resource, required.action
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_permission_creation() {
        let perm = Permission::new("users", "read");
        assert_eq!(perm.resource, "users");
        assert_eq!(perm.action, "read");

        let wildcard = Permission::wildcard("posts");
        assert_eq!(wildcard.action, "*");
    }

    #[test]
    fn test_permission_matching() {
        let perm = Permission::new("users", "read");
        let wildcard = Permission::new("users", "*");
        let global_wildcard = Permission::new("*", "*");

        assert!(wildcard.matches(&perm));
        assert!(global_wildcard.matches(&perm));
        assert!(!perm.matches(&Permission::new("posts", "read")));
    }

    #[test]
    fn test_role_creation() {
        let role = Role::new("admin")
            .with_permission(Permission::wildcard("*"))
            .inherits_from("user");

        assert_eq!(role.name, "admin");
        assert_eq!(role.permissions.len(), 1);
        assert_eq!(role.inherits_from, vec!["user"]);
    }

    #[test]
    fn test_rbac_basic() {
        let rbac = RoleBasedAccessControl::new();

        let user_role = Role::new("user")
            .with_permission(Permission::new("posts", "read"))
            .with_permission(Permission::new("comments", "read"));

        rbac.add_role(user_role);

        let user_roles = vec!["user".to_string()];
        assert!(rbac
            .has_permission(&user_roles, &Permission::new("posts", "read"))
            .unwrap());
        assert!(!rbac
            .has_permission(&user_roles, &Permission::new("posts", "write"))
            .unwrap());
    }

    #[test]
    fn test_rbac_inheritance() {
        let rbac = RoleBasedAccessControl::new();

        let user_role = Role::new("user").with_permission(Permission::new("posts", "read"));

        let admin_role = Role::new("admin")
            .with_permission(Permission::new("posts", "write"))
            .inherits_from("user");

        rbac.add_role(user_role);
        rbac.add_role(admin_role);

        let admin_roles = vec!["admin".to_string()];
        assert!(rbac
            .has_permission(&admin_roles, &Permission::new("posts", "read"))
            .unwrap());
        assert!(rbac
            .has_permission(&admin_roles, &Permission::new("posts", "write"))
            .unwrap());
    }

    #[test]
    fn test_rbac_require_permission() {
        let rbac = RoleBasedAccessControl::new();

        let user_role = Role::new("user").with_permission(Permission::new("posts", "read"));

        rbac.add_role(user_role);

        let user_roles = vec!["user".to_string()];
        assert!(rbac
            .require_permission(&user_roles, &Permission::new("posts", "read"))
            .is_ok());
        assert!(rbac
            .require_permission(&user_roles, &Permission::new("posts", "delete"))
            .is_err());
    }
}
