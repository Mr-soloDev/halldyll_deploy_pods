//! Deployment plan types and construction.
//!
//! This module defines the structure of deployment plans and provides
//! functionality for converting diffs into executable plans.

use chrono::{DateTime, Utc};

use crate::config::{DeployConfig, GuardrailsConfig, PodConfig};

use super::diff::{DiffResult, DiffType};

/// A complete deployment plan.
#[derive(Debug)]
pub struct DeploymentPlan {
    /// When the plan was created.
    pub created_at: DateTime<Utc>,
    /// Configuration hash this plan is based on.
    pub config_hash: String,
    /// Planned actions in execution order.
    pub actions: Vec<PlannedAction>,
    /// Estimated hourly cost delta (positive = increase).
    pub estimated_cost_delta: Option<f64>,
    /// Whether the plan passes guardrails.
    pub passes_guardrails: bool,
    /// Guardrail violations (if any).
    pub guardrail_violations: Vec<String>,
}

/// A single planned action.
#[derive(Debug, Clone)]
pub struct PlannedAction {
    /// Action type.
    pub action_type: ActionType,
    /// Resource name.
    pub resource_name: String,
    /// Pod configuration (if applicable).
    pub pod_config: Option<PodConfig>,
    /// `RunPod` pod ID (if applicable).
    pub runpod_id: Option<String>,
    /// Reason for this action.
    pub reason: String,
    /// New spec hash (if applicable).
    pub new_hash: Option<String>,
    /// Dependencies (action indices that must complete first).
    pub dependencies: Vec<usize>,
}

/// Types of actions in a plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionType {
    /// Create a new pod.
    CreatePod,
    /// Update an existing pod (recreate with new config).
    UpdatePod,
    /// Delete a pod.
    DeletePod,
    /// Stop a pod.
    StopPod,
    /// Resume a stopped pod.
    ResumePod,
    /// No operation (for tracking).
    Noop,
}

impl DeploymentPlan {
    /// Creates a new plan from a diff result.
    #[must_use]
    pub fn from_diff(
        diff: &DiffResult,
        config: &DeployConfig,
        config_hash: &str,
    ) -> Self {
        let mut actions = Vec::new();

        // Process deletes first
        for resource_diff in &diff.diffs {
            if resource_diff.diff_type == DiffType::Delete {
                actions.push(PlannedAction {
                    action_type: ActionType::DeletePod,
                    resource_name: resource_diff.name.clone(),
                    pod_config: None,
                    runpod_id: resource_diff
                        .details
                        .first()
                        .and_then(|d| d.old_value.clone()),
                    reason: String::from("Pod removed from configuration"),
                    new_hash: None,
                    dependencies: vec![],
                });
            }
        }

        let delete_count = actions.len();

        // Process creates
        for resource_diff in &diff.diffs {
            if resource_diff.diff_type == DiffType::Create
                && let Some(pod_config) = config.pods.iter().find(|p| p.name == resource_diff.name) {
                    actions.push(PlannedAction {
                        action_type: ActionType::CreatePod,
                        resource_name: resource_diff.name.clone(),
                        pod_config: Some(pod_config.clone()),
                        runpod_id: None,
                        reason: String::from("Pod defined in configuration"),
                        new_hash: resource_diff.new_hash.clone(),
                        dependencies: vec![], // Creates can run in parallel
                    });
                }
        }

        // Process updates (recreate strategy)
        for (i, resource_diff) in diff.diffs.iter().enumerate() {
            if matches!(resource_diff.diff_type, DiffType::Update | DiffType::Drift)
                && let Some(pod_config) = config.pods.iter().find(|p| p.name == resource_diff.name) {
                    // Add delete action
                    let delete_idx = actions.len();
                    actions.push(PlannedAction {
                        action_type: ActionType::DeletePod,
                        resource_name: resource_diff.name.clone(),
                        pod_config: None,
                        runpod_id: diff.diffs.get(i).and_then(|d| {
                            d.details.first().and_then(|det| det.old_value.clone())
                        }),
                        reason: format!("Recreating pod due to {}", resource_diff.diff_type),
                        new_hash: None,
                        dependencies: vec![], // Can start immediately
                    });

                    // Add create action (depends on delete)
                    actions.push(PlannedAction {
                        action_type: ActionType::CreatePod,
                        resource_name: resource_diff.name.clone(),
                        pod_config: Some(pod_config.clone()),
                        runpod_id: None,
                        reason: format!("Recreating pod due to {}", resource_diff.diff_type),
                        new_hash: resource_diff.new_hash.clone(),
                        dependencies: vec![delete_idx],
                    });
                }
        }

        // Check guardrails
        let (passes_guardrails, guardrail_violations) =
            Self::check_guardrails(config, &actions, delete_count);

        Self {
            created_at: Utc::now(),
            config_hash: config_hash.to_string(),
            actions,
            estimated_cost_delta: None,
            passes_guardrails,
            guardrail_violations,
        }
    }

