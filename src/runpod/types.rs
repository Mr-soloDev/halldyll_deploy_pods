//! `RunPod` API types and data structures.
//!
//! This module defines the types used for communication with the `RunPod` API.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A `RunPod` pod instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pod {
    /// Unique pod identifier.
    pub id: String,
    /// Pod name.
    pub name: String,
    /// Current status.
    #[serde(default)]
    pub desired_status: PodStatus,
    /// Image name.
    #[serde(default)]
    pub image_name: String,
    /// Machine information.
    #[serde(default)]
    pub machine: Option<PodMachine>,
    /// Runtime information.
    #[serde(default)]
    pub runtime: Option<PodRuntime>,
    /// GPU count.
    #[serde(default)]
    pub gpu_count: u32,
    /// Volume in GB.
    #[serde(default)]
    pub volume_in_gb: u32,
    /// Container disk in GB.
    #[serde(default)]
    pub container_disk_in_gb: u32,
    /// Memory in GB.
    #[serde(default)]
    pub memory_in_gb: u32,
    /// vCPU count.
    #[serde(default)]
    pub vcpu_count: u32,
    /// Ports exposed.
    #[serde(default)]
    pub ports: Option<String>,
    /// Environment variables.
    #[serde(default)]
    pub env: Vec<PodEnvVar>,
    /// Custom tags on the pod.
    #[serde(default)]
    pub custom_tags: Option<HashMap<String, String>>,
}

/// Pod machine information.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PodMachine {
    /// GPU type identifier.
    pub gpu_type_id: Option<String>,
}

/// Pod runtime information.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PodRuntime {
    /// Ports configuration.
    #[serde(default)]
    pub ports: Vec<PodPort>,
    /// GPU information.
    #[serde(default)]
    pub gpus: Vec<RunPodGpu>,
    /// Uptime in seconds.
    #[serde(default)]
    pub uptime_in_seconds: u64,
}

/// Pod port information.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PodPort {
    /// Internal port.
    pub ip: String,
    /// Private port.
    pub private_port: u16,
    /// Public port.
    pub public_port: Option<u16>,
    /// Port type.
    #[serde(rename = "type")]
    pub port_type: Option<String>,
}

/// Pod environment variable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodEnvVar {
    /// Variable key.
    pub key: String,
    /// Variable value.
    pub value: String,
}

/// GPU information from `RunPod`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunPodGpu {
    /// GPU ID.
    pub id: String,
    /// GPU name.
    #[serde(default)]
    pub gpu_utilization_percent: f32,
    /// Memory utilization.
    #[serde(default)]
    pub memory_utilization_percent: f32,
}

/// Pod status enum.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PodStatus {
    /// Pod is running.
    Running,
    /// Pod is starting.
    Starting,
    /// Pod is exited.
    Exited,
    /// Pod is stopped.
    Stopped,
    /// Pod is being created.
    Creating,
    /// Unknown status.
    #[default]
    Unknown,
}

/// Request to create a new pod.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePodRequest {
    /// Cloud type (SECURE or COMMUNITY).
    pub cloud_type: String,
    /// GPU type ID.
    pub gpu_type_id: String,
    /// Number of GPUs.
    pub gpu_count: u32,
    /// Pod name.
    pub name: String,
    /// Container image.
    pub image_name: String,
    /// Volume in GB.
    pub volume_in_gb: u32,
    /// Container disk in GB.
    pub container_disk_in_gb: u32,
    /// Volume mount path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_mount_path: Option<String>,
    /// Ports to expose (e.g., "8000/http,22/tcp").
    pub ports: String,
    /// Environment variables.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<PodEnvVar>,
    /// Docker command arguments.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub docker_args: Option<String>,
    /// Data center ID preference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_center_id: Option<String>,
    /// Minimum VRAM requirement.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_vcpu_count: Option<u32>,
    /// Minimum memory requirement.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_memory_in_gb: Option<u32>,
    /// Network volume ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_volume_id: Option<String>,
    /// Custom tags.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_tags: Option<HashMap<String, String>>,
}

/// Request to update a pod.
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePodRequest {
    /// Pod ID.
    pub pod_id: String,
    /// New image name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_name: Option<String>,
    /// New environment variables.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<Vec<PodEnvVar>>,
}

/// GPU type information.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuType {
    /// GPU type ID.
    pub id: String,
    /// Display name.
    pub display_name: String,
    /// Memory in GB.
    pub memory_in_gb: u32,
    /// Whether it's available in secure cloud.
    #[serde(default)]
    pub secure_cloud: bool,
    /// Whether it's available in community cloud.
    #[serde(default)]
    pub community_cloud: bool,
    /// Price per hour for secure cloud (if available).
    #[serde(default)]
    pub secure_price: Option<f64>,
    /// Price per hour for community cloud (if available).
    #[serde(default)]
    pub community_price: Option<f64>,
}

