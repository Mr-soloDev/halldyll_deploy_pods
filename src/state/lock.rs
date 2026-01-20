//! State locking for concurrent access protection.
//!
//! This module provides distributed locking to prevent concurrent
//! modifications to the deployment state.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Lock expiry duration in seconds.
pub const LOCK_EXPIRY_SECS: i64 = 300; // 5 minutes

/// Information about a state lock.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockInfo {
    /// Unique lock identifier.
    pub lock_id: String,
    /// Who holds the lock.
    pub holder: String,
    /// When the lock was acquired.
    pub acquired_at: DateTime<Utc>,
    /// When the lock expires.
    pub expires_at: DateTime<Utc>,
}

/// State lock for coordinating access.
#[derive(Debug)]
pub struct StateLock {
    /// Lock information.
    info: LockInfo,
}

impl LockInfo {
    /// Creates a new lock info.
    #[must_use]
    pub fn new(holder: &str) -> Self {
        let now = Utc::now();
        Self {
            lock_id: Uuid::new_v4().to_string(),
            holder: holder.to_string(),
            acquired_at: now,
            expires_at: now + chrono::Duration::seconds(LOCK_EXPIRY_SECS),
        }
    }

    /// Checks if the lock has expired.
    #[must_use]
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }

    /// Refreshes the lock expiry time.
    pub fn refresh(&mut self) {
        self.expires_at = Utc::now() + chrono::Duration::seconds(LOCK_EXPIRY_SECS);
    }

    /// Returns the remaining time until expiry in seconds.
    #[must_use]
    pub fn remaining_secs(&self) -> i64 {
        let remaining = self.expires_at - Utc::now();
        remaining.num_seconds().max(0)
    }
}

impl StateLock {
    /// Creates a new state lock.
    #[must_use]
    pub fn new(holder: &str) -> Self {
        Self {
            info: LockInfo::new(holder),
        }
    }

    /// Creates a state lock from existing lock info.
    #[must_use]
    pub const fn from_info(info: LockInfo) -> Self {
        Self { info }
    }

    /// Gets the lock ID.
    #[must_use]
    pub fn lock_id(&self) -> &str {
        &self.info.lock_id
    }

    /// Gets the lock holder.
    #[must_use]
    pub fn holder(&self) -> &str {
        &self.info.holder
    }

    /// Gets the lock info.
    #[must_use]
    pub const fn info(&self) -> &LockInfo {
        &self.info
    }

    /// Checks if the lock has expired.
    #[must_use]
    pub fn is_expired(&self) -> bool {
        self.info.is_expired()
    }

    /// Refreshes the lock.
    pub fn refresh(&mut self) {
        self.info.refresh();
    }
}

/// Generates a unique holder identifier for the current process.
#[must_use]
pub fn generate_holder_id() -> String {
    let hostname = hostname::get().map_or_else(|_| String::from("unknown"), |h| h.to_string_lossy().to_string());

    let pid = std::process::id();
    let uuid = &Uuid::new_v4().to_string()[..8];

    format!("{hostname}-{pid}-{uuid}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lock_creation() {
        let lock = StateLock::new("test-holder");
        assert_eq!(lock.holder(), "test-holder");
        assert!(!lock.is_expired());
        assert!(lock.info().remaining_secs() > 0);
    }

    #[test]
    fn test_lock_refresh() {
        let mut lock = StateLock::new("test-holder");
        let original_expiry = lock.info().expires_at;

        // Wait a tiny bit and refresh
        std::thread::sleep(std::time::Duration::from_millis(10));
        lock.refresh();

        assert!(lock.info().expires_at >= original_expiry);
    }

    #[test]
    fn test_holder_id_generation() {
        let id1 = generate_holder_id();
        let id2 = generate_holder_id();

        // IDs should be unique
        assert_ne!(id1, id2);

        // IDs should contain the process ID
        let pid = std::process::id().to_string();
        assert!(id1.contains(&pid));
    }
}