    /// Creates an empty plan (no changes needed).
    #[must_use]
    pub fn empty(config_hash: &str) -> Self {
        Self {
            created_at: Utc::now(),
            config_hash: config_hash.to_string(),
            actions: vec![],
            estimated_cost_delta: Some(0.0),
            passes_guardrails: true,
            guardrail_violations: vec![],
        }
    }

    /// Checks guardrails for the plan.
    fn check_guardrails(
        config: &DeployConfig,
        actions: &[PlannedAction],
        _delete_count: usize,
    ) -> (bool, Vec<String>) {
        let mut violations = Vec::new();

        if let Some(guardrails) = &config.guardrails {
            // Check GPU quota
            if let Some(max_gpus) = guardrails.max_gpus {
                let total_gpus: u32 = actions
                    .iter()
                    .filter_map(|a| a.pod_config.as_ref())
                    .map(|p| p.gpu.count)
                    .sum();

                if total_gpus > max_gpus {
                    violations.push(format!(
                        "Plan requires {total_gpus} GPUs but max_gpus is {max_gpus}"
                    ));
                }
            }

            // Additional guardrail checks can be added here
            Self::check_cost_guardrails(guardrails, actions, &violations);
        }

        (violations.is_empty(), violations)
    }

    /// Checks cost-related guardrails.
    const fn check_cost_guardrails(
        guardrails: &GuardrailsConfig,
        _actions: &[PlannedAction],
        violations: &Vec<String>,
    ) {
        // Placeholder for cost estimation
        // In a real implementation, this would query GPU prices
        if guardrails.max_hourly_cost.is_some() {
            // Cost checking would go here
            // For now, we don't have pricing data
            let _ = violations; // Suppress unused warning
        }
    }

    /// Returns true if the plan is empty (no changes).
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }

    /// Returns the number of actions.
    #[must_use]
    pub const fn action_count(&self) -> usize {
        self.actions.len()
    }

    /// Returns the number of create actions.
    #[must_use]
    pub fn create_count(&self) -> usize {
        self.actions
            .iter()
            .filter(|a| a.action_type == ActionType::CreatePod)
            .count()
    }

    /// Returns the number of delete actions.
    #[must_use]
    pub fn delete_count(&self) -> usize {
        self.actions
            .iter()
            .filter(|a| a.action_type == ActionType::DeletePod)
            .count()
    }

    /// Returns actions that can be executed immediately (no dependencies).
    #[must_use]
    pub fn ready_actions(&self) -> Vec<&PlannedAction> {
        self.actions
            .iter()
            .filter(|a| a.dependencies.is_empty())
            .collect()
    }

    /// Gets actions that depend on a specific action index.
    #[must_use]
    pub fn dependent_actions(&self, action_idx: usize) -> Vec<(usize, &PlannedAction)> {
        self.actions
            .iter()
            .enumerate()
            .filter(|(_, a)| a.dependencies.contains(&action_idx))
            .collect()
    }
}

impl PlannedAction {
    /// Returns a human-readable description of the action.
    #[must_use]
    pub fn description(&self) -> String {
        match self.action_type {
            ActionType::CreatePod => format!("Create pod '{}'", self.resource_name),
            ActionType::UpdatePod => format!("Update pod '{}'", self.resource_name),
            ActionType::DeletePod => format!("Delete pod '{}'", self.resource_name),
            ActionType::StopPod => format!("Stop pod '{}'", self.resource_name),
            ActionType::ResumePod => format!("Resume pod '{}'", self.resource_name),
            ActionType::Noop => format!("No change for '{}'", self.resource_name),
        }
    }
}

impl std::fmt::Display for ActionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::CreatePod => "create",
            Self::UpdatePod => "update",
            Self::DeletePod => "delete",
            Self::StopPod => "stop",
            Self::ResumePod => "resume",
            Self::Noop => "noop",
        };
        write!(f, "{s}")
    }
}

impl std::fmt::Display for PlannedAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.action_type, self.resource_name)?;
        if !self.reason.is_empty() {
            write!(f, " ({})", self.reason)?;
        }
        Ok(())
    }
}

impl std::fmt::Display for DeploymentPlan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.actions.is_empty() {
            return write!(f, "No changes required");
        }

        writeln!(f, "Deployment Plan ({} actions):", self.actions.len())?;
        for (i, action) in self.actions.iter().enumerate() {
            writeln!(f, "  {i}. {action}")?;
        }

        if !self.guardrail_violations.is_empty() {
            writeln!(f, "\nGuardrail violations:")?;
            for violation in &self.guardrail_violations {
                writeln!(f, "  - {violation}")?;
            }
        }

        Ok(())
    }
}
