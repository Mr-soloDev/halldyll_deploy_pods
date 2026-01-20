//! Configuration validation for deployment specs.
//!
//! This module provides comprehensive validation of deployment configurations,
//! ensuring all values are valid and consistent before deployment.

use crate::error::{ConfigError, HalldyllError, Result};
use std::collections::HashSet;
use tracing::debug;

use super::spec::{DeployConfig, PodConfig, StateBackend, VolumeConfig};

/// Validator for deployment configurations.
#[derive(Debug, Default)]
pub struct ConfigValidator {
    /// Known valid GPU types.
    known_gpu_types: HashSet<String>,
}

/// Known GPU types supported by `RunPod`.
const KNOWN_GPU_TYPES: &[&str] = &[
    "NVIDIA A40",
    "NVIDIA A100 80GB PCIe",
    "NVIDIA A100-SXM4-80GB",
    "NVIDIA GeForce RTX 3070",
    "NVIDIA GeForce RTX 3080",
    "NVIDIA GeForce RTX 3080 Ti",
    "NVIDIA GeForce RTX 3090",
    "NVIDIA GeForce RTX 3090 Ti",
    "NVIDIA GeForce RTX 4070 Ti",
    "NVIDIA GeForce RTX 4080",
    "NVIDIA GeForce RTX 4090",
    "NVIDIA H100 80GB HBM3",
    "NVIDIA H100 PCIe",
    "NVIDIA L4",
    "NVIDIA L40",
    "NVIDIA L40S",
    "NVIDIA RTX 4000 Ada Generation",
    "NVIDIA RTX 5000 Ada Generation",
    "NVIDIA RTX 6000 Ada Generation",
    "NVIDIA RTX A4000",
    "NVIDIA RTX A4500",
    "NVIDIA RTX A5000",
    "NVIDIA RTX A6000",
];

/// Validation result containing all errors found.
#[derive(Debug, Default)]
pub struct ValidationResult {
    /// List of validation errors.
    pub errors: Vec<ValidationError>,
    /// List of warnings (non-fatal issues).
    pub warnings: Vec<String>,
}

/// A single validation error.
#[derive(Debug)]
pub struct ValidationError {
    /// The field path that failed validation.
    pub field: String,
    /// The error message.
    pub message: String,
}

