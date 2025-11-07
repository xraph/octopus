//! API key management for Octopus Gateway

use dashmap::DashMap;
use octopus_core::{Error, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::SystemTime;

/// API key for authentication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    /// Unique key identifier
    pub id: String,
    /// The actual API key value (should be hashed in production!)
    pub key: String,
    /// User/service this key belongs to
    pub owner: String,
    /// Key description
    pub description: String,
    /// Allowed scopes/permissions
    pub scopes: Vec<String>,
    /// Creation time
    pub created_at: SystemTime,
    /// Expiration time (if any)
    pub expires_at: Option<SystemTime>,
    /// Whether key is active
    pub active: bool,
}

impl ApiKey {
    /// Create a new API key
    pub fn new(
        id: impl Into<String>,
        key: impl Into<String>,
        owner: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            key: key.into(),
            owner: owner.into(),
            description: description.into(),
            scopes: Vec::new(),
            created_at: SystemTime::now(),
            expires_at: None,
            active: true,
        }
    }

    /// Add scope to API key
    pub fn with_scope(mut self, scope: impl Into<String>) -> Self {
        self.scopes.push(scope.into());
        self
    }

    /// Set expiration
    pub fn expires_at(mut self, expires_at: SystemTime) -> Self {
        self.expires_at = Some(expires_at);
        self
    }

    /// Check if key is expired
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            SystemTime::now() > expires_at
        } else {
            false
        }
    }

    /// Check if key is valid (active and not expired)
    pub fn is_valid(&self) -> bool {
        self.active && !self.is_expired()
    }
}

/// API key store
#[derive(Debug, Clone)]
pub struct ApiKeyStore {
    keys: Arc<DashMap<String, ApiKey>>, // key value -> ApiKey
}

impl Default for ApiKeyStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ApiKeyStore {
    /// Create a new API key store
    pub fn new() -> Self {
        Self {
            keys: Arc::new(DashMap::new()),
        }
    }

    /// Add an API key
    pub fn add_key(&self, key: ApiKey) {
        self.keys.insert(key.key.clone(), key);
    }

    /// Get an API key by value
    pub fn get_key(&self, key_value: &str) -> Result<ApiKey> {
        self.keys
            .get(key_value)
            .map(|k| k.clone())
            .ok_or_else(|| Error::Authentication("Invalid API key".to_string()))
    }

    /// Validate an API key
    pub fn validate_key(&self, key_value: &str) -> Result<ApiKey> {
        let key = self.get_key(key_value)?;

        if !key.is_valid() {
            return Err(Error::Authentication(
                "API key is invalid or expired".to_string(),
            ));
        }

        Ok(key)
    }

    /// Revoke an API key
    pub fn revoke_key(&self, key_value: &str) -> Result<()> {
        if let Some(mut key) = self.keys.get_mut(key_value) {
            key.active = false;
            Ok(())
        } else {
            Err(Error::Authentication("API key not found".to_string()))
        }
    }

    /// Delete an API key
    pub fn delete_key(&self, key_value: &str) -> Result<()> {
        self.keys
            .remove(key_value)
            .ok_or_else(|| Error::Authentication("API key not found".to_string()))?;
        Ok(())
    }

    /// Get all keys for an owner
    pub fn get_keys_by_owner(&self, owner: &str) -> Vec<ApiKey> {
        self.keys
            .iter()
            .filter(|entry| entry.owner == owner)
            .map(|entry| entry.clone())
            .collect()
    }

    /// Clean up expired keys
    pub fn cleanup_expired(&self) {
        self.keys.retain(|_, key| !key.is_expired());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_api_key_creation() {
        let key = ApiKey::new("key-123", "secret-key", "user-456", "Test key")
            .with_scope("read:users")
            .with_scope("write:posts");

        assert_eq!(key.id, "key-123");
        assert_eq!(key.owner, "user-456");
        assert_eq!(key.scopes.len(), 2);
        assert!(key.is_valid());
    }

    #[test]
    fn test_api_key_expiration() {
        let past = SystemTime::now() - Duration::from_secs(3600);
        let key = ApiKey::new("key-123", "secret-key", "user-456", "Test key").expires_at(past);

        assert!(key.is_expired());
        assert!(!key.is_valid());
    }

    #[test]
    fn test_api_key_store() {
        let store = ApiKeyStore::new();
        let key =
            ApiKey::new("key-123", "secret-key-xyz", "user-456", "Test key").with_scope("admin");

        store.add_key(key.clone());

        let fetched = store.validate_key("secret-key-xyz").unwrap();
        assert_eq!(fetched.id, "key-123");
        assert_eq!(fetched.owner, "user-456");
        assert!(fetched.scopes.contains(&"admin".to_string()));
    }

    #[test]
    fn test_api_key_revoke() {
        let store = ApiKeyStore::new();
        let key = ApiKey::new("key-123", "secret-key-xyz", "user-456", "Test key");

        store.add_key(key);
        assert!(store.validate_key("secret-key-xyz").is_ok());

        store.revoke_key("secret-key-xyz").unwrap();
        assert!(store.validate_key("secret-key-xyz").is_err());
    }

    #[test]
    fn test_get_keys_by_owner() {
        let store = ApiKeyStore::new();
        store.add_key(ApiKey::new("key-1", "secret-1", "user-1", "Key 1"));
        store.add_key(ApiKey::new("key-2", "secret-2", "user-1", "Key 2"));
        store.add_key(ApiKey::new("key-3", "secret-3", "user-2", "Key 3"));

        let user1_keys = store.get_keys_by_owner("user-1");
        assert_eq!(user1_keys.len(), 2);

        let user2_keys = store.get_keys_by_owner("user-2");
        assert_eq!(user2_keys.len(), 1);
    }
}
