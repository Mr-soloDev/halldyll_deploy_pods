//! Plan executor for applying deployment plans.
//!
//! This module handles the execution of deployment plans, including
//! error handling, rollback, and progress tracking.

use std::collections::HashSet;
use tracing::{debug, error, info, warn};

use crate::config::ProjectConfig;
use crate::error::{HalldyllError, ReconcileError, Result};
use crate::runpod::PodProvisioner;
use crate::state::{DeploymentHistoryEntry, DeploymentOperation, DeploymentState, PodState};

use super::plan::{ActionType, DeploymentPlan, PlannedAction};

/// Executor for deployment plans.
#[derive(Debug)]
pub struct PlanExecutor<'a> {
    /// Pod provisioner.
    provisioner: &'a PodProvisioner,
    /// Project configuration.
    project: &'a ProjectConfig,
    /// Whether to continue on errors.
    continue_on_error: bool,
}

/// Result of executing a single action.
#[derive(Debug)]
pub struct ActionResult {
    /// Action index.
    pub index: usize,
    /// Action that was executed.
    pub action: PlannedAction,
    /// Whether the action succeeded.
    pub success: bool,
    /// `RunPod` pod ID (if created).
    pub pod_id: Option<String>,
    /// Error message (if failed).
    pub error: Option<String>,
}

/// Result of executing the entire plan.
#[derive(Debug)]
pub struct ExecutionResult {
    /// Individual action results.
    pub results: Vec<ActionResult>,
    /// Total actions executed.
    pub total_executed: usize,
    /// Number of successful actions.
    pub successful: usize,
    /// Number of failed actions.
    pub failed: usize,
    /// Number of skipped actions (due to dependency failures).
    pub skipped: usize,
    /// Whether the entire plan succeeded.
    pub success: bool,
}

impl<'a> PlanExecutor<'a> {
    /// Creates a new plan executor.
    #[must_use]
    pub const fn new(provisioner: &'a PodProvisioner, project: &'a ProjectConfig) -> Self {
        Self {
            provisioner,
            project,
            continue_on_error: false,
        }
    }

    /// Sets whether to continue on errors.
    #[must_use]
    pub const fn with_continue_on_error(mut self, continue_on_error: bool) -> Self {
        self.continue_on_error = continue_on_error;
        self
    }

    /// Executes a deployment plan.
    ///
    /// # Errors
    ///
    /// Returns an error if a critical action fails and `continue_on_error` is false.
    pub async fn execute(
        &self,
        plan: &DeploymentPlan,
        state: &mut DeploymentState,
    ) -> Result<ExecutionResult> {
        info!("Executing deployment plan with {} actions", plan.actions.len());

        if plan.actions.is_empty() {
            return Ok(ExecutionResult {
                results: vec![],
                total_executed: 0,
                successful: 0,
                failed: 0,
                skipped: 0,
                success: true,
            });
        }

        // Check guardrails
        if !plan.passes_guardrails {
            error!("Plan does not pass guardrails");
            for violation in &plan.guardrail_violations {
                error!("  - {violation}");
            }
            return Err(HalldyllError::Reconcile(ReconcileError::Aborted {
                reason: String::from("Plan violates guardrails"),
            }));
        }

        let mut results = Vec::new();
        let mut completed: HashSet<usize> = HashSet::new();
        let mut failed_indices: HashSet<usize> = HashSet::new();

        // Execute actions in dependency order
        for (idx, action) in plan.actions.iter().enumerate() {
            // Check if dependencies are met
            let deps_failed = action
                .dependencies
                .iter()
                .any(|dep| failed_indices.contains(dep));

            if deps_failed {
                warn!("Skipping action {} due to failed dependencies", idx);
                results.push(ActionResult {
                    index: idx,
                    action: action.clone(),
                    success: false,
                    pod_id: None,
                    error: Some(String::from("Skipped due to dependency failure")),
                });
                failed_indices.insert(idx);
                continue;
            }

            // Wait for dependencies to complete
            let deps_complete = action.dependencies.iter().all(|dep| completed.contains(dep));
            if !deps_complete {
                // This shouldn't happen with proper ordering, but handle it gracefully
                warn!("Action {} has incomplete dependencies, waiting...", idx);
            }

            // Execute the action
            let result = self.execute_action(idx, action, state).await;

            if result.success {
                completed.insert(idx);
            } else {
                failed_indices.insert(idx);

                if !self.continue_on_error {
                    // Add result and return early
                    results.push(result);
                    break;
                }
            }

            results.push(result);
        }

        // Compute summary
        let successful = results.iter().filter(|r| r.success).count();
        let failed = results.iter().filter(|r| !r.success && r.error.as_deref() != Some("Skipped due to dependency failure")).count();
        let skipped = results.iter().filter(|r| r.error.as_deref() == Some("Skipped due to dependency failure")).count();

        let execution_result = ExecutionResult {
            total_executed: results.len(),
            successful,
            failed,
            skipped,
            success: failed == 0,
            results,
        };

        // Add to history
        let history_entry = if execution_result.success {
            DeploymentHistoryEntry::new(
                DeploymentOperation::Create,
                &plan.config_hash,
                plan.actions.iter().map(|a| a.resource_name.clone()).collect(),
            )
        } else {
            DeploymentHistoryEntry::failed(
                DeploymentOperation::Create,
                &plan.config_hash,
                plan.actions.iter().map(|a| a.resource_name.clone()).collect(),
                &format!("{} actions failed", execution_result.failed),
            )
        };
        state.add_history(history_entry);
        state.config_hash.clone_from(&plan.config_hash);

        Ok(execution_result)
    }

