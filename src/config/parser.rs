//! Configuration parser for loading and merging configuration files.
//!
//! This module handles loading configuration from YAML files and environment
//! variables, with proper precedence and error handling.

use crate::error::{ConfigError, HalldyllError, Result};
use std::path::Path;
use tracing::{debug, info};

use super::spec::DeployConfig;

/// Configuration parser for loading deployment configuration.
#[derive(Debug, Default)]
pub struct ConfigParser {
    /// Base path for resolving relative paths.
    base_path: Option<std::path::PathBuf>,
}

impl ConfigParser {
    /// Creates a new configuration parser.
    #[must_use]
    pub const fn new() -> Self {
        Self { base_path: None }
    }

    /// Sets the base path for resolving relative paths.
    #[must_use]
    pub fn with_base_path(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.base_path = Some(path.into());
        self
    }

    /// Loads configuration from a YAML file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or parsed.
    pub fn load_file(&self, path: impl AsRef<Path>) -> Result<DeployConfig> {
        let path = path.as_ref();
        info!("Loading configuration from: {}", path.display());

        if !path.exists() {
            return Err(HalldyllError::Config(ConfigError::FileNotFound {
                path: path.to_path_buf(),
            }));
        }

        let content = std::fs::read_to_string(path).map_err(|e| {
            HalldyllError::Config(ConfigError::ParseError {
                message: format!("Failed to read file: {e}"),
                location: Some(path.display().to_string()),
            })
        })?;

        self.parse_yaml(&content, Some(path))
    }

    /// Parses configuration from a YAML string.
    ///
    /// # Errors
    ///
    /// Returns an error if the YAML is invalid.
    pub fn parse_yaml(&self, content: &str, source: Option<&Path>) -> Result<DeployConfig> {
        debug!("Parsing YAML configuration");

        let config: DeployConfig = serde_yaml::from_str(content).map_err(|e| {
            let location = source.map(|p| p.display().to_string());
            HalldyllError::Config(ConfigError::ParseError {
                message: format!("YAML parse error: {e}"),
                location,
            })
        })?;

        debug!("Successfully parsed configuration for project: {}", config.project.name);
        Ok(config)
    }

    /// Loads configuration with environment variable overrides.
    ///
    /// Environment variables are checked in the format:
    /// `HALLDYLL_<SECTION>_<KEY>` (e.g., `HALLDYLL_PROJECT_NAME`)
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or parsed.
    pub fn load_with_env(&self, path: impl AsRef<Path>) -> Result<DeployConfig> {
        let mut config = self.load_file(path)?;

        // Apply environment overrides
        Self::apply_env_overrides(&mut config);

        Ok(config)
    }

    /// Applies environment variable overrides to the configuration.
    fn apply_env_overrides(config: &mut DeployConfig) {
        // Project overrides
        if let Ok(name) = std::env::var("HALLDYLL_PROJECT_NAME") {
            debug!("Overriding project.name from environment");
            config.project.name = name;
        }

        if let Ok(env) = std::env::var("HALLDYLL_PROJECT_ENVIRONMENT") {
            debug!("Overriding project.environment from environment");
            config.project.environment = env;
        }

        if let Ok(region) = std::env::var("HALLDYLL_PROJECT_REGION") {
            debug!("Overriding project.region from environment");
            config.project.region = Some(region);
        }

        // State overrides
        if let Ok(bucket) = std::env::var("HALLDYLL_STATE_BUCKET") {
            debug!("Overriding state.bucket from environment");
            config.state.bucket = Some(bucket);
        }

        if let Ok(prefix) = std::env::var("HALLDYLL_STATE_PREFIX") {
            debug!("Overriding state.prefix from environment");
            config.state.prefix = Some(prefix);
        }
    }

    /// Loads the .env file if present.
    ///
    /// # Errors
    ///
    /// Returns an error if the .env file exists but cannot be loaded.
    pub fn load_dotenv(&self) -> Result<()> {
        let env_path = self
            .base_path
            .as_ref()
            .map_or_else(|| std::path::PathBuf::from(".env"), |p| p.join(".env"));

        if env_path.exists() {
            info!("Loading environment from: {}", env_path.display());
            dotenvy::from_path(&env_path).map_err(|e| {
                HalldyllError::Config(ConfigError::ParseError {
                    message: format!("Failed to load .env file: {e}"),
                    location: Some(env_path.display().to_string()),
                })
            })?;
        } else {
            debug!(".env file not found at: {}", env_path.display());
        }

        Ok(())
    }

