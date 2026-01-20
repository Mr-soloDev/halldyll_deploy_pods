//! Configuration module for Halldyll deployment system.
//!
//! This module handles all configuration-related functionality:
//! - Parsing and deserializing `halldyll.deploy.yaml`
//! - Validation of configuration values
//! - Computing configuration hashes for change detection

mod spec;
mod parser;
mod validator;
mod hash;

pub use spec::{
    CloudType, ComputeType, DeployConfig, GpuConfig, GuardrailsConfig, HealthCheckConfig,
    LoadConfig, ModelConfig, ModelProvider, PodConfig, PortConfig, PortProtocol, ProjectConfig,
    RuntimeConfig, StateBackend, StateConfig, VolumeConfig,
};
pub use parser::{ConfigParser, find_config_file};
pub use validator::ConfigValidator;
pub use hash::ConfigHasher;
