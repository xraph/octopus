//! Session management for Octopus Gateway

use dashmap::DashMap;
use octopus_core::{Error, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

/// Session data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Session ID
    pub id: String,
    /// User ID
    pub user_id: String,
    /// Session creation time
    pub created_at: SystemTime,
    /// Session expiration time
    pub expires_at: SystemTime,
    /// Session data
    pub data: std::collections::HashMap<String, String>,
}

impl Session {
    /// Create a new session
    pub fn new(id: impl Into<String>, user_id: impl Into<String>, duration: Duration) -> Self {
        let now = SystemTime::now();
        Self {
            id: id.into(),
            user_id: user_id.into(),
            created_at: now,
            expires_at: now + duration,
            data: std::collections::HashMap::new(),
        }
    }

    /// Check if session is expired
    pub fn is_expired(&self) -> bool {
        SystemTime::now() > self.expires_at
    }

    /// Extend session expiration
    pub fn extend(&mut self, duration: Duration) {
        self.expires_at = SystemTime::now() + duration;
    }
}

/// Session manager
#[derive(Debug, Clone)]
pub struct SessionManager {
    sessions: Arc<DashMap<String, Session>>,
    default_duration: Duration,
}

impl SessionManager {
    /// Create a new session manager
    pub fn new(default_duration: Duration) -> Self {
        Self {
            sessions: Arc::new(DashMap::new()),
            default_duration,
        }
    }

    /// Create a new session
    pub fn create_session(&self, user_id: impl Into<String>) -> Session {
        let session_id = uuid::Uuid::new_v4().to_string();
        let session = Session::new(session_id.clone(), user_id, self.default_duration);
        self.sessions.insert(session_id, session.clone());
        session
    }

    /// Get a session by ID
    pub fn get_session(&self, session_id: &str) -> Result<Session> {
        let session = self
            .sessions
            .get(session_id)
            .ok_or_else(|| Error::Authentication("Session not found".to_string()))?
            .clone();

        if session.is_expired() {
            self.sessions.remove(session_id);
            return Err(Error::Authentication("Session expired".to_string()));
        }

        Ok(session)
    }

    /// Refresh a session (extend expiration)
    pub fn refresh_session(&self, session_id: &str) -> Result<()> {
        if let Some(mut session) = self.sessions.get_mut(session_id) {
            session.extend(self.default_duration);
            Ok(())
        } else {
            Err(Error::Authentication("Session not found".to_string()))
        }
    }

    /// Delete a session
    pub fn delete_session(&self, session_id: &str) -> Result<()> {
        self.sessions
            .remove(session_id)
            .ok_or_else(|| Error::Authentication("Session not found".to_string()))?;
        Ok(())
    }

    /// Clean up expired sessions
    pub fn cleanup_expired(&self) {
        self.sessions.retain(|_, session| !session.is_expired());
    }

    /// Get active session count
    pub fn active_count(&self) -> usize {
        self.sessions.len()
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new(Duration::from_secs(3600)) // 1 hour default
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn test_session_creation() {
        let session = Session::new("session-123", "user-456", Duration::from_secs(3600));
        assert_eq!(session.id, "session-123");
        assert_eq!(session.user_id, "user-456");
        assert!(!session.is_expired());
    }

    #[test]
    fn test_session_expiration() {
        let mut session = Session::new("session-123", "user-456", Duration::from_millis(10));
        assert!(!session.is_expired());
        sleep(Duration::from_millis(15));
        assert!(session.is_expired());

        // Test extend
        session.extend(Duration::from_secs(3600));
        assert!(!session.is_expired());
    }

    #[test]
    fn test_session_manager() {
        let manager = SessionManager::new(Duration::from_secs(3600));
        let session = manager.create_session("user-123");

        assert_eq!(manager.active_count(), 1);

        let fetched = manager.get_session(&session.id).unwrap();
        assert_eq!(fetched.user_id, "user-123");

        manager.delete_session(&session.id).unwrap();
        assert_eq!(manager.active_count(), 0);
    }

    #[test]
    fn test_session_refresh() {
        let manager = SessionManager::new(Duration::from_secs(1));
        let session = manager.create_session("user-123");

        sleep(Duration::from_millis(500));
        manager.refresh_session(&session.id).unwrap();

        sleep(Duration::from_millis(700));
        // Should still be valid because we refreshed
        assert!(manager.get_session(&session.id).is_ok());
    }

    #[test]
    fn test_cleanup_expired() {
        let manager = SessionManager::new(Duration::from_millis(10));
        manager.create_session("user-1");
        manager.create_session("user-2");

        assert_eq!(manager.active_count(), 2);

        sleep(Duration::from_millis(15));
        manager.cleanup_expired();

        assert_eq!(manager.active_count(), 0);
    }
}