impl ConfigValidator {
    /// Creates a new validator with default known GPU types.
    #[must_use]
    pub fn new() -> Self {
        Self {
            known_gpu_types: KNOWN_GPU_TYPES.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    /// Adds a custom GPU type to the known list.
    pub fn add_gpu_type(&mut self, gpu_type: impl Into<String>) {
        self.known_gpu_types.insert(gpu_type.into());
    }

    /// Validates a deployment configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if validation fails.
    pub fn validate(&self, config: &DeployConfig) -> Result<ValidationResult> {
        let mut result = ValidationResult::default();

        Self::validate_project(&config.project, &mut result);
        Self::validate_state(&config.state, &mut result);
        self.validate_pods(&config.pods, &mut result);
        Self::validate_guardrails(config, &mut result);

        if result.errors.is_empty() {
            debug!("Configuration validation passed");
            Ok(result)
        } else {
            let first_error = &result.errors[0];
            Err(HalldyllError::Config(ConfigError::ValidationError {
                message: first_error.message.clone(),
                field: Some(first_error.field.clone()),
            }))
        }
    }

    /// Validates project configuration.
    fn validate_project(
        project: &super::spec::ProjectConfig,
        result: &mut ValidationResult,
    ) {
        // Project name must be valid
        if project.name.is_empty() {
            result.errors.push(ValidationError {
                field: String::from("project.name"),
                message: String::from("Project name cannot be empty"),
            });
        } else if !is_valid_name(&project.name) {
            result.errors.push(ValidationError {
                field: String::from("project.name"),
                message: format!(
                    "Project name '{}' is invalid. Must be lowercase alphanumeric with hyphens.",
                    project.name
                ),
            });
        }

        // Environment must be valid
        if project.environment.is_empty() {
            result.errors.push(ValidationError {
                field: String::from("project.environment"),
                message: String::from("Environment cannot be empty"),
            });
        }
    }

    /// Validates state configuration.
    fn validate_state(state: &super::spec::StateConfig, result: &mut ValidationResult) {
        match state.backend {
            StateBackend::S3 => {
                if state.bucket.is_none() || state.bucket.as_ref().is_some_and(String::is_empty) {
                    result.errors.push(ValidationError {
                        field: String::from("state.bucket"),
                        message: String::from("S3 bucket name is required when using S3 backend"),
                    });
                }
            }
            StateBackend::Local => {
                // Local backend is always valid
            }
        }
    }

    /// Validates all pod configurations.
    fn validate_pods(&self, pods: &[PodConfig], result: &mut ValidationResult) {
        if pods.is_empty() {
            result.warnings.push(String::from("No pods defined in configuration"));
            return;
        }

        let mut seen_names = HashSet::new();
        let mut all_ports: HashSet<u16> = HashSet::new();

        for (i, pod) in pods.iter().enumerate() {
            let prefix = format!("pods[{i}]");

            // Validate unique name
            if seen_names.contains(&pod.name) {
                result.errors.push(ValidationError {
                    field: format!("{prefix}.name"),
                    message: format!("Duplicate pod name: {}", pod.name),
                });
            } else {
                seen_names.insert(&pod.name);
            }

            // Validate pod name format
            if !is_valid_name(&pod.name) {
                result.errors.push(ValidationError {
                    field: format!("{prefix}.name"),
                    message: format!(
                        "Pod name '{}' is invalid. Must be lowercase alphanumeric with hyphens.",
                        pod.name
                    ),
                });
            }

            // Validate GPU
            self.validate_gpu(&pod.gpu, &prefix, result);

            // Validate ports
            Self::validate_ports(&pod.ports, &prefix, &mut all_ports, result);

            // Validate volumes
            Self::validate_volumes(&pod.volumes, &prefix, result);

            // Validate runtime
            Self::validate_runtime(&pod.runtime, &prefix, result);

            // Validate models
            Self::validate_models(&pod.models, &prefix, result);
        }
    }

    /// Validates GPU configuration.
    fn validate_gpu(
        &self,
        gpu: &super::spec::GpuConfig,
        prefix: &str,
        result: &mut ValidationResult,
    ) {
        if gpu.count == 0 {
            result.errors.push(ValidationError {
                field: format!("{prefix}.gpu.count"),
                message: String::from("GPU count must be at least 1"),
            });
        }

        if gpu.count > 8 {
            result.warnings.push(format!(
                "{prefix}.gpu.count: Requesting {count} GPUs is unusual",
                count = gpu.count
            ));
        }

        if !self.known_gpu_types.contains(&gpu.gpu_type) {
            result.warnings.push(format!(
                "{prefix}.gpu.type: Unknown GPU type '{}'. This may fail if not available.",
                gpu.gpu_type
            ));
        }

        // Validate fallback GPU types
        for (i, fallback) in gpu.fallback.iter().enumerate() {
            if !self.known_gpu_types.contains(fallback) {
                result.warnings.push(format!(
                    "{prefix}.gpu.fallback[{i}]: Unknown fallback GPU type '{fallback}'",
                ));
            }
        }
    }

    /// Validates port configurations.
    fn validate_ports(
        ports: &[super::spec::PortConfig],
        prefix: &str,
        all_ports: &mut HashSet<u16>,
        result: &mut ValidationResult,
    ) {
        let mut pod_ports = HashSet::new();

        for (i, port) in ports.iter().enumerate() {
            // Check for duplicate ports within the same pod
            if pod_ports.contains(&port.port) {
                result.errors.push(ValidationError {
                    field: format!("{prefix}.ports[{i}]"),
                    message: format!("Duplicate port {} in pod", port.port),
                });
            } else {
                pod_ports.insert(port.port);
            }

            // Check for reserved ports
            if port.port < 1024 && port.port != 22 && port.port != 80 && port.port != 443 {
                result.warnings.push(format!(
                    "{prefix}.ports[{i}]: Port {} is in the reserved range (<1024)",
                    port.port
                ));
            }
        }

        // Add all ports to global set (for cross-pod collision detection)
        all_ports.extend(pod_ports);
    }

    /// Validates volume configurations.
    fn validate_volumes(
        volumes: &[VolumeConfig],
        prefix: &str,
        result: &mut ValidationResult,
    ) {
        let mut seen_names = HashSet::new();
        let mut seen_mounts = HashSet::new();

        for (i, volume) in volumes.iter().enumerate() {
            // Check for duplicate names
            if seen_names.contains(&volume.name) {
                result.errors.push(ValidationError {
                    field: format!("{prefix}.volumes[{i}].name"),
                    message: format!("Duplicate volume name: {}", volume.name),
                });
            } else {
                seen_names.insert(&volume.name);
            }

            // Check for duplicate mount paths
            if seen_mounts.contains(&volume.mount) {
                result.errors.push(ValidationError {
                    field: format!("{prefix}.volumes[{i}].mount"),
                    message: format!("Duplicate mount path: {}", volume.mount),
                });
            } else {
                seen_mounts.insert(&volume.mount);
            }

            // Validate mount path is absolute
            if !volume.mount.starts_with('/') {
                result.errors.push(ValidationError {
                    field: format!("{prefix}.volumes[{i}].mount"),
                    message: format!("Mount path must be absolute: {}", volume.mount),
                });
            }
        }
    }

    /// Validates runtime configuration.
    fn validate_runtime(
        runtime: &super::spec::RuntimeConfig,
        prefix: &str,
        result: &mut ValidationResult,
    ) {
        if runtime.image.is_empty() {
            result.errors.push(ValidationError {
                field: format!("{prefix}.runtime.image"),
                message: String::from("Container image cannot be empty"),
            });
        }

        // Warn about latest tag
        if runtime.image.ends_with(":latest") {
            result.warnings.push(format!(
                "{prefix}.runtime.image: Using ':latest' tag is not recommended for production"
            ));
        }
    }

    /// Validates model configurations.
    fn validate_models(
        models: &[super::spec::ModelConfig],
        prefix: &str,
        result: &mut ValidationResult,
    ) {
        let mut seen_ids = HashSet::new();

        for (i, model) in models.iter().enumerate() {
            // Check for duplicate IDs
            if seen_ids.contains(&model.id) {
                result.errors.push(ValidationError {
                    field: format!("{prefix}.models[{i}].id"),
                    message: format!("Duplicate model ID: {}", model.id),
                });
            } else {
                seen_ids.insert(&model.id);
            }

            // Validate HuggingFace models have a repo
            if model.provider == super::spec::ModelProvider::Huggingface
                && model.repo.is_none()
            {
                result.errors.push(ValidationError {
                    field: format!("{prefix}.models[{i}].repo"),
                    message: format!(
                        "Model '{}' uses huggingface provider but no repo specified",
                        model.id
                    ),
                });
            }

            // Validate bundle models have components
            if model.provider == super::spec::ModelProvider::Bundle
                && model.components.as_ref().is_none_or(Vec::is_empty)
            {
                result.errors.push(ValidationError {
                    field: format!("{prefix}.models[{i}].components"),
                    message: format!(
                        "Model '{}' uses bundle provider but no components specified",
                        model.id
                    ),
                });
            }
        }
    }

    /// Validates guardrails configuration.
    fn validate_guardrails(config: &DeployConfig, result: &mut ValidationResult) {
        if let Some(guardrails) = &config.guardrails {
            // Validate max_hourly_cost
            if let Some(cost) = guardrails.max_hourly_cost
                && cost <= 0.0 {
                    result.errors.push(ValidationError {
                        field: String::from("guardrails.max_hourly_cost"),
                        message: String::from("Maximum hourly cost must be positive"),
                    });
                }

            // Validate max_gpus against actual pod requirements
            if let Some(max_gpus) = guardrails.max_gpus {
                let total_gpus = config.total_gpus();
                if total_gpus > max_gpus {
                    result.errors.push(ValidationError {
                        field: String::from("guardrails.max_gpus"),
                        message: format!(
                            "Configuration requires {total_gpus} GPUs but max_gpus is {max_gpus}"
                        ),
                    });
                }
            }

            // Validate TTL
            if let Some(ttl) = guardrails.ttl_hours
                && ttl == 0 {
                    result.errors.push(ValidationError {
                        field: String::from("guardrails.ttl_hours"),
                        message: String::from("TTL must be at least 1 hour"),
                    });
                }
        }
    }
}

/// Validates that a name follows the naming convention.
/// Names must be lowercase alphanumeric with hyphens, starting with a letter.
fn is_valid_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }

