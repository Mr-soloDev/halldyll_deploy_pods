//! Pod command executor for post-provisioning tasks.
//!
//! This module handles executing commands on running pods via the RunPod API,
//! including model downloads and inference engine startup.

use std::time::Duration;
use tracing::{debug, error, info, warn};

use crate::config::{LoadConfig, ModelConfig, ModelProvider, PodConfig};
use crate::error::{HalldyllError, Result, RunPodError};

use super::client::RunPodClient;
use super::types::PodStatus;

/// Default timeout for command execution in seconds.
const DEFAULT_COMMAND_TIMEOUT_SECS: u64 = 600;

/// Default timeout for model download in seconds.
const MODEL_DOWNLOAD_TIMEOUT_SECS: u64 = 1800;

/// Polling interval for command status checks.
const POLL_INTERVAL_SECS: u64 = 5;

/// Pod command executor for post-provisioning tasks.
#[derive(Debug)]
pub struct PodExecutor {
    /// `RunPod` API client.
    client: RunPodClient,
}

/// Result of a command execution.
#[derive(Debug, Clone)]
pub struct CommandResult {
    /// Whether the command succeeded.
    pub success: bool,
    /// Command output (stdout).
    pub stdout: String,
    /// Command error output (stderr).
    pub stderr: String,
    /// Exit code if available.
    pub exit_code: Option<i32>,
}

/// Model setup result.
#[derive(Debug, Clone)]
pub struct ModelSetupResult {
    /// Model ID.
    pub model_id: String,
    /// Whether setup succeeded.
    pub success: bool,
    /// Model path on the pod.
    pub model_path: Option<String>,
    /// Error message if failed.
    pub error: Option<String>,
}

/// Engine startup result.
#[derive(Debug, Clone)]
pub struct EngineStartResult {
    /// Engine name.
    pub engine: String,
    /// Whether startup succeeded.
    pub success: bool,
    /// Service endpoint if available.
    pub endpoint: Option<String>,
    /// Error message if failed.
    pub error: Option<String>,
}

impl PodExecutor {
    /// Creates a new pod executor.
    #[must_use]
    pub const fn new(client: RunPodClient) -> Self {
        Self { client }
    }

    /// Executes a command on a running pod.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot be executed.
    pub async fn execute_command(
        &self,
        pod_id: &str,
        command: &str,
        timeout_secs: Option<u64>,
    ) -> Result<CommandResult> {
        let timeout = timeout_secs.unwrap_or(DEFAULT_COMMAND_TIMEOUT_SECS);
        
        debug!("Executing command on pod {}: {}", pod_id, command);

        // Use RunPod's exec endpoint
        let result = self.client.exec_command(pod_id, command, timeout).await?;

        Ok(result)
    }

