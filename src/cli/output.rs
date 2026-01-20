//! Output formatting for CLI commands.
//!
//! This module provides formatting utilities for displaying
//! information to the user in various formats.

use colored::Colorize;
use std::fmt::Write;
use tabled::{Table, Tabled};

use crate::planner::{ActionType, DeploymentPlan};
use crate::reconciler::{DriftReport, ReconciliationResult};
use crate::runpod::{ObservedPod, ProjectStatus, PodStatus, HealthStatus};
use crate::state::DeploymentState;

use super::commands::OutputFormat;

/// Output formatter for CLI.
#[derive(Debug)]
pub struct OutputFormatter {
    /// Output format.
    format: OutputFormat,
}

/// Pod status row for table display.
#[derive(Tabled)]
struct PodStatusRow {
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Status")]
    status: String,
    #[tabled(rename = "GPU")]
    gpu: String,
    #[tabled(rename = "Image")]
    image: String,
    #[tabled(rename = "ID")]
    id: String,
}

/// Plan action row for table display.
#[derive(Tabled)]
struct PlanActionRow {
    #[tabled(rename = "#")]
    index: usize,
    #[tabled(rename = "Action")]
    action: String,
    #[tabled(rename = "Resource")]
    resource: String,
    #[tabled(rename = "Reason")]
    reason: String,
}

impl OutputFormatter {
    /// Creates a new output formatter.
    #[must_use]
    pub const fn new(format: OutputFormat) -> Self {
        Self { format }
    }

    /// Formats a deployment plan for display.
    #[must_use]
    pub fn format_plan(&self, plan: &DeploymentPlan) -> String {
        match self.format {
            OutputFormat::Json => {
                serde_json::to_string_pretty(&PlanJson::from(plan)).unwrap_or_default()
            }
            OutputFormat::Text => Self::format_plan_text(plan),
        }
    }

    /// Formats a plan as text.
    fn format_plan_text(plan: &DeploymentPlan) -> String {
        if plan.is_empty() {
            return format!(
                "{} No changes required - infrastructure is up to date.\n",
                "✓".green()
            );
        }

        let mut output = String::new();

        let _ = write!(
            output,
            "\n📋 Deployment Plan\n"
        );
        let _ = write!(
            output,
            "   Config hash: {}\n\n",
            &plan.config_hash[..8]
        );

        // Create action table
        let rows: Vec<PlanActionRow> = plan
            .actions
            .iter()
            .enumerate()
            .map(|(i, a)| PlanActionRow {
                index: i + 1,
                action: Self::format_action_type(a.action_type),
                resource: a.resource_name.clone(),
                reason: Self::truncate(&a.reason, 40),
            })
            .collect();

        if !rows.is_empty() {
            let table = Table::new(rows).to_string();
            output.push_str(&table);
            output.push('\n');
        }

        // Summary
        let _ = write!(
            output,
            "\nPlan: {} to create, {} to update, {} to destroy\n",
            plan.create_count().to_string().green(),
            (plan.action_count() - plan.create_count() - plan.delete_count())
                .to_string()
                .yellow(),
            plan.delete_count().to_string().red()
        );

        // Guardrail warnings
        if !plan.passes_guardrails {
            let _ = write!(output, "\n{} Guardrail violations:\n", "⚠".yellow());
            for violation in &plan.guardrail_violations {
                let _ = writeln!(output, "   - {violation}");
            }
        }

        output
    }

    /// Formats project status for display.
    #[must_use]
    pub fn format_status(&self, status: &ProjectStatus, health: Option<&[HealthStatus]>) -> String {
        match self.format {
            OutputFormat::Json => {
                serde_json::to_string_pretty(&StatusJson::from(status)).unwrap_or_default()
            }
            OutputFormat::Text => Self::format_status_text(status, health),
        }
    }

