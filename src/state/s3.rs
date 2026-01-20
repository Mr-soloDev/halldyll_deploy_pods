//! S3-based state storage backend.
//!
//! This module provides a remote state storage using AWS S3 (or compatible services)
//! for distributed deployments and team collaboration.

use async_trait::async_trait;
use aws_sdk_s3::Client;
use tracing::{debug, info};

use crate::error::{HalldyllError, Result, StateError};

use super::lock::{generate_holder_id, LockInfo, LOCK_EXPIRY_SECS};
use super::store::StateStore;
use super::types::DeploymentState;

/// State file key suffix.
const STATE_KEY: &str = "state.json";

/// Lock file key suffix.
const LOCK_KEY: &str = "state.lock";

/// S3-based state store.
#[derive(Debug)]
pub struct S3StateStore {
    /// S3 client.
    client: Client,
    /// Bucket name.
    bucket: String,
    /// Key prefix.
    prefix: String,
}

impl S3StateStore {
    /// Creates a new S3 state store.
    ///
    /// # Errors
    ///
    /// Returns an error if the S3 client cannot be initialized.
    pub async fn new(bucket: &str, prefix: Option<&str>, region: Option<&str>) -> Result<Self> {
        let config = if let Some(region_str) = region {
            aws_config::from_env()
                .region(aws_config::Region::new(region_str.to_string()))
                .load()
                .await
        } else {
            aws_config::load_from_env().await
        };

        let client = Client::new(&config);

        let prefix = prefix
            .map(|p| {
                let p = p.trim_matches('/');
                if p.is_empty() {
                    String::new()
                } else {
                    format!("{p}/")
                }
            })
            .unwrap_or_default();

        Ok(Self {
            client,
            bucket: bucket.to_string(),
            prefix,
        })
    }

    /// Creates a new S3 state store with an existing client.
    #[must_use]
    pub fn with_client(client: Client, bucket: &str, prefix: Option<&str>) -> Self {
        let prefix = prefix
            .map(|p| {
                let p = p.trim_matches('/');
                if p.is_empty() {
                    String::new()
                } else {
                    format!("{p}/")
                }
            })
            .unwrap_or_default();

        Self {
            client,
            bucket: bucket.to_string(),
            prefix,
        }
    }

    /// Gets the full S3 key for a file.
    fn key(&self, file: &str) -> String {
        format!("{}{file}", self.prefix)
    }

    /// Gets an object from S3.
    async fn get_object(&self, key: &str) -> Result<Option<String>> {
        let result = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await;

        match result {
            Ok(response) => {
                let bytes = response.body.collect().await.map_err(|e| {
                    HalldyllError::State(StateError::s3(format!("Failed to read S3 object: {e}")))
                })?;

                let content = String::from_utf8(bytes.to_vec()).map_err(|e| {
                    HalldyllError::State(StateError::Corrupted {
                        message: format!("Invalid UTF-8 in S3 object: {e}"),
                    })
                })?;

                Ok(Some(content))
            }
            Err(sdk_err) => {
                let service_err = sdk_err.into_service_error();
                if service_err.is_no_such_key() {
                    Ok(None)
                } else {
                    Err(HalldyllError::State(StateError::s3(format!(
                        "S3 get error: {service_err}"
                    ))))
                }
            }
        }
    }

    /// Puts an object to S3.
    async fn put_object(&self, key: &str, content: &str) -> Result<()> {
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .body(content.as_bytes().to_vec().into())
            .content_type("application/json")
            .send()
            .await
            .map_err(|e| {
                HalldyllError::State(StateError::s3(format!("S3 put error: {e}")))
            })?;

        Ok(())
    }

    /// Deletes an object from S3.
    async fn delete_object(&self, key: &str) -> Result<()> {
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| {
                HalldyllError::State(StateError::s3(format!("S3 delete error: {e}")))
            })?;

        Ok(())
    }

    /// Checks if an object exists in S3.
    async fn object_exists(&self, key: &str) -> Result<bool> {
        let result = self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await;

        match result {
            Ok(_) => Ok(true),
            Err(sdk_err) => {
                let service_err = sdk_err.into_service_error();
                if service_err.is_not_found() {
                    Ok(false)
                } else {
                    Err(HalldyllError::State(StateError::s3(format!(
                        "S3 head error: {service_err}"
                    ))))
                }
            }
        }
    }
}