    /// Validates that required environment variables are set.
    ///
    /// # Errors
    ///
    /// Returns an error if required environment variables are missing.
    pub fn validate_required_env(&self) -> Result<()> {
        const REQUIRED_VARS: &[&str] = &["RUNPOD_API_KEY"];

        for var in REQUIRED_VARS {
            if std::env::var(var).is_err() {
                return Err(HalldyllError::Config(ConfigError::MissingEnvVar {
                    name: (*var).to_string(),
                }));
            }
        }

        Ok(())
    }

    /// Gets the `RunPod` API key from environment.
    ///
    /// # Errors
    ///
    /// Returns an error if the API key is not set.
    pub fn get_runpod_api_key() -> Result<String> {
        std::env::var("RUNPOD_API_KEY").map_err(|_| {
            HalldyllError::Config(ConfigError::MissingEnvVar {
                name: String::from("RUNPOD_API_KEY"),
            })
        })
    }

    /// Gets the `HuggingFace` token from environment (optional).
    #[must_use]
    pub fn get_hf_token() -> Option<String> {
        std::env::var("HF_TOKEN").ok()
    }
}

/// Default configuration file names to search for.
pub const DEFAULT_CONFIG_FILES: &[&str] = &[
    "halldyll.deploy.yaml",
    "halldyll.deploy.yml",
    "deploy.yaml",
    "deploy.yml",
];

/// Finds the configuration file in the current directory or parent directories.
///
/// # Errors
///
/// Returns an error if no configuration file is found.
pub fn find_config_file(start_dir: impl AsRef<Path>) -> Result<std::path::PathBuf> {
    let start = start_dir.as_ref();
    let mut current = start.to_path_buf();

    loop {
        for filename in DEFAULT_CONFIG_FILES {
            let config_path = current.join(filename);
            if config_path.exists() {
                info!("Found configuration file: {}", config_path.display());
                return Ok(config_path);
            }
        }

        if !current.pop() {
            break;
        }
    }

    Err(HalldyllError::Config(ConfigError::FileNotFound {
        path: start.join(DEFAULT_CONFIG_FILES[0]),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minimal_config() {
        let yaml = r"
project:
  name: test-project
state:
  backend: local
pods: []
";
        let parser = ConfigParser::new();
        let result = parser.parse_yaml(yaml, None);
        assert!(result.is_ok());

        let config = result.unwrap();
        assert_eq!(config.project.name, "test-project");
        assert_eq!(config.project.environment, "dev");
    }

    #[test]
    fn test_parse_full_config() {
        let yaml = r#"
project:
  name: halldyll-agent
  environment: prod
  region: EU
  cloud_type: SECURE
  compute_type: GPU

state:
  backend: s3
  bucket: halldyll-state
  prefix: halldyll-agent/prod

pods:
  - name: pod-text
    gpu:
      type: "NVIDIA A40"
      count: 1
    ports:
      - "22/tcp"
      - "8000/http"
    volumes:
      - name: hf-cache
        mount: /root/.cache/huggingface
        persistent: true
    runtime:
      image: ghcr.io/halldyll/pod-text:latest
      env:
        VLLM_PORT: "8000"
    models:
      - id: qwen2.5-14b-instruct
        provider: huggingface
        repo: Qwen/Qwen2.5-14B-Instruct
        load:
          engine: vllm
          quant: awq
          max_seq_len: 8192
"#;
        let parser = ConfigParser::new();
        let result = parser.parse_yaml(yaml, None);
        assert!(result.is_ok());

        let config = result.unwrap();
        assert_eq!(config.project.name, "halldyll-agent");
        assert_eq!(config.pods.len(), 1);
        assert_eq!(config.pods[0].name, "pod-text");
        assert_eq!(config.pods[0].models.len(), 1);
    }
}
