// ============================================================================
// Strict linting - Dangerous or non-idiomatic practices are forbidden
// ============================================================================

#![deny(warnings)]                    // All warnings are treated as errors
#![deny(unsafe_code)]                 // Unsafe code is forbidden
#![deny(missing_docs)]                // All public items must be documented
#![deny(dead_code)]                   // Unused code is forbidden
#![deny(non_camel_case_types)]        // Types must follow CamelCase convention

// Additional strictness - Leave nothing unchecked
#![deny(unused_imports)]              // Unused imports are forbidden
#![deny(unused_variables)]            // Unused variables are forbidden
#![deny(unused_must_use)]             // Must handle Result and Option explicitly
#![deny(non_snake_case)]              // Variables and functions must be snake_case
#![deny(non_upper_case_globals)]      // Constants must be UPPER_CASE
#![deny(nonstandard_style)]           // Non-standard code style is forbidden
#![forbid(unsafe_op_in_unsafe_fn)]    // Unsafe ops in unsafe fns are forbidden

// Clippy lints (warnings only)
#![warn(clippy::all)]                 // All standard Clippy lints
#![warn(clippy::pedantic)]            // Very strict Clippy lints
#![warn(clippy::nursery)]             // Experimental lints
#![warn(clippy::unwrap_used)]         // unwrap() warning
#![warn(clippy::expect_used)]         // expect() warning
#![warn(clippy::panic)]               // panic!() warning
#![warn(clippy::print_stdout)]        // println!() warning
#![warn(clippy::todo)]                // TODO warning
#![warn(clippy::unimplemented)]       // unimplemented!() warning
#![warn(clippy::missing_const_for_fn)] // Force const when possible
#![warn(clippy::unwrap_in_result)]    // unwrap() in Result warning
#![warn(clippy::module_inception)]    // Module with same name as crate warning
#![warn(clippy::redundant_clone)]     // Useless clones warning
#![warn(clippy::shadow_unrelated)]    // Shadowing unrelated variables warning
#![warn(clippy::too_many_arguments)]  // Limit function arguments
#![warn(clippy::cognitive_complexity)] // Limit cognitive complexity

// Safety and robustness lints
#![deny(overflowing_literals)]        // Overflowing literals are forbidden
#![deny(arithmetic_overflow)]         // Arithmetic overflow is forbidden

// ============================================================================
// Crate Documentation
// ============================================================================

//! # Halldyll Deploy Pods
//!
//! A declarative, idempotent, and reconcilable deployment system for `RunPod` GPU pods.
//!
//! ## Overview
//!
//! Halldyll provides a Kubernetes-like deployment experience for `RunPod`, allowing you to:
//!
//! - Define your infrastructure as code in a YAML configuration file
//! - Deploy and manage multi-pod GPU workloads
//! - Automatically reconcile drift between desired and actual state
//! - Track deployment history and state
//!
//! ## Architecture
//!
//! The system is built around the concept of **desired state reconciliation**:
//!
//! 1. **Desired State**: Defined in `halldyll.deploy.yaml`
//! 2. **Observed State**: Queried from `RunPod` API
//! 3. **Reconciler**: Compares states and executes necessary actions
//!
//! ## Modules
//!
//! - [`config`]: Configuration parsing and validation
//! - [`state`]: State storage backends (local, S3)
//! - [`runpod`]: `RunPod` API client and provisioning
//! - [`planner`]: Diff computation and execution planning
//! - [`reconciler`]: State reconciliation engine
//! - [`cli`]: Command-line interface
//!
//! ## Example
//!
//! ```yaml
//! project:
//!   name: my-ml-stack
//!   environment: prod
//!
//! pods:
//!   - name: inference
//!     gpu:
//!       type: "NVIDIA A40"
//!       count: 1
//!     runtime:
//!       image: ghcr.io/my-org/inference:latest
//!     ports:
//!       - "8000/http"
//! ```

// ============================================================================
// Modules
// ============================================================================

pub mod cli;
pub mod config;
pub mod error;
pub mod planner;
pub mod reconciler;
pub mod runpod;
pub mod state;

// ============================================================================
// Re-exports
// ============================================================================

pub use cli::{Cli, Commands, OutputFormatter};
pub use config::{ConfigHasher, ConfigParser, ConfigValidator, DeployConfig};
pub use error::{HalldyllError, Result};
pub use planner::{DeploymentPlan, DiffEngine, PlanExecutor};
pub use reconciler::{DriftReport, ReconciliationResult, Reconciler};
pub use runpod::{
    HealthChecker, PodObserver, PodProvisioner, RunPodClient,
    PodExecutor, CommandResult, ModelSetupResult, EngineStartResult, PostProvisionResult,
};
pub use state::{DeploymentState, LocalStateStore, S3StateStore, StateStore};
