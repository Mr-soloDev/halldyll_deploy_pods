//! Diff engine for comparing desired vs observed state.
//!
//! This module computes the difference between the desired configuration
//! and the observed state on `RunPod`.

use std::collections::HashMap;
use tracing::debug;

use crate::config::{ConfigHasher, DeployConfig, PodConfig};
use crate::runpod::ObservedPod;
use crate::state::DeploymentState;

/// Engine for computing diffs between desired and observed states.
#[derive(Debug, Default)]
pub struct DiffEngine {
    /// Configuration hasher.
    hasher: ConfigHasher,
}

/// Difference for a single resource.
#[derive(Debug, Clone)]
pub struct ResourceDiff {
    /// Resource name.
    pub name: String,
    /// Type of difference.
    pub diff_type: DiffType,
    /// Details about the difference.
    pub details: Vec<DiffDetail>,
    /// Previous hash (if applicable).
    pub old_hash: Option<String>,
    /// New hash (if applicable).
    pub new_hash: Option<String>,
}

/// Type of difference detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffType {
    /// Resource needs to be created.
    Create,
    /// Resource needs to be updated.
    Update,
    /// Resource needs to be deleted.
    Delete,
    /// Resource is unchanged.
    NoChange,
    /// Resource exists but has drifted from config.
    Drift,
}

/// Detail about a specific difference.
#[derive(Debug, Clone)]
pub struct DiffDetail {
    /// Field that differs.
    pub field: String,
    /// Old value.
    pub old_value: Option<String>,
    /// New value.
    pub new_value: Option<String>,
}

/// Complete diff result.
#[derive(Debug)]
pub struct DiffResult {
    /// All resource diffs.
    pub diffs: Vec<ResourceDiff>,
    /// Number of resources to create.
    pub creates: usize,
    /// Number of resources to update.
    pub updates: usize,
    /// Number of resources to delete.
    pub deletes: usize,
    /// Number of unchanged resources.
    pub unchanged: usize,
}

