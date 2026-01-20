//! State types for tracking deployment state.
//!
//! These types represent the observed/recorded state of deployments,
//! used for reconciliation and idempotent operations.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Current version of the state format.
pub const STATE_VERSION: &str = "1.0";

/// The complete deployment state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentState {
    /// State format version.
    pub version: String,
    /// Project name.
    pub project: String,
    /// Environment name.
    pub environment: String,
    /// Hash of the last applied configuration.
    pub config_hash: String,
    /// State of individual pods.
    pub pods: HashMap<String, PodState>,
    /// State of persistent volumes.
    pub volumes: HashMap<String, VolumeState>,
    /// When the state was last updated.
    pub last_updated: DateTime<Utc>,
    /// Deployment history (recent entries).
    #[serde(default)]
    pub history: Vec<DeploymentHistoryEntry>,
}

/// State of a single pod.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodState {
    /// Local pod name (from config).
    pub name: String,
    /// `RunPod` pod ID.
    pub runpod_id: String,
    /// Hash of the pod configuration when deployed.
    pub config_hash: String,
    /// Current status.
    pub status: DeploymentStatus,
    /// GPU type actually allocated.
    pub gpu_type: String,
    /// Number of GPUs.
    pub gpu_count: u32,
    /// Container image deployed.
    pub image: String,
    /// Public endpoints (port -> URL mapping).
    pub endpoints: HashMap<u16, String>,
    /// When the pod was created.
    pub created_at: DateTime<Utc>,
    /// When the pod was last updated.
    pub updated_at: DateTime<Utc>,
    /// Tags applied to the pod.
    pub tags: HashMap<String, String>,
}

/// State of a persistent volume.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeState {
    /// Volume name.
    pub name: String,
    /// `RunPod` volume ID.
    pub runpod_id: String,
    /// Mount path.
    pub mount_path: String,
    /// Size in GB.
    pub size_gb: u32,
    /// When the volume was created.
    pub created_at: DateTime<Utc>,
}

/// Deployment status.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DeploymentStatus {
    /// Pod is being created.
    Creating,
    /// Pod is running.
    Running,
    /// Pod is stopped.
    Stopped,
    /// Pod has an error.
    Error,
    /// Pod is being deleted.
    Deleting,
    /// Pod was deleted.
    Deleted,
    /// Status is unknown.
    Unknown,
}

/// A single entry in the deployment history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentHistoryEntry {
    /// When the deployment occurred.
    pub timestamp: DateTime<Utc>,
    /// Type of operation.
    pub operation: DeploymentOperation,
    /// Configuration hash at time of deployment.
    pub config_hash: String,
    /// Resources affected.
    pub resources: Vec<String>,
    /// Whether the deployment succeeded.
    pub success: bool,
    /// Optional error message.
    #[serde(default)]
    pub error: Option<String>,
}

/// Types of deployment operations.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentOperation {
    /// Initial deployment.
    Create,
    /// Configuration update.
    Update,
    /// Scale operation.
    Scale,
    /// Reconciliation.
    Reconcile,
    /// Destruction.
    Destroy,
}

impl DeploymentState {
    /// Creates a new empty deployment state.
    #[must_use]
    pub fn new(project: &str, environment: &str) -> Self {
        Self {
            version: STATE_VERSION.to_string(),
            project: project.to_string(),
            environment: environment.to_string(),
            config_hash: String::new(),
            pods: HashMap::new(),
            volumes: HashMap::new(),
            last_updated: Utc::now(),
            history: Vec::new(),
        }
    }

    /// Gets a pod by name.
    #[must_use]
    pub fn get_pod(&self, name: &str) -> Option<&PodState> {
        self.pods.get(name)
    }

    /// Gets a mutable reference to a pod by name.
    pub fn get_pod_mut(&mut self, name: &str) -> Option<&mut PodState> {
        self.pods.get_mut(name)
    }