/// Pod endpoint information.
#[derive(Debug, Clone)]
pub struct PodEndpoint {
    /// Port number.
    pub port: u16,
    /// Full URL to access this endpoint.
    pub url: String,
    /// Protocol type.
    pub protocol: String,
}

impl Pod {
    /// Returns the GPU type name if available.
    #[must_use]
    pub fn gpu_type_name(&self) -> Option<&str> {
        self.machine
            .as_ref()
            .and_then(|m| m.gpu_type_id.as_deref())
    }

    /// Returns the public endpoints for this pod.
    #[must_use]
    pub fn endpoints(&self) -> Vec<PodEndpoint> {
        let mut endpoints = Vec::new();

        if let Some(runtime) = &self.runtime {
            for port in &runtime.ports {
                if let Some(public_port) = port.public_port {
                    let protocol = port.port_type.as_deref().unwrap_or("http");
                    let url = format!("https://{}-{}.proxy.runpod.net", self.id, public_port);
                    endpoints.push(PodEndpoint {
                        port: port.private_port,
                        url,
                        protocol: protocol.to_string(),
                    });
                }
            }
        }

        endpoints
    }

    /// Checks if the pod is running.
    #[must_use]
    pub const fn is_running(&self) -> bool {
        matches!(self.desired_status, PodStatus::Running)
    }

    /// Gets a custom tag value.
    #[must_use]
    pub fn get_tag(&self, key: &str) -> Option<&str> {
        self.custom_tags
            .as_ref()
            .and_then(|tags| tags.get(key).map(String::as_str))
    }
}

impl CreatePodRequest {
    /// Creates a new pod creation request.
    #[must_use]
    pub fn new(name: &str, gpu_type_id: &str, image: &str) -> Self {
        Self {
            cloud_type: String::from("SECURE"),
            gpu_type_id: gpu_type_id.to_string(),
            gpu_count: 1,
            name: name.to_string(),
            image_name: image.to_string(),
            volume_in_gb: 20,
            container_disk_in_gb: 20,
            volume_mount_path: None,
            ports: String::from("8000/http"),
            env: Vec::new(),
            docker_args: None,
            data_center_id: None,
            min_vcpu_count: None,
            min_memory_in_gb: None,
            network_volume_id: None,
            custom_tags: None,
        }
    }

    /// Sets the cloud type.
    #[must_use]
    pub fn with_cloud_type(mut self, cloud_type: &str) -> Self {
        self.cloud_type = cloud_type.to_string();
        self
    }

    /// Sets the GPU count.
    #[must_use]
    pub const fn with_gpu_count(mut self, count: u32) -> Self {
        self.gpu_count = count;
        self
    }

    /// Sets the volume size.
    #[must_use]
    pub const fn with_volume_gb(mut self, size_gb: u32) -> Self {
        self.volume_in_gb = size_gb;
        self
    }

    /// Sets the container disk size.
    #[must_use]
    pub const fn with_container_disk_gb(mut self, size_gb: u32) -> Self {
        self.container_disk_in_gb = size_gb;
        self
    }

    /// Sets the volume mount path.
    #[must_use]
    pub fn with_mount_path(mut self, path: &str) -> Self {
        self.volume_mount_path = Some(path.to_string());
        self
    }

    /// Sets the ports to expose.
    #[must_use]
    pub fn with_ports(mut self, ports: &str) -> Self {
        self.ports = ports.to_string();
        self
    }

    /// Adds an environment variable.
    #[must_use]
    pub fn with_env(mut self, key: &str, value: &str) -> Self {
        self.env.push(PodEnvVar {
            key: key.to_string(),
            value: value.to_string(),
        });
        self
    }

    /// Sets all environment variables.
    #[must_use]
    pub fn with_env_map(mut self, env: HashMap<String, String>) -> Self {
        self.env = env
            .into_iter()
            .map(|(key, value)| PodEnvVar { key, value })
            .collect();
        self
    }

    /// Sets custom tags.
    #[must_use]
    pub fn with_tags(mut self, tags: HashMap<String, String>) -> Self {
        self.custom_tags = Some(tags);
        self
    }

    /// Adds a single tag.
    #[must_use]
    pub fn with_tag(mut self, key: &str, value: &str) -> Self {
        self.custom_tags
            .get_or_insert_with(HashMap::new)
            .insert(key.to_string(), value.to_string());
        self
    }
}

impl std::fmt::Display for PodStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let status = match self {
            Self::Running => "running",
            Self::Starting => "starting",
            Self::Exited => "exited",
            Self::Stopped => "stopped",
            Self::Creating => "creating",
            Self::Unknown => "unknown",
        };
        write!(f, "{status}")
    }
}