#[async_trait]
impl StateStore for S3StateStore {
    async fn load(&self) -> Result<Option<DeploymentState>> {
        let key = self.key(STATE_KEY);
        debug!("Loading state from s3://{}/{key}", self.bucket);

        let content = self.get_object(&key).await?;

        if let Some(json) = content {
            let state: DeploymentState = serde_json::from_str(&json).map_err(|e| {
                HalldyllError::State(StateError::Corrupted {
                    message: format!("Failed to parse state: {e}"),
                })
            })?;

            info!(
                "Loaded state for project: {}/{}",
                state.project, state.environment
            );
            Ok(Some(state))
        } else {
            debug!("No state found in S3");
            Ok(None)
        }
    }

    async fn save(&self, state: &DeploymentState) -> Result<()> {
        let key = self.key(STATE_KEY);
        info!("Saving state to s3://{}/{key}", self.bucket);

        let content = serde_json::to_string_pretty(state).map_err(|e| {
            HalldyllError::State(StateError::serialization(format!(
                "Failed to serialize state: {e}"
            )))
        })?;

        self.put_object(&key, &content).await?;

        debug!("State saved successfully to S3");
        Ok(())
    }

    async fn delete(&self) -> Result<()> {
        let state_key = self.key(STATE_KEY);
        let lock_key = self.key(LOCK_KEY);

        info!("Deleting state from s3://{}/{state_key}", self.bucket);

        self.delete_object(&state_key).await?;
        self.delete_object(&lock_key).await?;

        Ok(())
    }

    async fn exists(&self) -> Result<bool> {
        let key = self.key(STATE_KEY);
        self.object_exists(&key).await
    }

    async fn acquire_lock(&self, holder: &str) -> Result<LockInfo> {
        let key = self.key(LOCK_KEY);

        // Check for existing lock
        if let Some(content) = self.get_object(&key).await? {
            let existing: LockInfo = serde_json::from_str(&content).map_err(|e| {
                HalldyllError::State(StateError::Corrupted {
                    message: format!("Failed to parse lock: {e}"),
                })
            })?;

            if !existing.is_expired() {
                return Err(HalldyllError::State(StateError::LockedByOther {
                    holder: existing.holder.clone(),
                    since: existing.acquired_at.to_rfc3339(),
                }));
            }
            debug!("Expired lock found, taking over");
        }

        let holder_id = if holder.is_empty() {
            generate_holder_id()
        } else {
            holder.to_string()
        };

        let lock_info = LockInfo::new(&holder_id);

        let content = serde_json::to_string_pretty(&lock_info).map_err(|e| {
            HalldyllError::State(StateError::serialization(format!(
                "Failed to serialize lock: {e}"
            )))
        })?;

        self.put_object(&key, &content).await?;

        info!(
            "Acquired state lock: {} (expires in {}s)",
            lock_info.lock_id, LOCK_EXPIRY_SECS
        );

        Ok(lock_info)
    }

    async fn release_lock(&self, lock_id: &str) -> Result<()> {
        let key = self.key(LOCK_KEY);

        if let Some(content) = self.get_object(&key).await? {
            let existing: LockInfo = serde_json::from_str(&content).map_err(|e| {
                HalldyllError::State(StateError::Corrupted {
                    message: format!("Failed to parse lock: {e}"),
                })
            })?;

            if existing.lock_id == lock_id {
                self.delete_object(&key).await?;
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
        let key = self.key(LOCK_KEY);

        if let Some(content) = self.get_object(&key).await? {
            let lock_info: LockInfo = serde_json::from_str(&content).map_err(|e| {
                HalldyllError::State(StateError::Corrupted {
                    message: format!("Failed to parse lock: {e}"),
                })
            })?;

            return Ok(Some(lock_info));
        }

        Ok(None)
    }

    async fn is_locked(&self) -> Result<bool> {
        if let Some(lock_info) = self.get_lock_info().await? {
            return Ok(!lock_info.is_expired());
        }
        Ok(false)
    }

    fn backend_type(&self) -> &'static str {
        "s3"
    }
}
