//! Planning module for deployment operations.
//!
//! This module handles the comparison between desired and observed states,
//! generating execution plans for applying changes.

mod diff;
mod plan;
mod executor;

pub use diff::{DiffEngine, ResourceDiff, DiffType};
pub use plan::{DeploymentPlan, PlannedAction, ActionType};
pub use executor::PlanExecutor;