    /// Waits for a pod to be ready for command execution.
    ///
    /// # Errors
    ///
    /// Returns an error if the pod doesn't become ready within the timeout.
    pub async fn wait_for_ready(&self, pod_id: &str, timeout_secs: u64) -> Result<()> {
        let start = std::time::Instant::now();
        let timeout = Duration::from_secs(timeout_secs);

        info!("Waiting for pod {} to be ready for commands", pod_id);

        loop {
            let pod = self.client.get_pod(pod_id).await?;

            // Check if pod is running
            if pod.desired_status == PodStatus::Running && pod.runtime.is_some() {
                // Try a simple command to verify SSH/exec is working
                match self.execute_command(pod_id, "echo ready", Some(30)).await {
                    Ok(result) if result.success => {
                        info!("Pod {} is ready for commands", pod_id);
                        return Ok(());
                    }
                    Ok(_) => {
                        debug!("Pod {} not ready yet, retrying...", pod_id);
                    }
                    Err(e) => {
                        debug!("Pod {} exec not ready: {}", pod_id, e);
                    }
                }
            }

            if start.elapsed() > timeout {
                return Err(HalldyllError::RunPod(RunPodError::Timeout {
                    pod_id: pod_id.to_string(),
                    expected_state: "ready for commands".to_string(),
                }));
            }

            tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_SECS)).await;
        }
    }

    /// Downloads and sets up models on a pod.
    ///
    /// # Errors
    ///
    /// Returns an error if model setup fails.
    pub async fn setup_models(
        &self,
        pod_id: &str,
        models: &[ModelConfig],
    ) -> Result<Vec<ModelSetupResult>> {
        if models.is_empty() {
            return Ok(Vec::new());
        }

        info!("Setting up {} model(s) on pod {}", models.len(), pod_id);

        let mut results = Vec::with_capacity(models.len());

        for model in models {
            let result = self.setup_single_model(pod_id, model).await;
            results.push(result);
        }

        // Check if any critical models failed
        let failed: Vec<_> = results.iter().filter(|r| !r.success).collect();
        if !failed.is_empty() {
            warn!(
                "{} model(s) failed to setup on pod {}",
                failed.len(),
                pod_id
            );
        }

        Ok(results)
    }

    /// Sets up a single model on a pod.
    async fn setup_single_model(&self, pod_id: &str, model: &ModelConfig) -> ModelSetupResult {
        info!("Setting up model '{}' on pod {}", model.id, pod_id);

        match model.provider {
            ModelProvider::Huggingface => {
                self.setup_huggingface_model(pod_id, model).await
            }
            ModelProvider::Bundle => {
                self.setup_bundle_model(pod_id, model).await
            }
            ModelProvider::Custom => {
                // Custom models are expected to be already available
                ModelSetupResult {
                    model_id: model.id.clone(),
                    success: true,
                    model_path: None,
                    error: None,
                }
            }
        }
    }

    /// Downloads a HuggingFace model.
    async fn setup_huggingface_model(
        &self,
        pod_id: &str,
        model: &ModelConfig,
    ) -> ModelSetupResult {
        let repo = match &model.repo {
            Some(r) => r,
            None => {
                return ModelSetupResult {
                    model_id: model.id.clone(),
                    success: false,
                    model_path: None,
                    error: Some("Missing 'repo' field for HuggingFace model".to_string()),
                };
            }
        };

        let model_path = format!("/root/.cache/huggingface/hub/models--{}", repo.replace('/', "--"));

        // Check if model already exists
        let check_cmd = format!("test -d '{}' && echo 'exists' || echo 'missing'", model_path);
        match self.execute_command(pod_id, &check_cmd, Some(30)).await {
            Ok(result) if result.stdout.trim() == "exists" => {
                info!("Model '{}' already exists on pod {}", model.id, pod_id);
                return ModelSetupResult {
                    model_id: model.id.clone(),
                    success: true,
                    model_path: Some(model_path),
                    error: None,
                };
            }
            _ => {}
        }

        // Download the model using huggingface-cli
        info!("Downloading model '{}' ({}) on pod {}", model.id, repo, pod_id);

        let download_cmd = format!(
            "huggingface-cli download {} --local-dir /models/{} 2>&1 || \
             python -c \"from huggingface_hub import snapshot_download; snapshot_download('{}')\" 2>&1",
            repo, model.id, repo
        );

        match self.execute_command(pod_id, &download_cmd, Some(MODEL_DOWNLOAD_TIMEOUT_SECS)).await {
            Ok(result) if result.success => {
                info!("Successfully downloaded model '{}' on pod {}", model.id, pod_id);
                ModelSetupResult {
                    model_id: model.id.clone(),
                    success: true,
                    model_path: Some(format!("/models/{}", model.id)),
                    error: None,
                }
            }
            Ok(result) => {
                error!("Failed to download model '{}': {}", model.id, result.stderr);
                ModelSetupResult {
                    model_id: model.id.clone(),
                    success: false,
                    model_path: None,
                    error: Some(result.stderr),
                }
            }
            Err(e) => {
                error!("Error downloading model '{}': {}", model.id, e);
                ModelSetupResult {
                    model_id: model.id.clone(),
                    success: false,
                    model_path: None,
                    error: Some(e.to_string()),
                }
            }
        }
    }

    /// Sets up a bundle of models/components.
    async fn setup_bundle_model(&self, pod_id: &str, model: &ModelConfig) -> ModelSetupResult {
        let components = match &model.components {
            Some(c) if !c.is_empty() => c,
            _ => {
                return ModelSetupResult {
                    model_id: model.id.clone(),
                    success: false,
                    model_path: None,
                    error: Some("Missing 'components' field for bundle model".to_string()),
                };
            }
        };

        info!("Setting up bundle '{}' with {} components", model.id, components.len());

        // Download each component
        for component in components {
            let cmd = format!(
                "huggingface-cli download {} 2>&1 || echo 'Failed to download {}'",
                component, component
            );
            
            if let Err(e) = self.execute_command(pod_id, &cmd, Some(MODEL_DOWNLOAD_TIMEOUT_SECS)).await {
                warn!("Failed to download component '{}': {}", component, e);
            }
        }

        ModelSetupResult {
            model_id: model.id.clone(),
            success: true,
            model_path: Some("/root/.cache/huggingface".to_string()),
            error: None,
        }
    }

    /// Starts an inference engine on the pod.
    ///
    /// # Errors
    ///
    /// Returns an error if the engine cannot be started.
    pub async fn start_inference_engine(
        &self,
        pod_id: &str,
        model: &ModelConfig,
        port: u16,
    ) -> Result<EngineStartResult> {
        let load_config = match &model.load {
            Some(c) => c,
            None => {
                return Ok(EngineStartResult {
                    engine: "none".to_string(),
                    success: true,
                    endpoint: None,
                    error: None,
                });
            }
        };

        let engine = load_config.engine.to_lowercase();
        info!("Starting {} engine for model '{}' on pod {}", engine, model.id, pod_id);

        match engine.as_str() {
            "vllm" => self.start_vllm(pod_id, model, load_config, port).await,
            "tgi" | "text-generation-inference" => {
                self.start_tgi(pod_id, model, load_config, port).await
            }
            "ollama" => self.start_ollama(pod_id, model, port).await,
            "transformers" => {
                // No server to start, just verify the model is loadable
                Ok(EngineStartResult {
                    engine: engine.clone(),
                    success: true,
                    endpoint: None,
                    error: None,
                })
            }
            other => {
                warn!("Unknown engine '{}', skipping auto-start", other);
                Ok(EngineStartResult {
                    engine: other.to_string(),
                    success: true,
                    endpoint: None,
                    error: Some(format!("Unknown engine '{}', manual start required", other)),
                })
            }
        }
    }

    /// Starts vLLM server.
    async fn start_vllm(
        &self,
        pod_id: &str,
        model: &ModelConfig,
        load_config: &LoadConfig,
        port: u16,
    ) -> Result<EngineStartResult> {
        let repo = model.repo.as_deref().unwrap_or(&model.id);
        
        let mut cmd_parts = vec![
            "nohup python -m vllm.entrypoints.openai.api_server".to_string(),
            format!("--model {}", repo),
            format!("--port {}", port),
            "--host 0.0.0.0".to_string(),
        ];

        // Add quantization if specified
        if let Some(quant) = &load_config.quant {
            let quant_arg = match quant.to_lowercase().as_str() {
                "awq" => "--quantization awq",
                "gptq" => "--quantization gptq",
                "squeezellm" => "--quantization squeezellm",
                "fp8" => "--quantization fp8",
                _ => "",
            };
            if !quant_arg.is_empty() {
                cmd_parts.push(quant_arg.to_string());
            }
        }

        // Add max sequence length
        if let Some(max_len) = load_config.max_seq_len {
            cmd_parts.push(format!("--max-model-len {}", max_len));
        }

        // Add any extra options
        for (key, value) in &load_config.options {
            if let Some(v) = value.as_str() {
                cmd_parts.push(format!("--{} {}", key, v));
            } else if let Some(v) = value.as_bool() {
                if v {
                    cmd_parts.push(format!("--{}", key));
                }
            } else if let Some(v) = value.as_i64() {
                cmd_parts.push(format!("--{} {}", key, v));
            }
        }

        cmd_parts.push("> /var/log/vllm.log 2>&1 &".to_string());

        let cmd = cmd_parts.join(" ");
        info!("Starting vLLM: {}", cmd);

        match self.execute_command(pod_id, &cmd, Some(60)).await {
            Ok(_) => {
                // Wait a bit for the server to start
                tokio::time::sleep(Duration::from_secs(10)).await;

                // Verify it's running
                let check_cmd = "pgrep -f 'vllm.entrypoints' || echo 'not running'";
                match self.execute_command(pod_id, check_cmd, Some(30)).await {
                    Ok(result) if !result.stdout.contains("not running") => {
                        info!("vLLM started successfully on pod {}", pod_id);
                        Ok(EngineStartResult {
                            engine: "vllm".to_string(),
                            success: true,
                            endpoint: Some(format!("http://localhost:{}", port)),
                            error: None,
                        })
                    }
                    _ => {
                        // Check logs for errors
                        let log_cmd = "tail -50 /var/log/vllm.log 2>/dev/null || echo 'No logs'";
                        let logs = self.execute_command(pod_id, log_cmd, Some(30)).await
                            .map(|r| r.stdout)
                            .unwrap_or_default();
                        
                        Ok(EngineStartResult {
                            engine: "vllm".to_string(),
                            success: false,
                            endpoint: None,
                            error: Some(format!("vLLM failed to start. Logs: {}", logs)),
                        })
                    }
                }
            }
            Err(e) => Ok(EngineStartResult {
                engine: "vllm".to_string(),
                success: false,
                endpoint: None,
                error: Some(e.to_string()),
            }),
        }
    }

    /// Starts Text Generation Inference server.
    async fn start_tgi(
        &self,
        pod_id: &str,
        model: &ModelConfig,
        load_config: &LoadConfig,
        port: u16,
    ) -> Result<EngineStartResult> {
        let repo = model.repo.as_deref().unwrap_or(&model.id);

        let mut cmd_parts = vec![
            "nohup text-generation-launcher".to_string(),
            format!("--model-id {}", repo),
            format!("--port {}", port),
            "--hostname 0.0.0.0".to_string(),
        ];

        // Add quantization
        if let Some(quant) = &load_config.quant {
            cmd_parts.push(format!("--quantize {}", quant));
        }

        // Add max sequence length
        if let Some(max_len) = load_config.max_seq_len {
            cmd_parts.push(format!("--max-input-length {}", max_len));
        }

        cmd_parts.push("> /var/log/tgi.log 2>&1 &".to_string());

        let cmd = cmd_parts.join(" ");
        info!("Starting TGI: {}", cmd);

        match self.execute_command(pod_id, &cmd, Some(60)).await {
            Ok(_) => {
                tokio::time::sleep(Duration::from_secs(10)).await;

                let check_cmd = "pgrep -f 'text-generation-launcher' || echo 'not running'";
                match self.execute_command(pod_id, check_cmd, Some(30)).await {
                    Ok(result) if !result.stdout.contains("not running") => {
                        Ok(EngineStartResult {
                            engine: "tgi".to_string(),
                            success: true,
                            endpoint: Some(format!("http://localhost:{}", port)),
                            error: None,
                        })
                    }
                    _ => Ok(EngineStartResult {
                        engine: "tgi".to_string(),
                        success: false,
                        endpoint: None,
                        error: Some("TGI failed to start".to_string()),
                    }),
                }
            }
            Err(e) => Ok(EngineStartResult {
                engine: "tgi".to_string(),
                success: false,
                endpoint: None,
                error: Some(e.to_string()),
            }),
        }
    }

    /// Starts Ollama server.
    async fn start_ollama(
        &self,
        pod_id: &str,
        model: &ModelConfig,
        port: u16,
    ) -> Result<EngineStartResult> {
        // Start Ollama server
        let start_cmd = format!(
            "nohup ollama serve > /var/log/ollama.log 2>&1 & sleep 5 && ollama pull {}",
            model.id
        );

        match self.execute_command(pod_id, &start_cmd, Some(300)).await {
            Ok(result) if result.success => {
                Ok(EngineStartResult {
                    engine: "ollama".to_string(),
                    success: true,
                    endpoint: Some(format!("http://localhost:{}", port)),
                    error: None,
                })
            }
            Ok(result) => Ok(EngineStartResult {
                engine: "ollama".to_string(),
                success: false,
                endpoint: None,
                error: Some(result.stderr),
            }),
            Err(e) => Ok(EngineStartResult {
                engine: "ollama".to_string(),
                success: false,
                endpoint: None,
                error: Some(e.to_string()),
            }),
        }
    }

    /// Performs full post-provisioning setup for a pod.
    ///
    /// This includes:
    /// 1. Waiting for the pod to be ready
    /// 2. Downloading and setting up models
    /// 3. Starting inference engines
    ///
    /// # Errors
    ///
    /// Returns an error if setup fails critically.
    pub async fn post_provision_setup(
        &self,
        pod_id: &str,
        pod_config: &PodConfig,
    ) -> Result<PostProvisionResult> {
        info!("Starting post-provisioning setup for pod {}", pod_id);

        // Wait for pod to be ready
        self.wait_for_ready(pod_id, 300).await?;

        // Setup models
        let model_results = self.setup_models(pod_id, &pod_config.models).await?;

        // Start engines for each model that has a load config
        let mut engine_results = Vec::new();
        
        // Get the primary HTTP port
        let port = pod_config
            .http_ports()
            .first()
            .copied()
            .unwrap_or(8000);

        for model in &pod_config.models {
            if model.load.is_some() {
                let result = self.start_inference_engine(pod_id, model, port).await?;
                engine_results.push(result);
            }
        }

        let success = model_results.iter().all(|r| r.success)
            && engine_results.iter().all(|r| r.success);

        Ok(PostProvisionResult {
            pod_id: pod_id.to_string(),
            success,
            model_results,
            engine_results,
        })
    }
}

/// Result of post-provisioning setup.
#[derive(Debug, Clone)]
pub struct PostProvisionResult {
    /// Pod ID.
    pub pod_id: String,
    /// Overall success status.
    pub success: bool,
    /// Individual model setup results.
    pub model_results: Vec<ModelSetupResult>,
    /// Individual engine startup results.
    pub engine_results: Vec<EngineStartResult>,
}

impl PostProvisionResult {
    /// Returns a summary of the setup.
    #[must_use]
    pub fn summary(&self) -> String {
        let models_ok = self.model_results.iter().filter(|r| r.success).count();
        let models_total = self.model_results.len();
        let engines_ok = self.engine_results.iter().filter(|r| r.success).count();
        let engines_total = self.engine_results.len();

        format!(
            "Pod {}: Models {}/{} OK, Engines {}/{} OK",
            self.pod_id, models_ok, models_total, engines_ok, engines_total
        )
    }
}
