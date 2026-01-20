//! Local file-based state storage backend.
//!
//! This module provides a simple file-based state storage for local development
//! and single-machine deployments.

use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tracing::{debug, info};

use crate::error::{HalldyllError, Result, StateError};

use super::lock::{generate_holder_id, LockInfo, LOCK_EXPIRY_SECS};
use super::store::StateStore;
use super::types::DeploymentState;

/// Default state directory name.
const STATE_DIR: &str = ".halldyll";

/// State file name.
const STATE_FILE: &str = "state.json";

/// Lock file name.
const LOCK_FILE: &str = "state.lock";

/// Local file-based state store.
#[derive(Debug)]
pub struct LocalStateStore {
    /// Base directory for state files.
    base_dir: PathBuf,
    /// Path to the state file.
    state_path: PathBuf,
    /// Path to the lock file.
    lock_path: PathBuf,
}

impl LocalStateStore {
    /// Creates a new local state store with default paths.
    ///
    /// # Errors
    ///
    /// Returns an error if the base directory cannot be determined.
    pub fn new() -> Result<Self> {
        let base_dir = std::env::current_dir()
            .map_err(|e| HalldyllError::internal(format!("Cannot determine current directory: {e}")))?
            .join(STATE_DIR);

        Ok(Self::with_base_dir(base_dir))
    }

    /// Creates a new local state store with a custom base directory.
    #[must_use]
    pub fn with_base_dir(base_dir: impl Into<PathBuf>) -> Self {
        let base_dir = base_dir.into();
        let state_path = base_dir.join(STATE_FILE);
        let lock_path = base_dir.join(LOCK_FILE);

        Self {
            base_dir,
            state_path,
            lock_path,
        }
    }

    /// Creates a new local state store from a custom state file path.
    #[must_use]
    pub fn with_state_path(state_path: impl Into<PathBuf>) -> Self {
        let state_path = state_path.into();
        let base_dir = state_path
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        let lock_path = base_dir.join(LOCK_FILE);

        Self {
            base_dir,
            state_path,
            lock_path,
        }
    }

    /// Ensures the state directory exists.
    async fn ensure_dir(&self) -> Result<()> {
        if !self.base_dir.exists() {
            debug!("Creating state directory: {}", self.base_dir.display());
            fs::create_dir_all(&self.base_dir).await.map_err(|e| {
                HalldyllError::State(StateError::S3Error {
                    message: format!("Failed to create state directory: {e}"),
                })
            })?;
        }
        Ok(())
    }

    /// Reads the lock file if it exists.
    async fn read_lock_file(&self) -> Result<Option<LockInfo>> {
        if !self.lock_path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&self.lock_path).await.map_err(|e| {
            HalldyllError::State(StateError::Corrupted {
                message: format!("Failed to read lock file: {e}"),
            })
        })?;

        let lock_info: LockInfo = serde_json::from_str(&content).map_err(|e| {
            HalldyllError::State(StateError::Corrupted {
                message: format!("Failed to parse lock file: {e}"),
            })
        })?;

        Ok(Some(lock_info))
    }

    /// Writes the lock file.
    async fn write_lock_file(&self, lock_info: &LockInfo) -> Result<()> {
        self.ensure_dir().await?;

        let content = serde_json::to_string_pretty(lock_info).map_err(|e| {
            HalldyllError::State(StateError::SerializationError {
                message: format!("Failed to serialize lock: {e}"),
            })
        })?;

        let mut file = fs::File::create(&self.lock_path).await.map_err(|e| {
            HalldyllError::State(StateError::LockFailed {
                message: format!("Failed to create lock file: {e}"),
            })
        })?;

        file.write_all(content.as_bytes()).await.map_err(|e| {
            HalldyllError::State(StateError::LockFailed {
                message: format!("Failed to write lock file: {e}"),
            })
        })?;

        file.sync_all().await.map_err(|e| {
            HalldyllError::State(StateError::LockFailed {
                message: format!("Failed to sync lock file: {e}"),
            })
        })?;

        Ok(())
    }

    /// Deletes the lock file.
    async fn delete_lock_file(&self) -> Result<()> {
        if self.lock_path.exists() {
            fs::remove_file(&self.lock_path).await.map_err(|e| {
                HalldyllError::State(StateError::LockFailed {
                    message: format!("Failed to delete lock file: {e}"),
                })
            })?;
        }
        Ok(())
    }
}

#[async_trait]
impl StateStore for LocalStateStore {
    async fn load(&self) -> Result<Option<DeploymentState>> {
        if !self.state_path.exists() {
            debug!("State file does not exist: {}", self.state_path.display());
            return Ok(None);
        }

        info!("Loading state from: {}", self.state_path.display());

        let content = fs::read_to_string(&self.state_path).await.map_err(|e| {
            HalldyllError::State(StateError::Corrupted {
                message: format!("Failed to read state file: {e}"),
            })
        })?;

        let state: DeploymentState = serde_json::from_str(&content).map_err(|e| {
            HalldyllError::State(StateError::Corrupted {
                message: format!("Failed to parse state file: {e}"),
            })
        })?;

        Ok(Some(state))
    }

