//! Activity log for tracking recent requests

use super::*;
use http::{Method, StatusCode};
use std::collections::VecDeque;

/// A single activity entry
#[derive(Debug, Clone)]
pub struct ActivityEntry {
    /// Timestamp in milliseconds
    pub timestamp: u64,
    /// HTTP method
    pub method: String,
    /// Request path
    pub path: String,
    /// Response status code
    pub status: u16,
    /// Latency in milliseconds
    pub latency_ms: f64,
    /// Upstream service name
    pub upstream: String,
}

impl ActivityEntry {
    /// Create a new activity entry
    pub fn new(
        method: Method,
        path: String,
        status: StatusCode,
        latency: Duration,
        upstream: String,
    ) -> Self {
        Self {
            timestamp: current_timestamp_ms(),
            method: method.to_string(),
            path,
            status: status.as_u16(),
            latency_ms: format_duration_ms(latency),
            upstream,
        }
    }

    /// Get formatted timestamp
    pub fn formatted_time(&self) -> String {
        // Convert to human-readable time
        let secs = self.timestamp / 1000;
        let millis = self.timestamp % 1000;
        
        // Simple formatting (in production, use chrono)
        format!("{}.{:03}s", secs, millis)
    }

    /// Get status class for UI styling
    pub fn status_class(&self) -> &'static str {
        match self.status {
            200..=299 => "success",
            300..=399 => "redirect",
            400..=499 => "client-error",
            500..=599 => "server-error",
            _ => "unknown",
        }
    }

    /// Check if this was an error
    pub fn is_error(&self) -> bool {
        self.status >= 400
    }
}

/// Activity log that tracks recent requests
#[derive(Debug, Clone)]
pub struct ActivityLog {
    /// Recent entries (circular buffer)
    entries: Arc<parking_lot::Mutex<VecDeque<ActivityEntry>>>,
    /// Maximum number of entries to keep
    max_entries: usize,
}

impl ActivityLog {
    /// Create a new activity log
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: Arc::new(parking_lot::Mutex::new(VecDeque::with_capacity(max_entries))),
            max_entries,
        }
    }

    /// Add a new activity entry
    pub fn add_entry(&self, entry: ActivityEntry) {
        let mut entries = self.entries.lock();
        if entries.len() >= self.max_entries {
            entries.pop_front();
        }
        entries.push_back(entry);
    }

    /// Record a request directly
    pub fn record(
        &self,
        method: Method,
        path: String,
        status: StatusCode,
        latency: Duration,
        upstream: String,
    ) {
        let entry = ActivityEntry::new(method, path, status, latency, upstream);
        self.add_entry(entry);
    }

    /// Get recent entries (most recent first)
    pub fn recent_entries(&self, limit: usize) -> Vec<ActivityEntry> {
        let entries = self.entries.lock();
        entries
            .iter()
            .rev()
            .take(limit)
            .cloned()
            .collect()
    }

    /// Get all entries
    pub fn all_entries(&self) -> Vec<ActivityEntry> {
        let entries = self.entries.lock();
        entries.iter().rev().cloned().collect()
    }

    /// Clear all entries
    pub fn clear(&self) {
        let mut entries = self.entries.lock();
        entries.clear();
    }

    /// Get entry count
    pub fn count(&self) -> usize {
        let entries = self.entries.lock();
        entries.len()
    }
}

impl Default for ActivityLog {
    fn default() -> Self {
        Self::new(1000) // Keep last 1000 requests by default
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_activity_entry() {
        let entry = ActivityEntry::new(
            Method::GET,
            "/users".to_string(),
            StatusCode::OK,
            Duration::from_millis(50),
            "user-service".to_string(),
        );

        assert_eq!(entry.method, "GET");
        assert_eq!(entry.path, "/users");
        assert_eq!(entry.status, 200);
        assert!((entry.latency_ms - 50.0).abs() < 0.1);
        assert_eq!(entry.status_class(), "success");
        assert!(!entry.is_error());
    }

    #[test]
    fn test_activity_log() {
        let log = ActivityLog::new(10);
        
        log.record(
            Method::GET,
            "/users".to_string(),
            StatusCode::OK,
            Duration::from_millis(50),
            "user-service".to_string(),
        );

        log.record(
            Method::POST,
            "/posts".to_string(),
            StatusCode::CREATED,
            Duration::from_millis(100),
            "post-service".to_string(),
        );

        assert_eq!(log.count(), 2);
        
        let entries = log.recent_entries(1);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "/posts"); // Most recent first
    }

    #[test]
    fn test_activity_log_limit() {
        let log = ActivityLog::new(3);
        
        for i in 0..5 {
            log.record(
                Method::GET,
                format!("/path{}", i),
                StatusCode::OK,
                Duration::from_millis(50),
                "service".to_string(),
            );
        }

        assert_eq!(log.count(), 3); // Should only keep last 3
        
        let entries = log.all_entries();
        assert_eq!(entries[0].path, "/path4"); // Most recent
        assert_eq!(entries[2].path, "/path2"); // Oldest kept
    }
}