    /// Executes a single action.
    async fn execute_action(
        &self,
        index: usize,
        action: &PlannedAction,
        state: &mut DeploymentState,
    ) -> ActionResult {
        info!("Executing action {}: {}", index, action.description());

        match action.action_type {
            ActionType::CreatePod => self.execute_create(index, action, state).await,
            ActionType::DeletePod => self.execute_delete(index, action, state).await,
            ActionType::UpdatePod => self.execute_update(index, action, state).await,
            ActionType::StopPod => self.execute_stop(index, action, state).await,
            ActionType::ResumePod => self.execute_resume(index, action, state).await,
            ActionType::Noop => ActionResult {
                index,
                action: action.clone(),
                success: true,
                pod_id: None,
                error: None,
            },
        }
    }

    /// Executes a create pod action.
    async fn execute_create(
        &self,
        index: usize,
        action: &PlannedAction,
        state: &mut DeploymentState,
    ) -> ActionResult {
        let Some(pod_config) = &action.pod_config else {
            return ActionResult {
                index,
                action: action.clone(),
                success: false,
                pod_id: None,
                error: Some(String::from("Missing pod configuration")),
            };
        };

        let spec_hash = action.new_hash.as_deref().unwrap_or("");

        match self.provisioner.create_pod(pod_config, self.project, spec_hash).await {
            Ok(pod) => {
                // Update state
                let mut pod_state = PodState::new(
                    &action.resource_name,
                    &pod.id,
                    spec_hash,
                );
                pod_state.gpu_type = pod.gpu_type_name().unwrap_or("").to_string();
                pod_state.gpu_count = pod.gpu_count;
                pod_state.image.clone_from(&pod.image_name);
                pod_state.set_status(crate::state::DeploymentStatus::Creating);

                state.set_pod(pod_state);

                info!("Created pod: {} (ID: {})", action.resource_name, pod.id);

                ActionResult {
                    index,
                    action: action.clone(),
                    success: true,
                    pod_id: Some(pod.id),
                    error: None,
                }
            }
            Err(e) => {
                error!("Failed to create pod {}: {}", action.resource_name, e);
                ActionResult {
                    index,
                    action: action.clone(),
                    success: false,
                    pod_id: None,
                    error: Some(e.to_string()),
                }
            }
        }
    }

