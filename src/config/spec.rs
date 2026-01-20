//! Configuration specification types for the deployment system.
//!
//! This module defines all the structs that map to the `halldyll.deploy.yaml` file.
//! These types are designed to be declarative and fully describe the desired state.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The root configuration structure for a Halldyll deployment.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeployConfig {
    /// Project-level configuration.
    pub project: ProjectConfig,
    /// State backend configuration.
    pub state: StateConfig,
    /// List of pods to deploy.
    pub pods: Vec<PodConfig>,
    /// Optional guardrails configuration.
    #[serde(default)]
    pub guardrails: Option<GuardrailsConfig>,
}

/// Project-level configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectConfig {
    /// Unique name for the project.
    pub name: String,
    /// Environment (e.g., "dev", "staging", "prod").
    #[serde(default = "default_environment")]
    pub environment: String,
    /// `RunPod` region preference.
    #[serde(default)]
    pub region: Option<String>,
    /// Cloud type (SECURE or COMMUNITY).
    #[serde(default)]
    pub cloud_type: CloudType,
    /// Compute type (GPU or CPU).
    #[serde(default)]
    pub compute_type: ComputeType,
}

/// State backend configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StateConfig {
    /// Backend type (local or s3).
    pub backend: StateBackend,
    /// S3 bucket name (required for s3 backend).
    #[serde(default)]
    pub bucket: Option<String>,
    /// S3 key prefix (optional).
    #[serde(default)]
    pub prefix: Option<String>,
    /// S3 region (optional, uses AWS default if not specified).
    #[serde(default)]
    pub region: Option<String>,
    /// Local state file path (for local backend).
    #[serde(default)]
    pub path: Option<String>,
}

/// State backend types.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum StateBackend {
    /// Local file-based state storage.
    #[default]
    Local,
    /// AWS S3-based state storage.
    S3,
}

/// Cloud type options.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "UPPERCASE")]
pub enum CloudType {
    /// Secure cloud (dedicated hardware).
    #[default]
    Secure,
    /// Community cloud (shared resources).
    Community,
}

/// Compute type options.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "UPPERCASE")]
pub enum ComputeType {
    /// GPU compute.
    #[default]
    Gpu,
    /// CPU-only compute.
    Cpu,
}

/// Configuration for a single pod.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PodConfig {
    /// Unique name for the pod within this project.
    pub name: String,
    /// GPU configuration.
    pub gpu: GpuConfig,
    /// Network ports to expose.
    #[serde(default)]
    pub ports: Vec<PortConfig>,
    /// Volume mounts.
    #[serde(default)]
    pub volumes: Vec<VolumeConfig>,
    /// Container runtime configuration.
    pub runtime: RuntimeConfig,
    /// Models to load on this pod.
    #[serde(default)]
    pub models: Vec<ModelConfig>,
    /// Optional health check configuration.
    #[serde(default)]
    pub health_check: Option<HealthCheckConfig>,
    /// Pod-specific tags (merged with project tags).
    #[serde(default)]
    pub tags: HashMap<String, String>,
}

/// GPU configuration for a pod.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GpuConfig {
    /// GPU type identifier (e.g., "NVIDIA A40", "NVIDIA RTX 4090").
    #[serde(rename = "type")]
    pub gpu_type: String,
    /// Number of GPUs to request.
    #[serde(default = "default_gpu_count")]
    pub count: u32,
    /// Minimum VRAM in GB (optional filter).
    #[serde(default)]
    pub min_vram_gb: Option<u32>,
    /// Fallback GPU types if primary is unavailable.
    #[serde(default)]
    pub fallback: Vec<String>,
}

/// Port configuration for a pod.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(try_from = "String", into = "String")]
pub struct PortConfig {
    /// Port number.
    pub port: u16,
    /// Protocol type.
    pub protocol: PortProtocol,
    /// Optional service name for documentation.
    pub name: Option<String>,
}

/// Port protocol types.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum PortProtocol {
    /// TCP protocol.
    Tcp,
    /// HTTP protocol (implies TCP).
    #[default]
    Http,
    /// HTTPS protocol (implies TCP).
    Https,
    /// UDP protocol.
    Udp,
}

/// Volume configuration for a pod.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VolumeConfig {
    /// Volume name.
    pub name: String,
    /// Mount path inside the container.
    pub mount: String,
    /// Whether the volume should persist across pod recreation.
    #[serde(default = "default_persistent")]
    pub persistent: bool,
    /// Size in GB (for new volumes).
    #[serde(default)]
    pub size_gb: Option<u32>,
}

/// Container runtime configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeConfig {
    /// Container image to use.
    pub image: String,
    /// Environment variables.
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// Optional command override.
    #[serde(default)]
    pub command: Option<Vec<String>>,
    /// Optional arguments.
    #[serde(default)]
    pub args: Option<Vec<String>>,
}

/// Model configuration for a pod.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelConfig {
    /// Unique identifier for the model within the pod.
    pub id: String,
    /// Model provider type.
    pub provider: ModelProvider,
    /// `HuggingFace` repository (for huggingface provider).
    #[serde(default)]
    pub repo: Option<String>,
    /// Model loading configuration.
    #[serde(default)]
    pub load: Option<LoadConfig>,
    /// Bundle components (for bundle provider).
    #[serde(default)]
    pub components: Option<Vec<String>>,
}

/// Model provider types.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ModelProvider {
    /// `HuggingFace` Hub.
    #[default]
    Huggingface,
    /// Pre-packaged bundle of models/components.
    Bundle,
    /// Custom/local model.
    Custom,
}