    let mut chars = name.chars();

    // First character must be a letter
    if let Some(first) = chars.next()
        && !first.is_ascii_lowercase() {
            return false;
        }

    // Rest must be lowercase alphanumeric or hyphen
    for c in chars {
        if !c.is_ascii_lowercase() && !c.is_ascii_digit() && c != '-' {
            return false;
        }
    }

    // Cannot end with hyphen
    if name.ends_with('-') {
        return false;
    }

    // Cannot have consecutive hyphens
    if name.contains("--") {
        return false;
    }

    true
}

impl ValidationResult {
    /// Returns true if validation passed (no errors).
    #[must_use]
    pub const fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }

    /// Returns the number of errors.
    #[must_use]
    pub const fn error_count(&self) -> usize {
        self.errors.len()
    }

    /// Returns the number of warnings.
    #[must_use]
    pub const fn warning_count(&self) -> usize {
        self.warnings.len()
    }
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.field, self.message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_name() {
        assert!(is_valid_name("pod-text"));
        assert!(is_valid_name("my-pod-123"));
        assert!(is_valid_name("a"));
        assert!(is_valid_name("test"));
    }

    #[test]
    fn test_invalid_name() {
        assert!(!is_valid_name(""));
        assert!(!is_valid_name("Pod-Text")); // uppercase
        assert!(!is_valid_name("123-pod")); // starts with number
        assert!(!is_valid_name("pod_text")); // underscore
        assert!(!is_valid_name("pod-")); // ends with hyphen
        assert!(!is_valid_name("pod--text")); // consecutive hyphens
    }
}