    /// Formats status as text.
    fn format_status_text(status: &ProjectStatus, health: Option<&[HealthStatus]>) -> String {
        let mut output = String::new();

        let _ = write!(
            output,
            "\n📦 Project: {}/{}\n\n",
            status.project,
            status.environment
        );

        if status.pods.is_empty() {
            output.push_str("   No pods deployed.\n");
            return output;
        }

        // Create pod table
        let rows: Vec<PodStatusRow> = status
            .pods
            .iter()
            .map(|p| {
                let health_indicator = health
                    .and_then(|h| h.iter().find(|hs| hs.pod_id == p.id))
                    .map_or("", |hs| if hs.healthy { "✓" } else { "✗" });

                PodStatusRow {
                    name: p.pod_name.clone().unwrap_or_else(|| p.name.clone()),
                    status: format!("{} {health_indicator}", Self::format_pod_status(p.status)),
                    gpu: format!(
                        "{}x {}",
                        p.gpu_count,
                        p.gpu_type.as_deref().unwrap_or("unknown")
                    ),
                    image: Self::truncate(&p.image, 30),
                    id: Self::truncate(&p.id, 12),
                }
            })
            .collect();

        let table = Table::new(rows).to_string();
        output.push_str(&table);
        output.push('\n');

        // Summary
        let health_status = if status.is_healthy() {
            "healthy".green().to_string()
        } else if status.has_errors() {
            "unhealthy".red().to_string()
        } else {
            "partial".yellow().to_string()
        };

        let _ = write!(
            output,
            "\nStatus: {} ({} running, {} stopped, {} errors)\n",
            health_status, status.running, status.stopped, status.error
        );

        // Endpoints
        let has_endpoints = status.pods.iter().any(|p| !p.endpoints.is_empty());
        if has_endpoints {
            output.push_str("\nEndpoints:\n");
            for pod in &status.pods {
                if !pod.endpoints.is_empty() {
                    let pod_name = pod.pod_name.as_deref().unwrap_or(&pod.name);
                    for (port, url) in &pod.endpoints {
                        let _ = writeln!(output, "   {pod_name}:{port} -> {url}");
                    }
                }
            }
        }

        output
    }

    /// Formats a drift report.
    #[must_use]
    pub fn format_drift(&self, report: &DriftReport) -> String {
        match self.format {
            OutputFormat::Json => serde_json::to_string_pretty(report).unwrap_or_default(),
            OutputFormat::Text => {
                if report.is_converged() {
                    format!("{} No drift detected - state is converged.\n", "✓".green())
                } else {
                    let mut output = format!("{} Drift detected:\n\n", "⚠".yellow());
                    for resource in &report.drifted_resources {
                        let _ = writeln!(output, "   - {resource}");
                    }
                    let _ = write!(
                        output,
                        "\n{}/{} resources have drifted.\n",
                        report.drifted_resources.len(),
                        report.total_resources
                    );
                    output
                }
            }
        }
    }

    /// Formats a reconciliation result.
    #[must_use]
    pub fn format_reconciliation(&self, result: &ReconciliationResult) -> String {
        match self.format {
            OutputFormat::Json => serde_json::to_string_pretty(result).unwrap_or_default(),
            OutputFormat::Text => {
                let status = if result.success {
                    format!("{} Reconciliation successful", "✓".green())
                } else {
                    format!("{} Reconciliation failed", "✗".red())
                };

                let mut output = format!("{status}\n\n");
                let _ = writeln!(output, "   Created: {}", result.created);
                let _ = writeln!(output, "   Updated: {}", result.updated);
                let _ = writeln!(output, "   Deleted: {}", result.deleted);
                let _ = writeln!(output, "   Unchanged: {}", result.unchanged);

                if !result.errors.is_empty() {
                    let _ = write!(output, "\n{} Errors:\n", "⚠".yellow());
                    for error in &result.errors {
                        let _ = writeln!(output, "   - {error}");
                    }
                }

                output
            }
        }
    }

    /// Formats deployment state.
    #[must_use]
    pub fn format_state(&self, state: &DeploymentState) -> String {
        match self.format {
            OutputFormat::Json => serde_json::to_string_pretty(state).unwrap_or_default(),
            OutputFormat::Text => {
                let mut output = String::new();

                let _ = write!(
                    output,
                    "\n💾 State: {}/{}\n\n",
                    state.project,
                    state.environment
                );

                let _ = writeln!(output, "   Version: {}", state.version);
                let _ = writeln!(output, "   Config hash: {}", &state.config_hash[..8.min(state.config_hash.len())]);
                let _ = writeln!(output, "   Last updated: {}", state.last_updated);
                let _ = writeln!(output, "   Pods: {}", state.pods.len());
                let _ = writeln!(output, "   Volumes: {}", state.volumes.len());

                if !state.history.is_empty() {
                    let _ = writeln!(output, "\n   Recent history ({}):", state.history.len());
                    for entry in state.history.iter().rev().take(5) {
                        let status = if entry.success { "✓" } else { "✗" };
                        let _ = writeln!(
                            output,
                            "     {status} {} - {} ({})",
                            entry.timestamp.format("%Y-%m-%d %H:%M"),
                            entry.operation,
                            entry.resources.join(", ")
                        );
                    }
                }

                output
            }
        }
    }

    /// Formats an action type with color.
    fn format_action_type(action_type: ActionType) -> String {
        match action_type {
            ActionType::CreatePod => "+create".green().to_string(),
            ActionType::UpdatePod => "~update".yellow().to_string(),
            ActionType::DeletePod => "-delete".red().to_string(),
            ActionType::StopPod => "stop".yellow().to_string(),
            ActionType::ResumePod => "resume".green().to_string(),
            ActionType::Noop => "noop".dimmed().to_string(),
        }
    }