    async fn save(&self, state: &DeploymentState) -> Result<()> {
        self.ensure_dir().await?;

        info!("Saving state to: {}", self.state_path.display());

        let content = serde_json::to_string_pretty(state).map_err(|e| {
            HalldyllError::State(StateError::SerializationError {
                message: format!("Failed to serialize state: {e}"),
            })
        })?;

        // Write to a temporary file first, then rename for atomicity
        let temp_path = self.state_path.with_extension("tmp");

        let mut file = fs::File::create(&temp_path).await.map_err(|e| {
            HalldyllError::State(StateError::S3Error {
                message: format!("Failed to create temp state file: {e}"),
            })
        })?;

        file.write_all(content.as_bytes()).await.map_err(|e| {
            HalldyllError::State(StateError::S3Error {
                message: format!("Failed to write state file: {e}"),
            })
        })?;

        file.sync_all().await.map_err(|e| {
            HalldyllError::State(StateError::S3Error {
                message: format!("Failed to sync state file: {e}"),
            })
        })?;

        // Atomic rename
        fs::rename(&temp_path, &self.state_path).await.map_err(|e| {
            HalldyllError::State(StateError::S3Error {
                message: format!("Failed to rename state file: {e}"),
            })
        })?;

        debug!("State saved successfully");
        Ok(())
    }

    async fn delete(&self) -> Result<()> {
        if self.state_path.exists() {
            info!("Deleting state file: {}", self.state_path.display());
            fs::remove_file(&self.state_path).await.map_err(|e| {
                HalldyllError::State(StateError::S3Error {
                    message: format!("Failed to delete state file: {e}"),
                })
            })?;
        }

        // Also delete lock file
        self.delete_lock_file().await?;

        Ok(())
    }

    async fn exists(&self) -> Result<bool> {
        Ok(self.state_path.exists())
    }

    async fn acquire_lock(&self, holder: &str) -> Result<LockInfo> {
        // Check for existing lock
        if let Some(existing) = self.read_lock_file().await? {
            if !existing.is_expired() {
                return Err(HalldyllError::State(StateError::LockedByOther {
                    holder: existing.holder.clone(),
                    since: existing.acquired_at.to_rfc3339(),
                }));
            }
            // Lock is expired, we can take it
            debug!("Expired lock found, taking over");
        }

        let holder_id = if holder.is_empty() {
            generate_holder_id()
        } else {
            holder.to_string()
        };

        let lock_info = LockInfo::new(&holder_id);
        self.write_lock_file(&lock_info).await?;

        info!(
            "Acquired state lock: {} (expires in {}s)",
            lock_info.lock_id, LOCK_EXPIRY_SECS
        );

        Ok(lock_info)
    }

    async fn release_lock(&self, lock_id: &str) -> Result<()> {
        if let Some(existing) = self.read_lock_file().await? {
            if existing.lock_id == lock_id {
                self.delete_lock_file().await?;
                info!("Released state lock: {lock_id}");
            } else {
                debug!(
                    "Lock ID mismatch: expected {lock_id}, found {}",
                    existing.lock_id
                );
            }
        }
        Ok(())
    }

    async fn get_lock_info(&self) -> Result<Option<LockInfo>> {
        self.read_lock_file().await
    }

    async fn is_locked(&self) -> Result<bool> {
        if let Some(lock_info) = self.read_lock_file().await? {
            return Ok(!lock_info.is_expired());
        }
        Ok(false)
    }

    fn backend_type(&self) -> &'static str {
        "local"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn create_test_store() -> (LocalStateStore, TempDir) {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let store = LocalStateStore::with_base_dir(temp_dir.path());
        (store, temp_dir)
    }

    #[tokio::test]
    async fn test_save_and_load() {
        let (store, _temp) = create_test_store().await;

        let state = DeploymentState::new("test-project", "dev");
        store.save(&state).await.expect("Failed to save state");

        let loaded = store
            .load()
            .await
            .expect("Failed to load state")
            .expect("State should exist");

        assert_eq!(loaded.project, "test-project");
        assert_eq!(loaded.environment, "dev");
    }

    #[tokio::test]
    async fn test_load_nonexistent() {
        let (store, _temp) = create_test_store().await;

        let result = store.load().await.expect("Load should not fail");
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_exists() {
        let (store, _temp) = create_test_store().await;

        assert!(!store.exists().await.expect("exists check failed"));

        let state = DeploymentState::new("test-project", "dev");
        store.save(&state).await.expect("Failed to save state");

        assert!(store.exists().await.expect("exists check failed"));
    }

    #[tokio::test]
    async fn test_lock_acquire_release() {
        let (store, _temp) = create_test_store().await;

        let lock = store
            .acquire_lock("test-holder")
            .await
            .expect("Failed to acquire lock");

        assert!(store.is_locked().await.expect("is_locked failed"));

        store
            .release_lock(&lock.lock_id)
            .await
            .expect("Failed to release lock");

        assert!(!store.is_locked().await.expect("is_locked failed"));
    }

    #[tokio::test]
    async fn test_lock_conflict() {
        let (store, _temp) = create_test_store().await;

        let _lock1 = store
            .acquire_lock("holder-1")
            .await
            .expect("Failed to acquire first lock");

        let result = store.acquire_lock("holder-2").await;
        assert!(result.is_err());
    }
}
