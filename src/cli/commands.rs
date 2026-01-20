//! CLI command definitions.
//!
//! This module defines all CLI commands and their arguments using clap.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// Halldyll - Declarative `RunPod` deployment manager.
#[derive(Parser, Debug)]
#[command(name = "halldyll")]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
pub struct Cli {
    /// Path to the configuration file.
    #[arg(short, long, global = true, env = "HALLDYLL_CONFIG")]
    pub config: Option<PathBuf>,

    /// Enable verbose output.
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Output format (text, json).
    #[arg(long, global = true, default_value = "text")]
    pub output: OutputFormat,

    /// Subcommand to execute.
    #[command(subcommand)]
    pub command: Commands,
}

/// Available CLI commands.
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Initialize a new Halldyll project.
    Init {
        /// Directory to initialize (defaults to current directory).
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Force overwrite existing files.
        #[arg(short, long)]
        force: bool,
    },

    /// Validate the deployment configuration.
    Validate {
        /// Show all warnings, not just errors.
        #[arg(short, long)]
        warnings: bool,
    },

    /// Generate and display the deployment plan.
    Plan {
        /// Show detailed diff information.
        #[arg(short, long)]
        detailed: bool,
    },

    /// Apply the deployment plan.
    Apply {
        /// Skip confirmation prompt.
        #[arg(short, long)]
        yes: bool,

        /// Continue on errors.
        #[arg(long)]
        continue_on_error: bool,
    },

    /// Show current deployment status.
    Status {
        /// Show detailed pod information.
        #[arg(short, long)]
        detailed: bool,

        /// Include health check results.
        #[arg(long)]
        health: bool,
    },

    /// Reconcile deployment to match configuration.
    Reconcile {
        /// Skip confirmation prompt.
        #[arg(short, long)]
        yes: bool,

        /// Maximum reconciliation attempts.
        #[arg(long, default_value = "3")]
        max_attempts: u32,
    },

    /// Destroy all deployed resources.
    Destroy {
        /// Skip confirmation prompt.
        #[arg(short, long)]
        yes: bool,

        /// Keep persistent volumes.
        #[arg(long)]
        keep_volumes: bool,
    },

    /// Show deployment logs.
    Logs {
        /// Pod name (optional, shows all pods if not specified).
        pod: Option<String>,

        /// Follow log output.
        #[arg(short, long)]
        follow: bool,

        /// Number of lines to show.
        #[arg(short, long, default_value = "100")]
        tail: u32,
    },

    /// Check for drift between config and actual state.
    Drift,

    /// Manage state backend.
    State {
        /// State subcommand.
        #[command(subcommand)]
        command: StateCommands,
    },
}

/// State management subcommands.
#[derive(Subcommand, Debug)]
pub enum StateCommands {
    /// Show current state.
    Show,

    /// Lock the state.
    Lock {
        /// Lock holder identifier.
        #[arg(long)]
        holder: Option<String>,
    },

    /// Unlock the state.
    Unlock {
        /// Lock ID to unlock.
        #[arg(long)]
        lock_id: Option<String>,

        /// Force unlock (dangerous).
        #[arg(long)]
        force: bool,
    },

    /// Pull state from remote backend.
    Pull,

    /// Push state to remote backend.
    Push {
        /// Force push even if locked.
        #[arg(long)]
        force: bool,
    },
}

/// Output format options.
#[derive(Debug, Clone, Copy, Default, clap::ValueEnum)]
pub enum OutputFormat {
    /// Human-readable text output.
    #[default]
    Text,
    /// JSON output for scripting.
    Json,
}

impl Cli {
    /// Parses CLI arguments from the command line.
    #[must_use]
    pub fn parse_args() -> Self {
        Self::parse()
    }
}