/// Model loading configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LoadConfig {
    /// Inference engine to use.
    pub engine: String,
    /// Quantization method (optional).
    #[serde(default)]
    pub quant: Option<String>,
    /// Maximum sequence length.
    #[serde(default)]
    pub max_seq_len: Option<u32>,
    /// Additional engine-specific options.
    #[serde(default)]
    pub options: HashMap<String, serde_json::Value>,
}

/// Health check configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HealthCheckConfig {
    /// HTTP endpoint to check.
    pub endpoint: String,
    /// Port to check on.
    pub port: u16,
    /// Interval between checks in seconds.
    #[serde(default = "default_health_interval")]
    pub interval_secs: u32,
    /// Timeout for each check in seconds.
    #[serde(default = "default_health_timeout")]
    pub timeout_secs: u32,
    /// Number of failures before marking unhealthy.
    #[serde(default = "default_health_threshold")]
    pub failure_threshold: u32,
}

/// Guardrails configuration for cost and resource limits.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GuardrailsConfig {
    /// Maximum hourly cost in USD.
    #[serde(default)]
    pub max_hourly_cost: Option<f64>,
    /// Maximum number of GPUs across all pods.
    #[serde(default)]
    pub max_gpus: Option<u32>,
    /// Time-to-live in hours (auto-stop after this time).
    #[serde(default)]
    pub ttl_hours: Option<u32>,
    /// Whether to allow fallback GPU types.
    #[serde(default = "default_allow_fallback")]
    pub allow_gpu_fallback: bool,
}

// Default value functions

const fn default_gpu_count() -> u32 {
    1
}

const fn default_persistent() -> bool {
    true
}

const fn default_health_interval() -> u32 {
    30
}

const fn default_health_timeout() -> u32 {
    5
}

const fn default_health_threshold() -> u32 {
    3
}

const fn default_allow_fallback() -> bool {
    false
}

fn default_environment() -> String {
    String::from("dev")
}

// Port config string conversion

impl TryFrom<String> for PortConfig {
    type Error = String;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::parse(&s)
    }
}

impl From<PortConfig> for String {
    fn from(port: PortConfig) -> Self {
        match port.protocol {
            PortProtocol::Tcp => format!("{}/tcp", port.port),
            PortProtocol::Http => format!("{}/http", port.port),
            PortProtocol::Https => format!("{}/https", port.port),
            PortProtocol::Udp => format!("{}/udp", port.port),
        }
    }
}

impl PortConfig {
    /// Parses a port configuration from a string like "8000/http".
    ///
    /// # Errors
    ///
    /// Returns an error if the port format is invalid.
    pub fn parse(s: &str) -> Result<Self, String> {
        let parts: Vec<&str> = s.split('/').collect();
        if parts.len() != 2 {
            return Err(format!("Invalid port format: {s}. Expected format: PORT/PROTOCOL"));
        }

        let port = parts[0]
            .parse::<u16>()
            .map_err(|_| format!("Invalid port number: {}", parts[0]))?;

        let protocol = match parts[1].to_lowercase().as_str() {
            "tcp" => PortProtocol::Tcp,
            "http" => PortProtocol::Http,
            "https" => PortProtocol::Https,
            "udp" => PortProtocol::Udp,
            other => return Err(format!("Invalid protocol: {other}. Expected: tcp, http, https, or udp")),
        };

        Ok(Self {
            port,
            protocol,
            name: None,
        })
    }

    /// Creates a new port configuration.
    #[must_use]
    pub const fn new(port: u16, protocol: PortProtocol) -> Self {
        Self {
            port,
            protocol,
            name: None,
        }
    }
}

impl DeployConfig {
    /// Returns the fully qualified project name including environment.
    #[must_use]
    pub fn qualified_name(&self) -> String {
        format!("{}-{}", self.project.name, self.project.environment)
    }

    /// Returns the total number of GPUs requested across all pods.
    #[must_use]
    pub fn total_gpus(&self) -> u32 {
        self.pods.iter().map(|p| p.gpu.count).sum()
    }

    /// Returns pod names.
    #[must_use]
    pub fn pod_names(&self) -> Vec<&str> {
        self.pods.iter().map(|p| p.name.as_str()).collect()
    }
}

impl PodConfig {
    /// Returns the full pod name including project context.
    #[must_use]
    pub fn full_name(&self, project: &ProjectConfig) -> String {
        format!("{}-{}-{}", project.name, project.environment, self.name)
    }

    /// Returns HTTP ports configured for this pod.
    #[must_use]
    pub fn http_ports(&self) -> Vec<u16> {
        self.ports
            .iter()
            .filter(|p| matches!(p.protocol, PortProtocol::Http | PortProtocol::Https))
            .map(|p| p.port)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_port_config_parse() {
        let port = PortConfig::parse("8000/http");
        assert!(port.is_ok());
        let port = port.unwrap();
        assert_eq!(port.port, 8000);
        assert_eq!(port.protocol, PortProtocol::Http);
    }

    #[test]
    fn test_port_config_parse_tcp() {
        let port = PortConfig::parse("22/tcp");
        assert!(port.is_ok());
        let port = port.unwrap();
        assert_eq!(port.port, 22);
        assert_eq!(port.protocol, PortProtocol::Tcp);
    }

    #[test]
    fn test_port_config_invalid() {
        let port = PortConfig::parse("invalid");
        assert!(port.is_err());
    }
}