impl DiffEngine {
    /// Creates a new diff engine.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            hasher: ConfigHasher::new(),
        }
    }

    /// Computes the diff between desired config and observed state.
    pub fn compute_diff(
        &self,
        config: &DeployConfig,
        state: Option<&DeploymentState>,
        observed: &[ObservedPod],
    ) -> DiffResult {
        let mut diffs = Vec::new();

        // Build a map of observed pods by name
        let observed_by_name: HashMap<&str, &ObservedPod> = observed
            .iter()
            .filter_map(|p| p.pod_name.as_deref().map(|name| (name, p)))
            .collect();

        // Build a map of state pods by name
        let state_pods: HashMap<&str, _> = state
            .map(|s| s.pods.iter().map(|(k, v)| (k.as_str(), v)).collect())
            .unwrap_or_default();

        // Check each desired pod
        for pod_config in &config.pods {
            let new_hash = self.hasher.hash_pod(pod_config);
            let observed_pod = observed_by_name.get(pod_config.name.as_str());
            let state_pod = state_pods.get(pod_config.name.as_str());

            let diff = Self::compute_pod_diff(pod_config, observed_pod.copied(), state_pod.copied(), &new_hash);
            diffs.push(diff);
        }

        // Check for pods that exist but are not in config (should be deleted)
        for observed_pod in observed {
            if let Some(pod_name) = &observed_pod.pod_name {
                let in_config = config.pods.iter().any(|p| p.name == *pod_name);
                if !in_config {
                    debug!("Found orphaned pod: {pod_name}");
                    diffs.push(ResourceDiff {
                        name: pod_name.clone(),
                        diff_type: DiffType::Delete,
                        details: vec![DiffDetail {
                            field: String::from("pod"),
                            old_value: Some(observed_pod.id.clone()),
                            new_value: None,
                        }],
                        old_hash: observed_pod.spec_hash.clone(),
                        new_hash: None,
                    });
                }
            }
        }

        // Compute summary
        let creates = diffs.iter().filter(|d| d.diff_type == DiffType::Create).count();
        let updates = diffs
            .iter()
            .filter(|d| matches!(d.diff_type, DiffType::Update | DiffType::Drift))
            .count();
        let deletes = diffs.iter().filter(|d| d.diff_type == DiffType::Delete).count();
        let unchanged = diffs.iter().filter(|d| d.diff_type == DiffType::NoChange).count();

        DiffResult {
            diffs,
            creates,
            updates,
            deletes,
            unchanged,
        }
    }

    /// Computes the diff for a single pod.
    fn compute_pod_diff(
        config: &PodConfig,
        observed: Option<&ObservedPod>,
        state: Option<&crate::state::PodState>,
        new_hash: &str,
    ) -> ResourceDiff {
        match (observed, state) {
            // Pod doesn't exist at all - create
            (None, None) => {
                debug!("Pod {} needs to be created", config.name);
                ResourceDiff {
                    name: config.name.clone(),
                    diff_type: DiffType::Create,
                    details: vec![DiffDetail {
                        field: String::from("pod"),
                        old_value: None,
                        new_value: Some(config.name.clone()),
                    }],
                    old_hash: None,
                    new_hash: Some(new_hash.to_string()),
                }
            }

            // Pod exists on RunPod
            (Some(obs), _) => {
                // Check if spec hash matches
                let old_hash = obs.spec_hash.as_deref();

                if old_hash == Some(new_hash) {
                    // Hash matches - no change needed
                    debug!("Pod {} is up to date", config.name);
                    ResourceDiff {
                        name: config.name.clone(),
                        diff_type: DiffType::NoChange,
                        details: vec![],
                        old_hash: old_hash.map(String::from),
                        new_hash: Some(new_hash.to_string()),
                    }
                } else {
                    // Hash differs - compute detailed diff
                    let details = Self::compute_detailed_diff(config, obs);
                    let diff_type = if old_hash.is_some() {
                        DiffType::Update
                    } else {
                        DiffType::Drift
                    };

                    debug!("Pod {} needs update ({:?})", config.name, diff_type);
                    ResourceDiff {
                        name: config.name.clone(),
                        diff_type,
                        details,
                        old_hash: old_hash.map(String::from),
                        new_hash: Some(new_hash.to_string()),
                    }
                }
            }

            // Pod exists in state but not on RunPod - recreate
            (None, Some(st)) => {
                debug!("Pod {} exists in state but not on RunPod, recreating", config.name);
                ResourceDiff {
                    name: config.name.clone(),
                    diff_type: DiffType::Create,
                    details: vec![DiffDetail {
                        field: String::from("pod"),
                        old_value: Some(format!("missing (was {})", st.runpod_id)),
                        new_value: Some(config.name.clone()),
                    }],
                    old_hash: Some(st.config_hash.clone()),
                    new_hash: Some(new_hash.to_string()),
                }
            }
        }
    }

    /// Computes detailed differences between config and observed state.
    fn compute_detailed_diff(config: &PodConfig, observed: &ObservedPod) -> Vec<DiffDetail> {
        let mut details = Vec::new();

        // Check image
        if config.runtime.image != observed.image {
            details.push(DiffDetail {
                field: String::from("image"),
                old_value: Some(observed.image.clone()),
                new_value: Some(config.runtime.image.clone()),
            });
        }

        // Check GPU type
        if let Some(obs_gpu) = &observed.gpu_type
            && config.gpu.gpu_type != *obs_gpu {
                details.push(DiffDetail {
                    field: String::from("gpu_type"),
                    old_value: Some(obs_gpu.clone()),
                    new_value: Some(config.gpu.gpu_type.clone()),
                });
            }

        // Check GPU count
        if config.gpu.count != observed.gpu_count {
            details.push(DiffDetail {
                field: String::from("gpu_count"),
                old_value: Some(observed.gpu_count.to_string()),
                new_value: Some(config.gpu.count.to_string()),
            });
        }

        details
    }
}

impl DiffResult {
    /// Returns true if there are any changes.
    #[must_use]
    pub const fn has_changes(&self) -> bool {
        self.creates > 0 || self.updates > 0 || self.deletes > 0
    }

    /// Returns the total number of changes.
    #[must_use]
    pub const fn total_changes(&self) -> usize {
        self.creates + self.updates + self.deletes
    }

    /// Filters to only diffs that require action.
    #[must_use]
    pub fn actionable_diffs(&self) -> Vec<&ResourceDiff> {
        self.diffs
            .iter()
            .filter(|d| d.diff_type != DiffType::NoChange)
            .collect()
    }
}

impl std::fmt::Display for DiffType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Delete => "delete",
            Self::NoChange => "no change",
            Self::Drift => "drift",
        };
        write!(f, "{s}")
    }
}

impl std::fmt::Display for ResourceDiff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.name, self.diff_type)?;
        if !self.details.is_empty() {
            write!(f, " (")?;
            for (i, detail) in self.details.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "{}", detail.field)?;
            }
            write!(f, ")")?;
        }
        Ok(())
    }
}
