//! Error types for the Halldyll deployment system.
//!
//! This module provides a comprehensive error hierarchy for all operations
//! in the deployment lifecycle: configuration, state management, `RunPod` API,
//! planning, and reconciliation.

use std::path::PathBuf;
use thiserror::Error;

/// The main error type for the Halldyll deployment system.
#[derive(Debug, Error)]
pub enum HalldyllError {
    /// Configuration-related errors.
    #[error("Configuration error: {0}")]
    Config(#[from] ConfigError),

    /// State management errors.
    #[error("State error: {0}")]
    State(#[from] StateError),

    /// `RunPod` API errors.
    #[error("RunPod API error: {0}")]
    RunPod(#[from] RunPodError),

    /// Planning errors.
    #[error("Planning error: {0}")]
    Plan(#[from] PlanError),

    /// Reconciliation errors.
    #[error("Reconciliation error: {0}")]
    Reconcile(#[from] ReconcileError),

    /// IO errors.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Generic internal error.
    #[error("Internal error: {0}")]
    Internal(String),
}

/// Configuration-related errors.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// The configuration file was not found.
    #[error("Configuration file not found: {path}")]
    FileNotFound {
        /// Path to the missing file.
        path: PathBuf,
    },

    /// The configuration file could not be parsed.
    #[error("Failed to parse configuration: {message}")]
    ParseError {
        /// Description of the parse error.
        message: String,
        /// Optional source location.
        location: Option<String>,
    },

    /// Validation failed.
    #[error("Configuration validation failed: {message}")]
    ValidationError {
        /// Description of the validation error.
        message: String,
        /// Field that failed validation.
        field: Option<String>,
    },

    /// Environment variable is missing.
    #[error("Missing environment variable: {name}")]
    MissingEnvVar {
        /// Name of the missing variable.
        name: String,
    },

    /// Duplicate resource definition.
    #[error("Duplicate {resource_type} name: {name}")]
    DuplicateName {
        /// Type of resource (pod, volume, etc.).
        resource_type: String,
        /// The duplicated name.
        name: String,
    },

    /// Invalid GPU type.
    #[error("Invalid GPU type: {gpu_type}")]
    InvalidGpuType {
        /// The invalid GPU type string.
        gpu_type: String,
    },

    /// Invalid port specification.
    #[error("Invalid port specification: {spec}")]
    InvalidPort {
        /// The invalid port specification.
        spec: String,
    },

    /// Circular dependency detected.
    #[error("Circular dependency detected: {cycle}")]
    CircularDependency {
        /// Description of the cycle.
        cycle: String,
    },
}

/// State management errors.
#[derive(Debug, Error)]
pub enum StateError {
    /// State file not found.
    #[error("State file not found: {path}")]
    NotFound {
        /// Path to the missing state file.
        path: PathBuf,
    },

    /// State is corrupted.
    #[error("State is corrupted: {message}")]
    Corrupted {
        /// Description of the corruption.
        message: String,
    },

    /// State lock acquisition failed.
    #[error("Failed to acquire state lock: {message}")]
    LockFailed {
        /// Description of the lock failure.
        message: String,
    },

    /// State lock is held by another process.
    #[error("State is locked by another process (lock holder: {holder}, since: {since})")]
    LockedByOther {
        /// Identifier of the lock holder.
        holder: String,
        /// When the lock was acquired.
        since: String,
    },

    /// S3 backend error.
    #[error("S3 state backend error: {message}")]
    S3Error {
        /// Description of the S3 error.
        message: String,
    },

    /// Serialization error.
    #[error("State serialization error: {message}")]
    SerializationError {
        /// Description of the serialization error.
        message: String,
    },

    /// State version mismatch.
    #[error("State version mismatch: expected {expected}, found {found}")]
    VersionMismatch {
        /// Expected state version.
        expected: String,
        /// Found state version.
        found: String,
    },
}

/// `RunPod` API errors.
#[derive(Debug, Error)]
pub enum RunPodError {
    /// Authentication failed.
    #[error("RunPod authentication failed: {message}")]
    AuthenticationFailed {
        /// Description of the auth failure.
        message: String,
    },

    /// API request failed.
    #[error("RunPod API request failed: {status} - {message}")]
    ApiRequestFailed {
        /// HTTP status code.
        status: u16,
        /// Error message from API.
        message: String,
    },

    /// Rate limited.
    #[error("RunPod API rate limited, retry after {retry_after_secs} seconds")]
    RateLimited {
        /// Seconds to wait before retrying.
        retry_after_secs: u64,
    },

    /// Pod not found.
    #[error("Pod not found: {pod_id}")]
    PodNotFound {
        /// ID of the missing pod.
        pod_id: String,
    },

    /// GPU type not available.
    #[error("GPU type not available: {gpu_type} in region {region}")]
    GpuNotAvailable {
        /// Requested GPU type.
        gpu_type: String,
        /// Requested region.
        region: String,
    },

    /// Insufficient quota.
    #[error("Insufficient quota: {message}")]
    InsufficientQuota {
        /// Description of the quota issue.
        message: String,
    },

    /// Network error.
    #[error("Network error communicating with RunPod: {message}")]
    NetworkError {
        /// Description of the network error.
        message: String,
    },

    /// Invalid response from API.
    #[error("Invalid response from RunPod API: {message}")]
    InvalidResponse {
        /// Description of the response issue.
        message: String,
    },

    /// Timeout waiting for pod.
    #[error("Timeout waiting for pod {pod_id} to reach state {expected_state}")]
    Timeout {
        /// ID of the pod.
        pod_id: String,
        /// Expected state that was not reached.
        expected_state: String,
    },
}

/// Planning errors.
#[derive(Debug, Error)]
pub enum PlanError {
    /// Plan is empty (nothing to do).
    #[error("Plan is empty: no changes required")]
    EmptyPlan,

    /// Plan would exceed budget.
    #[error("Plan would exceed budget: estimated ${estimated:.2}/hr, limit ${limit:.2}/hr")]
    BudgetExceeded {
        /// Estimated hourly cost.
        estimated: f64,
        /// Budget limit.
        limit: f64,
    },

    /// Plan would exceed GPU quota.
    #[error("Plan would exceed GPU quota: needs {needed}, available {available}")]
    GpuQuotaExceeded {
        /// Number of GPUs needed.
        needed: u32,
        /// Number of GPUs available.
        available: u32,
    },

    /// Conflicting operations in plan.
    #[error("Conflicting operations in plan: {message}")]
    ConflictingOperations {
        /// Description of the conflict.
        message: String,
    },

    /// Dependency resolution failed.
    #[error("Failed to resolve dependencies: {message}")]
    DependencyResolutionFailed {
        /// Description of the dependency issue.
        message: String,
    },
}

/// Reconciliation errors.
#[derive(Debug, Error)]
pub enum ReconcileError {
    /// Reconciliation failed for a specific resource.
    #[error("Failed to reconcile {resource_type} '{name}': {reason}")]
    ResourceReconcileFailed {
        /// Type of resource.
        resource_type: String,
        /// Name of the resource.
        name: String,
        /// Reason for failure.
        reason: String,
    },

    /// Maximum retry attempts exceeded.
    #[error("Maximum retry attempts ({attempts}) exceeded for {resource}")]
    MaxRetriesExceeded {
        /// Number of attempts made.
        attempts: u32,
        /// Resource that failed.
        resource: String,
    },

    /// Drift detected but auto-reconcile is disabled.
    #[error("Drift detected for {resource}: {drift_description}")]
    DriftDetected {
        /// Resource with drift.
        resource: String,
        /// Description of the drift.
        drift_description: String,
    },

    /// Reconciliation was aborted.
    #[error("Reconciliation aborted: {reason}")]
    Aborted {
        /// Reason for abort.
        reason: String,
    },
}

/// Result type alias for Halldyll operations.
pub type Result<T> = std::result::Result<T, HalldyllError>;

impl HalldyllError {
    /// Creates a new internal error with the given message.
    #[must_use]
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }

    /// Returns true if this error is retryable.
    #[must_use]
    pub const fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::RunPod(
                RunPodError::RateLimited { .. } | RunPodError::NetworkError { .. }
            ) | Self::State(StateError::LockFailed { .. })
        )
    }

    /// Returns the suggested retry delay in seconds, if applicable.
    #[must_use]
    pub const fn retry_delay_secs(&self) -> Option<u64> {
        match self {
            Self::RunPod(RunPodError::RateLimited { retry_after_secs }) => Some(*retry_after_secs),
            Self::RunPod(RunPodError::NetworkError { .. }) => Some(5),
            Self::State(StateError::LockFailed { .. }) => Some(2),
            _ => None,
        }
    }
}

impl ConfigError {
    /// Creates a validation error for a specific field.
    #[must_use]
    pub fn validation(message: impl Into<String>, field: impl Into<String>) -> Self {
        Self::ValidationError {
            message: message.into(),
            field: Some(field.into()),
        }
    }

    /// Creates a validation error without a specific field.
    #[must_use]
    pub fn validation_general(message: impl Into<String>) -> Self {
        Self::ValidationError {
            message: message.into(),
            field: None,
        }
    }
}

impl StateError {
    /// Creates an S3 error with the given message.
    #[must_use]
    pub fn s3(message: impl Into<String>) -> Self {
        Self::S3Error {
            message: message.into(),
        }
    }

    /// Creates a serialization error with the given message.
    #[must_use]
    pub fn serialization(message: impl Into<String>) -> Self {
        Self::SerializationError {
            message: message.into(),
        }
    }
}

impl RunPodError {
    /// Creates an API request error.
    #[must_use]
    pub fn api_error(status: u16, message: impl Into<String>) -> Self {
        Self::ApiRequestFailed {
            status,
            message: message.into(),
        }
    }

    /// Creates a network error.
    #[must_use]
    pub fn network(message: impl Into<String>) -> Self {
        Self::NetworkError {
            message: message.into(),
        }
    }
}