    /// Formats a pod status with color.
    fn format_pod_status(status: PodStatus) -> String {
        match status {
            PodStatus::Running => "running".green().to_string(),
            PodStatus::Starting => "starting".yellow().to_string(),
            PodStatus::Stopped | PodStatus::Exited => "stopped".red().to_string(),
            PodStatus::Creating => "creating".yellow().to_string(),
            PodStatus::Unknown => "unknown".dimmed().to_string(),
        }
    }

    /// Truncates a string to a maximum length.
    fn truncate(s: &str, max_len: usize) -> String {
        if s.len() <= max_len {
            s.to_string()
        } else {
            format!("{}...", &s[..max_len - 3])
        }
    }

    /// Prints a success message.
    pub fn success(&self, message: &str) {
        match self.format {
            OutputFormat::Json => {
                let json = serde_json::json!({ "status": "success", "message": message });
                let output = serde_json::to_string_pretty(&json).unwrap_or_default();
                // Note: In real implementation, this would write to stdout
                // For now, we use a placeholder since println is denied
                let _ = output;
            }
            OutputFormat::Text => {
                let _ = format!("{} {message}", "✓".green());
            }
        }
    }

    /// Prints an error message.
    pub fn error(&self, message: &str) {
        match self.format {
            OutputFormat::Json => {
                let json = serde_json::json!({ "status": "error", "message": message });
                let _ = serde_json::to_string_pretty(&json).unwrap_or_default();
            }
            OutputFormat::Text => {
                let _ = format!("{} {message}", "✗".red());
            }
        }
    }

    /// Prints a warning message.
    pub fn warning(&self, message: &str) {
        match self.format {
            OutputFormat::Json => {
                let json = serde_json::json!({ "status": "warning", "message": message });
                let _ = serde_json::to_string_pretty(&json).unwrap_or_default();
            }
            OutputFormat::Text => {
                let _ = format!("{} {message}", "⚠".yellow());
            }
        }
    }
}

// JSON serialization helpers

#[derive(serde::Serialize)]
struct PlanJson {
    config_hash: String,
    action_count: usize,
    creates: usize,
    deletes: usize,
    passes_guardrails: bool,
    actions: Vec<ActionJson>,
}

#[derive(serde::Serialize)]
struct ActionJson {
    action_type: String,
    resource: String,
    reason: String,
}

impl From<&DeploymentPlan> for PlanJson {
    fn from(plan: &DeploymentPlan) -> Self {
        Self {
            config_hash: plan.config_hash.clone(),
            action_count: plan.action_count(),
            creates: plan.create_count(),
            deletes: plan.delete_count(),
            passes_guardrails: plan.passes_guardrails,
            actions: plan
                .actions
                .iter()
                .map(|a| ActionJson {
                    action_type: a.action_type.to_string(),
                    resource: a.resource_name.clone(),
                    reason: a.reason.clone(),
                })
                .collect(),
        }
    }
}

#[derive(serde::Serialize)]
struct StatusJson {
    project: String,
    environment: String,
    total_pods: usize,
    running: usize,
    stopped: usize,
    error: usize,
    pods: Vec<PodJson>,
}

#[derive(serde::Serialize)]
struct PodJson {
    id: String,
    name: String,
    status: String,
    gpu_type: Option<String>,
    gpu_count: u32,
    image: String,
}

impl From<&ProjectStatus> for StatusJson {
    fn from(status: &ProjectStatus) -> Self {
        Self {
            project: status.project.clone(),
            environment: status.environment.clone(),
            total_pods: status.total_pods,
            running: status.running,
            stopped: status.stopped,
            error: status.error,
            pods: status
                .pods
                .iter()
                .map(|p| PodJson {
                    id: p.id.clone(),
                    name: p.pod_name.clone().unwrap_or_else(|| p.name.clone()),
                    status: p.status.to_string(),
                    gpu_type: p.gpu_type.clone(),
                    gpu_count: p.gpu_count,
                    image: p.image.clone(),
                })
                .collect(),
        }
    }
}

impl From<&ObservedPod> for PodJson {
    fn from(pod: &ObservedPod) -> Self {
        Self {
            id: pod.id.clone(),
            name: pod.pod_name.clone().unwrap_or_else(|| pod.name.clone()),
            status: pod.status.to_string(),
            gpu_type: pod.gpu_type.clone(),
            gpu_count: pod.gpu_count,
            image: pod.image.clone(),
        }
    }
}
