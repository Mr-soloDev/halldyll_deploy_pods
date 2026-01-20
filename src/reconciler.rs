//! Reconciler for maintaining desired state.
//!
//! This module implements the core reconciliation loop that compares
//! desired state (configuration) with observed state (`RunPod`) and
//! takes corrective actions to converge them.

use tracing::{debug, error, info, warn};

use crate::config::{ConfigHasher, DeployConfig};
use crate::error::{HalldyllError, ReconcileError, Result};
use crate::planner::{DeploymentPlan, DiffEngine, PlanExecutor};
use crate::runpod::{ObservedPod, PodObserver, PodProvisioner};
use crate::state::{DeploymentState, StateStore};

/// Reconciler for maintaining desired state.
pub struct Reconciler<'a, S: StateStore> {
    /// Configuration.
    config: &'a DeployConfig,
    /// State store.
    state_store: &'a S,
    /// Pod provisioner.
    provisioner: &'a PodProvisioner,
    /// Pod observer.
    observer: &'a PodObserver,
    /// Configuration hasher.
    hasher: ConfigHasher,
    /// Diff engine.
    diff_engine: DiffEngine,
    /// Maximum reconciliation attempts.
    max_attempts: u32,
}

/// Result of a reconciliation run.
#[derive(Debug, serde::Serialize)]
pub struct ReconciliationResult {
    /// Whether reconciliation succeeded.
    pub success: bool,
    /// Number of pods created.
    pub created: usize,
    /// Number of pods updated.
    pub updated: usize,
    /// Number of pods deleted.
    pub deleted: usize,
    /// Number of pods unchanged.
    pub unchanged: usize,
    /// Errors encountered.
    pub errors: Vec<String>,
    /// Final state after reconciliation.
    #[serde(skip)]
    pub final_state: Option<DeploymentState>,
}

impl<'a, S: StateStore> Reconciler<'a, S> {
    /// Creates a new reconciler.
    #[must_use]
    pub const fn new(
        config: &'a DeployConfig,
        state_store: &'a S,
        provisioner: &'a PodProvisioner,
        observer: &'a PodObserver,
    ) -> Self {
        Self {
            config,
            state_store,
            provisioner,
            observer,
            hasher: ConfigHasher::new(),
            diff_engine: DiffEngine::new(),
            max_attempts: 3,
        }
    }

    /// Sets the maximum reconciliation attempts.
    #[must_use]
    pub const fn with_max_attempts(mut self, max_attempts: u32) -> Self {
        self.max_attempts = max_attempts;
        self
    }

    /// Performs a full reconciliation.
    ///
    /// # Errors
    ///
    /// Returns an error if reconciliation fails.
    pub async fn reconcile(&self) -> Result<ReconciliationResult> {
        info!(
            "Starting reconciliation for {}/{}",
            self.config.project.name, self.config.project.environment
        );

        let config_hash = self.hasher.hash_config(self.config);

        // Load current state
        let mut state = self
            .state_store
            .load()
            .await?
            .unwrap_or_else(|| {
                DeploymentState::new(&self.config.project.name, &self.config.project.environment)
            });

        // Observe current pods on RunPod
        let observed = self
            .observer
            .list_project_pods(&self.config.project.name, &self.config.project.environment)
            .await?;

        debug!("Found {} existing pods", observed.len());

        // Attempt reconciliation with retries
        let mut last_error = None;
        let mut result = ReconciliationResult {
            success: false,
            created: 0,
            updated: 0,
            deleted: 0,
            unchanged: 0,
            errors: vec![],
            final_state: None,
        };

        for attempt in 1..=self.max_attempts {
            debug!("Reconciliation attempt {}/{}", attempt, self.max_attempts);

            match self
                .reconcile_once(&mut state, &observed, &config_hash)
                .await
            {
                Ok(r) => {
                    result = r;
                    if result.success {
                        break;
                    }
                    // Partial success, might need another attempt
                    if attempt < self.max_attempts {
                        warn!("Reconciliation partially succeeded, retrying...");
                    }
                }
                Err(err) => {
                    error!("Reconciliation attempt {} failed: {}", attempt, err);
                    result.errors.push(format!("Attempt {attempt}: {err}"));
                    last_error = Some(err);

                    if attempt < self.max_attempts {
                        // Wait before retry
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    }
                }
            }
        }

        // Save final state
        if let Err(e) = self.state_store.save(&state).await {
            error!("Failed to save state: {}", e);
            result.errors.push(format!("Failed to save state: {e}"));
        }

        result.final_state = Some(state);

        if !result.success && let Some(err) = last_error {
            return Err(err);
        }

        Ok(result)
    }

