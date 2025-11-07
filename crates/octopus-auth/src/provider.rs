//! Authentication providers for Octopus Gateway

use async_trait::async_trait;
use dashmap::DashMap;
use octopus_core::{Error, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// User identity information
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct User {
    /// Unique user ID
    pub id: String,
    /// Username
    pub username: String,
    /// Email address
    pub email: String,
    /// User roles
    pub roles: Vec<String>,
    /// User metadata
    pub metadata: std::collections::HashMap<String, String>,
}

impl User {
    /// Create a new user
    pub fn new(
        id: impl Into<String>,
        username: impl Into<String>,
        email: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            username: username.into(),
            email: email.into(),
            roles: Vec::new(),
            metadata: std::collections::HashMap::new(),
        }
    }

    /// Add a role to the user
    pub fn with_role(mut self, role: impl Into<String>) -> Self {
        self.roles.push(role.into());
        self
    }

    /// Check if user has a specific role
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role)
    }

    /// Check if user has any of the specified roles
    pub fn has_any_role(&self, roles: &[String]) -> bool {
        roles.iter().any(|role| self.has_role(role))
    }
}

/// Authentication provider trait
#[async_trait]
pub trait AuthProvider: Send + Sync + std::fmt::Debug {
    /// Authenticate a user with credentials
    async fn authenticate(&self, username: &str, password: &str) -> Result<User>;

    /// Get a user by ID
    async fn get_user(&self, user_id: &str) -> Result<Option<User>>;

    /// Validate a token and return the associated user
    async fn validate_token(&self, token: &str) -> Result<User>;
}

/// In-memory user store for testing/development
#[derive(Debug, Clone)]
pub struct UserStore {
    users: Arc<DashMap<String, User>>,
    credentials: Arc<DashMap<String, String>>, // username -> password hash
}

impl Default for UserStore {
    fn default() -> Self {
        Self::new()
    }
}

impl UserStore {
    /// Create a new user store
    pub fn new() -> Self {
        Self {
            users: Arc::new(DashMap::new()),
            credentials: Arc::new(DashMap::new()),
        }
    }

    /// Add a user to the store
    pub fn add_user(&self, user: User, password_hash: String) {
        self.credentials
            .insert(user.username.clone(), password_hash);
        self.users.insert(user.id.clone(), user);
    }

    /// Get a user by username
    pub fn get_by_username(&self, username: &str) -> Option<User> {
        self.users.iter().find_map(|entry| {
            if entry.username == username {
                Some(entry.clone())
            } else {
                None
            }
        })
    }

    /// Verify password (simple comparison for demo - use proper hashing in production!)
    pub fn verify_password(&self, username: &str, password: &str) -> bool {
        self.credentials
            .get(username)
            .map(|hash| hash.value() == password)
            .unwrap_or(false)
    }
}

#[async_trait]
impl AuthProvider for UserStore {
    async fn authenticate(&self, username: &str, password: &str) -> Result<User> {
        if !self.verify_password(username, password) {
            return Err(Error::Authentication("Invalid credentials".to_string()));
        }

        self.get_by_username(username)
            .ok_or_else(|| Error::Authentication("User not found".to_string()))
    }

    async fn get_user(&self, user_id: &str) -> Result<Option<User>> {
        Ok(self.users.get(user_id).map(|u| u.clone()))
    }

    async fn validate_token(&self, _token: &str) -> Result<User> {
        // Token validation would be done by JWT plugin or similar
        Err(Error::Authentication(
            "Token validation not implemented".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_creation() {
        let user = User::new("1", "alice", "alice@example.com")
            .with_role("admin")
            .with_role("user");

        assert_eq!(user.id, "1");
        assert_eq!(user.username, "alice");
        assert_eq!(user.roles.len(), 2);
        assert!(user.has_role("admin"));
        assert!(user.has_role("user"));
        assert!(!user.has_role("superadmin"));
    }

    #[test]
    fn test_user_has_any_role() {
        let user = User::new("1", "alice", "alice@example.com").with_role("admin");

        assert!(user.has_any_role(&vec!["admin".to_string(), "user".to_string()]));
        assert!(!user.has_any_role(&vec!["superadmin".to_string(), "moderator".to_string()]));
    }

    #[tokio::test]
    async fn test_user_store() {
        let store = UserStore::new();
        let user = User::new("1", "alice", "alice@example.com").with_role("admin");

        store.add_user(user.clone(), "password123".to_string());

        // Test get_user
        let fetched = store.get_user("1").await.unwrap();
        assert_eq!(fetched, Some(user.clone()));

        // Test authentication
        let auth_result = store.authenticate("alice", "password123").await;
        assert!(auth_result.is_ok());
        assert_eq!(auth_result.unwrap().id, "1");

        // Test wrong password
        let wrong_pass = store.authenticate("alice", "wrongpass").await;
        assert!(wrong_pass.is_err());
    }
}