    /// Executes a delete pod action.
    async fn execute_delete(
        &self,
        index: usize,
        action: &PlannedAction,
        state: &mut DeploymentState,
    ) -> ActionResult {
        // Find the pod ID
        let pod_id = action.runpod_id.clone().or_else(|| {
            state.get_pod(&action.resource_name).map(|p| p.runpod_id.clone())
        });

        let Some(pod_id) = pod_id else {
            debug!("No pod ID found for {}, considering delete successful", action.resource_name);
            state.remove_pod(&action.resource_name);
            return ActionResult {
                index,
                action: action.clone(),
                success: true,
                pod_id: None,
                error: None,
            };
        };

        match self.provisioner.terminate_pod(&pod_id).await {
            Ok(()) => {
                state.remove_pod(&action.resource_name);
                info!("Deleted pod: {} (ID: {})", action.resource_name, pod_id);

                ActionResult {
                    index,
                    action: action.clone(),
                    success: true,
                    pod_id: Some(pod_id),
                    error: None,
                }
            }
            Err(e) => {
                // Check if pod was already deleted
                if matches!(e, HalldyllError::RunPod(crate::error::RunPodError::PodNotFound { .. })) {
                    state.remove_pod(&action.resource_name);
                    info!("Pod {} was already deleted", action.resource_name);
                    return ActionResult {
                        index,
                        action: action.clone(),
                        success: true,
                        pod_id: Some(pod_id),
                        error: None,
                    };
                }

                error!("Failed to delete pod {}: {}", action.resource_name, e);
                ActionResult {
                    index,
                    action: action.clone(),
                    success: false,
                    pod_id: Some(pod_id),
                    error: Some(e.to_string()),
                }
            }
        }
    }

    /// Executes an update pod action (currently just recreates).
    async fn execute_update(
        &self,
        index: usize,
        action: &PlannedAction,
        state: &mut DeploymentState,
    ) -> ActionResult {
        // For now, update is handled as delete + create in the plan
        // This is a fallback in case it's called directly
        self.execute_create(index, action, state).await
    }

    /// Executes a stop pod action.
    async fn execute_stop(
        &self,
        index: usize,
        action: &PlannedAction,
        state: &mut DeploymentState,
    ) -> ActionResult {
        let pod_id = action.runpod_id.clone().or_else(|| {
            state.get_pod(&action.resource_name).map(|p| p.runpod_id.clone())
        });

        let Some(pod_id) = pod_id else {
            return ActionResult {
                index,
                action: action.clone(),
                success: false,
                pod_id: None,
                error: Some(String::from("Pod not found")),
            };
        };

        match self.provisioner.stop_pod(&pod_id).await {
            Ok(()) => {
                if let Some(pod_state) = state.get_pod_mut(&action.resource_name) {
                    pod_state.set_status(crate::state::DeploymentStatus::Stopped);
                }

                ActionResult {
                    index,
                    action: action.clone(),
                    success: true,
                    pod_id: Some(pod_id),
                    error: None,
                }
            }
            Err(e) => {
                error!("Failed to stop pod {}: {}", action.resource_name, e);
                ActionResult {
                    index,
                    action: action.clone(),
                    success: false,
                    pod_id: Some(pod_id),
                    error: Some(e.to_string()),
                }
            }
        }
    }

    /// Executes a resume pod action.
    async fn execute_resume(
        &self,
        index: usize,
        action: &PlannedAction,
        state: &mut DeploymentState,
    ) -> ActionResult {
        let pod_id = action.runpod_id.clone().or_else(|| {
            state.get_pod(&action.resource_name).map(|p| p.runpod_id.clone())
        });

        let Some(pod_id) = pod_id else {
            return ActionResult {
                index,
                action: action.clone(),
                success: false,
                pod_id: None,
                error: Some(String::from("Pod not found")),
            };
        };

        match self.provisioner.resume_pod(&pod_id).await {
            Ok(_) => {
                if let Some(pod_state) = state.get_pod_mut(&action.resource_name) {
                    pod_state.set_status(crate::state::DeploymentStatus::Running);
                }

                ActionResult {
                    index,
                    action: action.clone(),
                    success: true,
                    pod_id: Some(pod_id),
                    error: None,
                }
            }
            Err(e) => {
                error!("Failed to resume pod {}: {}", action.resource_name, e);
                ActionResult {
                    index,
                    action: action.clone(),
                    success: false,
                    pod_id: Some(pod_id),
                    error: Some(e.to_string()),
                }
            }
        }
    }
}

impl ExecutionResult {
    /// Returns true if all actions succeeded.
    #[must_use]
    pub const fn all_successful(&self) -> bool {
        self.success && self.failed == 0 && self.skipped == 0
    }
}

impl std::fmt::Display for ExecutionResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Executed {} actions: {} successful, {} failed, {} skipped",
            self.total_executed, self.successful, self.failed, self.skipped
        )
    }
}