    /// Performs a single reconciliation attempt.
    async fn reconcile_once(
        &self,
        state: &mut DeploymentState,
        observed: &[ObservedPod],
        config_hash: &str,
    ) -> Result<ReconciliationResult> {
        // Compute diff
        let diff = self
            .diff_engine
            .compute_diff(self.config, Some(state), observed);

        info!(
            "Diff: {} creates, {} updates, {} deletes, {} unchanged",
            diff.creates, diff.updates, diff.deletes, diff.unchanged
        );

        if !diff.has_changes() {
            info!("No changes required - state is converged");
            return Ok(ReconciliationResult {
                success: true,
                created: 0,
                updated: 0,
                deleted: 0,
                unchanged: diff.unchanged,
                errors: vec![],
                final_state: None,
            });
        }

        // Generate plan
        let plan = DeploymentPlan::from_diff(&diff, self.config, config_hash);

        if !plan.passes_guardrails {
            return Err(HalldyllError::Reconcile(ReconcileError::Aborted {
                reason: format!(
                    "Plan violates guardrails: {}",
                    plan.guardrail_violations.join(", ")
                ),
            }));
        }

        // Execute plan
        let executor = PlanExecutor::new(self.provisioner, &self.config.project)
            .with_continue_on_error(true);

        let execution_result = executor.execute(&plan, state).await?;

        let mut errors: Vec<String> = execution_result
            .results
            .iter()
            .filter(|r| !r.success)
            .filter_map(|r| r.error.clone())
            .collect();

        if !execution_result.success {
            errors.insert(
                0,
                format!(
                    "{} of {} actions failed",
                    execution_result.failed, execution_result.total_executed
                ),
            );
        }

        Ok(ReconciliationResult {
            success: execution_result.success,
            created: diff.creates,
            updated: diff.updates,
            deleted: diff.deletes,
            unchanged: diff.unchanged,
            errors,
            final_state: None,
        })
    }

    /// Checks for drift without applying changes.
    ///
    /// # Errors
    ///
    /// Returns an error if the drift check fails.
    pub async fn check_drift(&self) -> Result<DriftReport> {
        info!(
            "Checking for drift in {}/{}",
            self.config.project.name, self.config.project.environment
        );

        let state = self.state_store.load().await?;

        let observed = self
            .observer
            .list_project_pods(&self.config.project.name, &self.config.project.environment)
            .await?;

        let diff = self
            .diff_engine
            .compute_diff(self.config, state.as_ref(), &observed);

        let drifted_resources: Vec<String> = diff
            .diffs
            .iter()
            .filter(|d| {
                matches!(
                    d.diff_type,
                    crate::planner::DiffType::Drift
                        | crate::planner::DiffType::Update
                        | crate::planner::DiffType::Create
                        | crate::planner::DiffType::Delete
                )
            })
            .map(|d| d.name.clone())
            .collect();

        Ok(DriftReport {
            has_drift: diff.has_changes(),
            drifted_resources,
            total_resources: self.config.pods.len(),
            observed_count: observed.len(),
        })
    }
}

/// Report of drift detection.
#[derive(Debug, serde::Serialize)]
pub struct DriftReport {
    /// Whether drift was detected.
    pub has_drift: bool,
    /// Resources that have drifted.
    pub drifted_resources: Vec<String>,
    /// Total number of resources in config.
    pub total_resources: usize,
    /// Number of resources observed on `RunPod`.
    pub observed_count: usize,
}

impl DriftReport {
    /// Returns true if the state is converged (no drift).
    #[must_use]
    pub const fn is_converged(&self) -> bool {
        !self.has_drift
    }
}

impl std::fmt::Display for DriftReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.has_drift {
            writeln!(f, "Drift detected:")?;
            for resource in &self.drifted_resources {
                writeln!(f, "  - {resource}")?;
            }
        } else {
            write!(f, "No drift detected - state is converged")?;
        }
        Ok(())
    }
}

impl std::fmt::Display for ReconciliationResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let status = if self.success { "successful" } else { "failed" };
        writeln!(f, "Reconciliation {status}:")?;
        writeln!(f, "  Created: {}", self.created)?;
        writeln!(f, "  Updated: {}", self.updated)?;
        writeln!(f, "  Deleted: {}", self.deleted)?;
        writeln!(f, "  Unchanged: {}", self.unchanged)?;

        if !self.errors.is_empty() {
            writeln!(f, "  Errors:")?;
            for error in &self.errors {
                writeln!(f, "    - {error}")?;
            }
        }

        Ok(())
    }
}
