//! State management module for Halldyll deployment system.
//!
//! This module provides persistent state storage for tracking deployed resources,
//! including pod mappings, volume identifiers, and deployment history.

mod store;
mod local;
mod s3;
mod lock;
mod types;

pub use store::StateStore;
pub use local::LocalStateStore;
pub use s3::S3StateStore;
pub use lock::{StateLock, LockInfo};
pub use types::{DeploymentState, PodState, VolumeState, DeploymentStatus, DeploymentHistoryEntry, DeploymentOperation};