    /// Adds or updates a pod.
    pub fn set_pod(&mut self, pod: PodState) {
        self.pods.insert(pod.name.clone(), pod);
        self.last_updated = Utc::now();
    }

    /// Removes a pod by name.
    pub fn remove_pod(&mut self, name: &str) -> Option<PodState> {
        let result = self.pods.remove(name);
        if result.is_some() {
            self.last_updated = Utc::now();
        }
        result
    }

    /// Gets a volume by name.
    #[must_use]
    pub fn get_volume(&self, name: &str) -> Option<&VolumeState> {
        self.volumes.get(name)
    }

    /// Adds or updates a volume.
    pub fn set_volume(&mut self, volume: VolumeState) {
        self.volumes.insert(volume.name.clone(), volume);
        self.last_updated = Utc::now();
    }

    /// Adds a history entry.
    pub fn add_history(&mut self, entry: DeploymentHistoryEntry) {
        // Keep only the last 100 entries
        const MAX_HISTORY: usize = 100;
        if self.history.len() >= MAX_HISTORY {
            self.history.remove(0);
        }
        self.history.push(entry);
    }

    /// Returns all running pods.
    #[must_use]
    pub fn running_pods(&self) -> Vec<&PodState> {
        self.pods
            .values()
            .filter(|p| p.status == DeploymentStatus::Running)
            .collect()
    }

    /// Returns all pod names.
    #[must_use]
    pub fn pod_names(&self) -> Vec<&str> {
        self.pods.keys().map(String::as_str).collect()
    }
}

impl PodState {
    /// Creates a new pod state.
    #[must_use]
    pub fn new(name: &str, runpod_id: &str, config_hash: &str) -> Self {
        let now = Utc::now();
        Self {
            name: name.to_string(),
            runpod_id: runpod_id.to_string(),
            config_hash: config_hash.to_string(),
            status: DeploymentStatus::Creating,
            gpu_type: String::new(),
            gpu_count: 0,
            image: String::new(),
            endpoints: HashMap::new(),
            created_at: now,
            updated_at: now,
            tags: HashMap::new(),
        }
    }

    /// Updates the status.
    pub fn set_status(&mut self, status: DeploymentStatus) {
        self.status = status;
        self.updated_at = Utc::now();
    }

    /// Adds an endpoint mapping.
    pub fn add_endpoint(&mut self, port: u16, url: String) {
        self.endpoints.insert(port, url);
        self.updated_at = Utc::now();
    }

    /// Checks if the pod is healthy (running).
    #[must_use]
    pub const fn is_healthy(&self) -> bool {
        matches!(self.status, DeploymentStatus::Running)
    }
}

impl DeploymentHistoryEntry {
    /// Creates a new history entry.
    #[must_use]
    pub fn new(operation: DeploymentOperation, config_hash: &str, resources: Vec<String>) -> Self {
        Self {
            timestamp: Utc::now(),
            operation,
            config_hash: config_hash.to_string(),
            resources,
            success: true,
            error: None,
        }
    }

    /// Creates a failed history entry.
    #[must_use]
    pub fn failed(
        operation: DeploymentOperation,
        config_hash: &str,
        resources: Vec<String>,
        error: &str,
    ) -> Self {
        Self {
            timestamp: Utc::now(),
            operation,
            config_hash: config_hash.to_string(),
            resources,
            success: false,
            error: Some(error.to_string()),
        }
    }
}

impl std::fmt::Display for DeploymentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let status = match self {
            Self::Creating => "creating",
            Self::Running => "running",
            Self::Stopped => "stopped",
            Self::Error => "error",
            Self::Deleting => "deleting",
            Self::Deleted => "deleted",
            Self::Unknown => "unknown",
        };
        write!(f, "{status}")
    }
}

impl std::fmt::Display for DeploymentOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let op = match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Scale => "scale",
            Self::Reconcile => "reconcile",
            Self::Destroy => "destroy",
        };
        write!(f, "{op}")
    }
}
