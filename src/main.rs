//! Halldyll CLI entrypoint.
//!
//! This is the main entrypoint for the halldyll command-line tool.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use halldyll_deploy_pods::cli::{Cli, Commands, OutputFormatter, StateCommands};
use halldyll_deploy_pods::config::{
    find_config_file, ConfigHasher, ConfigParser, ConfigValidator, StateBackend,
};
use halldyll_deploy_pods::error::Result;
use halldyll_deploy_pods::planner::{DeploymentPlan, DiffEngine};
use halldyll_deploy_pods::reconciler::Reconciler;
use halldyll_deploy_pods::runpod::{HealthChecker, PodObserver, PodProvisioner, RunPodClient};
use halldyll_deploy_pods::state::{DeploymentState, LocalStateStore, S3StateStore, StateStore};

use clap::Parser;
use tracing::{debug, error, info};
use tracing_subscriber::EnvFilter;

/// Main entrypoint.
fn main() -> ExitCode {
    let cli = Cli::parse();

    // Initialize logging
    init_logging(cli.verbose);

    // Run async runtime
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("Failed to create async runtime: {e}");
            return ExitCode::FAILURE;
        }
    };

    match runtime.block_on(run(cli)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Initializes the logging system.
fn init_logging(verbose: bool) {
    let filter = if verbose {
        EnvFilter::new("debug")
    } else {
        EnvFilter::new("info")
    };

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}

/// Main async entry point.
async fn run(cli: Cli) -> Result<()> {
    let formatter = OutputFormatter::new(cli.output);

    match cli.command {
        Commands::Init { path, force } => cmd_init(&path, force),
        Commands::Validate { warnings } => cmd_validate(cli.config.as_ref(), warnings, &formatter),
        Commands::Plan { detailed } => cmd_plan(cli.config.as_ref(), detailed, &formatter).await,
        Commands::Apply { yes, continue_on_error } => {
            cmd_apply(cli.config.as_ref(), yes, continue_on_error, &formatter).await
        }
        Commands::Status { detailed, health } => {
            cmd_status(cli.config.as_ref(), detailed, health, &formatter).await
        }
        Commands::Reconcile { yes, max_attempts } => {
            cmd_reconcile(cli.config.as_ref(), yes, max_attempts, &formatter).await
        }
        Commands::Destroy { yes, keep_volumes } => {
            cmd_destroy(cli.config.as_ref(), yes, keep_volumes, &formatter).await
        }
        Commands::Logs { pod, follow, tail } => cmd_logs(cli.config.as_ref(), pod, follow, tail),
        Commands::Drift => cmd_drift(cli.config.as_ref(), &formatter).await,
        Commands::State { command } => cmd_state(cli.config.as_ref(), command, &formatter).await,
    }
}

/// Initialize a new project.
fn cmd_init(path: &PathBuf, force: bool) -> Result<()> {
    info!("Initializing new Halldyll project in: {}", path.display());

    let config_path = path.join("halldyll.deploy.yaml");
    let env_path = path.join(".env.example");
    let gitignore_path = path.join(".gitignore");

    // Check if files exist
    if !force && config_path.exists() {
        eprintln!("Configuration file already exists: {}", config_path.display());
        eprintln!("Use --force to overwrite.");
        return Ok(());
    }

    // Create directory if needed
    if !path.exists() {
        std::fs::create_dir_all(path)?;
    }

    // Write config template
    let config_template = include_str!("../templates/halldyll.deploy.yaml");
    std::fs::write(&config_path, config_template)?;
    eprintln!("Created: {}", config_path.display());

    // Write .env.example
    let env_template = include_str!("../templates/.env.example");
    std::fs::write(&env_path, env_template)?;
    eprintln!("Created: {}", env_path.display());

    // Write/update .gitignore
    let gitignore_content = ".env\n.halldyll/\n";
    if gitignore_path.exists() {
        let existing = std::fs::read_to_string(&gitignore_path)?;
        if !existing.contains(".env") || !existing.contains(".halldyll") {
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(&gitignore_path)?;
            writeln!(file, "\n# Halldyll")?;
            if !existing.contains(".env") {
                writeln!(file, ".env")?;
            }
            if !existing.contains(".halldyll") {
                writeln!(file, ".halldyll/")?;
            }
            eprintln!("Updated: {}", gitignore_path.display());
        }
    } else {
        std::fs::write(&gitignore_path, gitignore_content)?;
        eprintln!("Created: {}", gitignore_path.display());
    }

    eprintln!("\nProject initialized successfully!");
    eprintln!("Next steps:");
    eprintln!("  1. Copy .env.example to .env and fill in your API keys");
    eprintln!("  2. Edit halldyll.deploy.yaml with your pod configuration");
    eprintln!("  3. Run 'halldyll validate' to check your configuration");
    eprintln!("  4. Run 'halldyll plan' to see what will be deployed");
    eprintln!("  5. Run 'halldyll apply' to deploy your pods");

    Ok(())
}

/// Validate configuration.
fn cmd_validate(
    config_path: Option<&PathBuf>,
    show_warnings: bool,
    formatter: &OutputFormatter,
) -> Result<()> {
    let config_file = resolve_config_path(config_path)?;
    info!("Validating configuration: {}", config_file.display());

    // Load .env
    let parser = ConfigParser::new().with_base_path(
        config_file
            .parent()
            .unwrap_or_else(|| std::path::Path::new(".")),
    );
    parser.load_dotenv()?;

    // Parse config
    let config = parser.load_file(&config_file)?;

    // Validate
    let validator = ConfigValidator::new();
    let result = validator.validate(&config)?;

    if result.is_valid() {
        eprintln!("Configuration is valid!");
        if show_warnings && !result.warnings.is_empty() {
            eprintln!("\nWarnings:");
            for warning in &result.warnings {
                eprintln!("  - {warning}");
            }
        }
    }

    // Show summary
    eprintln!("\nConfiguration summary:");
    eprintln!("  Project: {}", config.project.name);
    eprintln!("  Environment: {}", config.project.environment);
    eprintln!("  Pods: {}", config.pods.len());
    eprintln!("  Total GPUs: {}", config.total_gpus());

    let _ = formatter;
    Ok(())
}

/// Show deployment plan.
async fn cmd_plan(
    config_path: Option<&PathBuf>,
    detailed: bool,
    formatter: &OutputFormatter,
) -> Result<()> {
    let (config, state_store) = load_config_and_state(config_path).await?;
    let client = create_runpod_client()?;
    let observer = PodObserver::new(client);

    // Load state
    let state = state_store.load().await?;

    // Get observed pods
    let observed_pods = observer
        .list_project_pods(&config.project.name, &config.project.environment)
        .await?;

    // Compute diff
    let hasher = ConfigHasher::new();
    let config_hash = hasher.hash_config(&config);
    let diff_engine = DiffEngine::new();
    let diff = diff_engine.compute_diff(&config, state.as_ref(), &observed_pods);

    // Generate plan
    let plan = DeploymentPlan::from_diff(&diff, &config, &config_hash);

    // Output
    let output = formatter.format_plan(&plan);
    eprintln!("{output}");

    if detailed {
        eprintln!("\nDetailed changes:");
        for action in &plan.actions {
            eprintln!("  {} {} - {}", action.action_type, action.resource_name, action.reason);
        }
    }

    Ok(())
}

/// Apply deployment plan.
async fn cmd_apply(
    config_path: Option<&PathBuf>,
    auto_approve: bool,
    continue_on_error: bool,
    formatter: &OutputFormatter,
) -> Result<()> {
    let (config, state_store) = load_config_and_state(config_path).await?;
    let client = create_runpod_client()?;
    let observer = PodObserver::new(client.clone());
    let mut provisioner = PodProvisioner::new(client);

    // Initialize GPU types
    provisioner.init_gpu_types().await?;

    // Load state
    let mut state = state_store
        .load()
        .await?
        .unwrap_or_else(|| DeploymentState::new(&config.project.name, &config.project.environment));

    // Get observed pods
    let observed_pods = observer
        .list_project_pods(&config.project.name, &config.project.environment)
        .await?;

    // Compute diff and plan
    let hasher = ConfigHasher::new();
    let config_hash = hasher.hash_config(&config);
    let diff_engine = DiffEngine::new();
    let diff = diff_engine.compute_diff(&config, Some(&state), &observed_pods);
    let plan = DeploymentPlan::from_diff(&diff, &config, &config_hash);

    if plan.is_empty() {
        eprintln!("No changes to apply.");
        return Ok(());
    }

    // Show plan
    let output = formatter.format_plan(&plan);
    eprintln!("{output}");

    // Confirm
    if !auto_approve {
        eprint!("Do you want to apply this plan? [y/N]: ");
        std::io::stderr().flush()?;

        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;

        if !input.trim().eq_ignore_ascii_case("y") {
            eprintln!("Apply cancelled.");
            return Ok(());
        }
    }

    // Execute plan
    let executor = halldyll_deploy_pods::planner::PlanExecutor::new(&provisioner, &config.project)
        .with_continue_on_error(continue_on_error);

    let result = executor.execute(&plan, &mut state).await?;

    // Save state
    state_store.save(&state).await?;

    // Show result
    eprintln!("\n{result}");

    Ok(())
}

/// Show deployment status.
async fn cmd_status(
    config_path: Option<&PathBuf>,
    _detailed: bool,
    include_health: bool,
    formatter: &OutputFormatter,
) -> Result<()> {
    let (config, _state_store) = load_config_and_state(config_path).await?;
    let client = create_runpod_client()?;
    let observer = PodObserver::new(client);

    // Get project status
    let status = observer
        .get_project_status(&config.project.name, &config.project.environment)
        .await?;

    // Optionally check health
    let health = if include_health && !status.pods.is_empty() {
        let checker = HealthChecker::new()?;
        Some(checker.check_pods(&status.pods).await)
    } else {
        None
    };

    // Output
    let output = formatter.format_status(&status, health.as_deref());
    eprintln!("{output}");

    Ok(())
}

/// Reconcile deployment.
async fn cmd_reconcile(
    config_path: Option<&PathBuf>,
    auto_approve: bool,
    max_attempts: u32,
    formatter: &OutputFormatter,
) -> Result<()> {
    let (config, state_store) = load_config_and_state(config_path).await?;
    let client = create_runpod_client()?;
    let observer = PodObserver::new(client.clone());
    let mut provisioner = PodProvisioner::new(client);

    // Initialize GPU types
    provisioner.init_gpu_types().await?;

    // Confirm
    if !auto_approve {
        eprint!("This will reconcile your deployment to match the configuration. Continue? [y/N]: ");
        std::io::stderr().flush()?;

        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;

        if !input.trim().eq_ignore_ascii_case("y") {
            eprintln!("Reconciliation cancelled.");
            return Ok(());
        }
    }

    // Create reconciler
    let reconciler =
        Reconciler::new(&config, &state_store, &provisioner, &observer).with_max_attempts(max_attempts);

    // Run reconciliation
    let result = reconciler.reconcile().await?;

    // Output
    let output = formatter.format_reconciliation(&result);
    eprintln!("{output}");

    Ok(())
}

/// Destroy deployment.
async fn cmd_destroy(
    config_path: Option<&PathBuf>,
    auto_approve: bool,
    _keep_volumes: bool,
    _formatter: &OutputFormatter,
) -> Result<()> {
    let (config, state_store) = load_config_and_state(config_path).await?;
    let client = create_runpod_client()?;
    let observer = PodObserver::new(client.clone());
    let provisioner = PodProvisioner::new(client);

    // Get current pods
    let pods = observer
        .list_project_pods(&config.project.name, &config.project.environment)
        .await?;

    if pods.is_empty() {
        eprintln!("No pods to destroy.");
        return Ok(());
    }

    eprintln!("The following pods will be destroyed:");
    for pod in &pods {
        let name = pod.pod_name.as_deref().unwrap_or(&pod.name);
        eprintln!("  - {name} ({})", pod.id);
    }

    // Confirm
    if !auto_approve {
        eprint!("\nThis action is IRREVERSIBLE. Type 'destroy' to confirm: ");
        std::io::stderr().flush()?;

        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;

        if input.trim() != "destroy" {
            eprintln!("Destruction cancelled.");
            return Ok(());
        }
    }

    // Destroy pods
    for pod in &pods {
        let name = pod.pod_name.as_deref().unwrap_or(&pod.name);
        eprintln!("Destroying {name}...");
        if let Err(e) = provisioner.terminate_pod(&pod.id).await {
            error!("Failed to destroy {name}: {e}");
        }
    }

    // Clear state
    state_store.delete().await?;

    eprintln!("\nAll pods destroyed.");
    Ok(())
}

/// Show logs (placeholder).
///
/// # Errors
///
/// Returns an error if the pod is not found (once implemented).
fn cmd_logs(
    _config_path: Option<&PathBuf>,
    _pod: Option<String>,
    _follow: bool,
    _tail: u32,
) -> Result<()> {
    Err(halldyll_deploy_pods::error::HalldyllError::internal(
        "Log viewing is not yet implemented. View logs directly in the RunPod dashboard.",
    ))
}

/// Check for drift.
async fn cmd_drift(config_path: Option<&PathBuf>, formatter: &OutputFormatter) -> Result<()> {
    let (config, state_store) = load_config_and_state(config_path).await?;
    let client = create_runpod_client()?;
    let observer = PodObserver::new(client.clone());
    let provisioner = PodProvisioner::new(client);

    let reconciler = Reconciler::new(&config, &state_store, &provisioner, &observer);
    let report = reconciler.check_drift().await?;

    let output = formatter.format_drift(&report);
    eprintln!("{output}");

    Ok(())
}

/// State management commands.
async fn cmd_state(
    config_path: Option<&PathBuf>,
    command: StateCommands,
    formatter: &OutputFormatter,
) -> Result<()> {
    let (_config, state_store) = load_config_and_state(config_path).await?;

    match command {
        StateCommands::Show => {
            if let Some(state) = state_store.load().await? {
                let output = formatter.format_state(&state);
                eprintln!("{output}");
            } else {
                eprintln!("No state found.");
            }
        }
        StateCommands::Lock { holder } => {
            let holder_str = holder.as_deref().unwrap_or("");
            let lock = state_store.acquire_lock(holder_str).await?;
            eprintln!("State locked: {}", lock.lock_id);
        }
        StateCommands::Unlock { lock_id, force } => {
            if force {
                // Force unlock by deleting lock file
                if let Some(lock_info) = state_store.get_lock_info().await? {
                    state_store.release_lock(&lock_info.lock_id).await?;
                    eprintln!("State forcefully unlocked.");
                }
            } else if let Some(id) = lock_id {
                state_store.release_lock(&id).await?;
                eprintln!("State unlocked.");
            } else {
                eprintln!("Please provide --lock-id or use --force");
            }
        }
        StateCommands::Pull => {
            eprintln!("State pull is only applicable for remote backends.");
        }
        StateCommands::Push { force: _ } => {
            eprintln!("State push is only applicable for remote backends.");
        }
    }

    Ok(())
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Resolves the configuration file path.
fn resolve_config_path(config_path: Option<&PathBuf>) -> Result<PathBuf> {
    config_path.map_or_else(|| find_config_file("."), |path| Ok(path.clone()))
}

/// Loads configuration and creates appropriate state store.
async fn load_config_and_state(
    config_path: Option<&PathBuf>,
) -> Result<(halldyll_deploy_pods::config::DeployConfig, Box<dyn StateStore>)> {
    let config_file = resolve_config_path(config_path)?;
    debug!("Loading configuration from: {}", config_file.display());

    let parser = ConfigParser::new().with_base_path(
        config_file
            .parent()
            .unwrap_or_else(|| std::path::Path::new(".")),
    );
    parser.load_dotenv()?;

    let config = parser.load_with_env(&config_file)?;

    // Validate
    let validator = ConfigValidator::new();
    validator.validate(&config)?;

    // Create state store based on config
    let state_store: Box<dyn StateStore> = match config.state.backend {
        StateBackend::Local => {
            let path = config.state.path.as_ref().map_or_else(
                || {
                    config_file
                        .parent()
                        .unwrap_or_else(|| std::path::Path::new("."))
                        .join(".halldyll")
                },
                PathBuf::from,
            );
            Box::new(LocalStateStore::with_base_dir(path))
        }
        StateBackend::S3 => {
            let bucket = config
                .state
                .bucket
                .as_deref()
                .ok_or_else(|| halldyll_deploy_pods::error::HalldyllError::internal("S3 bucket not configured"))?;
            let prefix = config.state.prefix.as_deref();
            let region = config.state.region.as_deref();
            Box::new(S3StateStore::new(bucket, prefix, region).await?)
        }
    };

    Ok((config, state_store))
}

/// Creates a `RunPod` API client.
fn create_runpod_client() -> Result<RunPodClient> {
    let api_key = ConfigParser::get_runpod_api_key()?;
    RunPodClient::new(&api_key)
}
