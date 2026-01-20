//! State store trait definition.
//!
//! This module defines the common interface for state storage backends.

use async_trait::async_trait;

use crate::error::Result;
use super::types::DeploymentState;
use super::lock::LockInfo;

/// Trait for state storage backends.
#[async_trait]
pub trait StateStore: Send + Sync {
    /// Loads the deployment state.
    ///
    /// Returns `None` if no state exists yet.
    async fn load(&self) -> Result<Option<DeploymentState>>;

    /// Saves the deployment state.
    async fn save(&self, state: &DeploymentState) -> Result<()>;

    /// Deletes the deployment state.
    async fn delete(&self) -> Result<()>;

    /// Checks if state exists.
    async fn exists(&self) -> Result<bool>;

    /// Acquires a lock on the state.
    ///
    /// Returns lock information if successful.
    async fn acquire_lock(&self, holder: &str) -> Result<LockInfo>;

    /// Releases a lock on the state.
    async fn release_lock(&self, lock_id: &str) -> Result<()>;

    /// Gets current lock information if locked.
    async fn get_lock_info(&self) -> Result<Option<LockInfo>>;

    /// Checks if the state is locked.
    async fn is_locked(&self) -> Result<bool>;

    /// Gets the backend type name.
    fn backend_type(&self) -> &'static str;
}

#[async_trait]
impl StateStore for Box<dyn StateStore> {
    async fn load(&self) -> Result<Option<DeploymentState>> {
        (**self).load().await
    }

    async fn save(&self, state: &DeploymentState) -> Result<()> {
        (**self).save(state).await
    }

    async fn delete(&self) -> Result<()> {
        (**self).delete().await
    }

    async fn exists(&self) -> Result<bool> {
        (**self).exists().await
    }

    async fn acquire_lock(&self, holder: &str) -> Result<LockInfo> {
        (**self).acquire_lock(holder).await
    }

    async fn release_lock(&self, lock_id: &str) -> Result<()> {
        (**self).release_lock(lock_id).await
    }

    async fn get_lock_info(&self) -> Result<Option<LockInfo>> {
        (**self).get_lock_info().await
    }

    async fn is_locked(&self) -> Result<bool> {
        (**self).is_locked().await
    }

    fn backend_type(&self) -> &'static str {
        (**self).backend_type()
    }
}
