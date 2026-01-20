//! Pod provisioner for creating and managing `RunPod` pods.
//!
//! This module handles the provisioning logic for pods, including
//! resource mapping, creation, and lifecycle management.

use std::collections::HashMap;
use tracing::{debug, info, warn};

use crate::config::{CloudType, GpuConfig, PodConfig, PortConfig, ProjectConfig, RuntimeConfig};
use crate::error::{HalldyllError, Result, RunPodError};

use super::client::RunPodClient;
use super::types::{CreatePodRequest, Pod, PodStatus};

/// Default volume size in GB.
const DEFAULT_VOLUME_GB: u32 = 50;

/// Default container disk size in GB.
const DEFAULT_CONTAINER_DISK_GB: u32 = 20;

/// Pod provisioner for managing `RunPod` pods.
#[derive(Debug)]
pub struct PodProvisioner {
    /// `RunPod` API client.
    client: RunPodClient,
    /// GPU type mapping (display name -> ID).
    gpu_type_map: HashMap<String, String>,
}

impl PodProvisioner {
    /// Creates a new pod provisioner.
    #[must_use]
    pub fn new(client: RunPodClient) -> Self {
        Self {
            client,
            gpu_type_map: HashMap::new(),
        }
    }

    /// Initializes the GPU type mapping by fetching available types.
    ///
    /// # Errors
    ///
    /// Returns an error if the GPU types cannot be fetched.
    pub async fn init_gpu_types(&mut self) -> Result<()> {
        info!("Fetching available GPU types");

        let gpu_types = self.client.list_gpu_types().await?;

        self.gpu_type_map.clear();
        for gpu in gpu_types {
            // Map both ID and display name to the ID
            self.gpu_type_map
                .insert(gpu.display_name.clone(), gpu.id.clone());
            self.gpu_type_map.insert(gpu.id.clone(), gpu.id);
        }

        debug!("Loaded {} GPU type mappings", self.gpu_type_map.len());
        Ok(())
    }

    /// Resolves a GPU type name to its `RunPod` ID.
    fn resolve_gpu_type(&self, gpu_type: &str) -> Option<&String> {
        self.gpu_type_map.get(gpu_type)
    }

    /// Creates a pod from a pod configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the pod cannot be created.
    pub async fn create_pod(
        &self,
        pod_config: &PodConfig,
        project: &ProjectConfig,
        spec_hash: &str,
    ) -> Result<Pod> {
        let full_name = pod_config.full_name(project);
        info!("Creating pod: {full_name}");

        // Resolve GPU type
        let gpu_type_id = self
            .resolve_gpu_type_with_fallback(&pod_config.gpu, &project.cloud_type)
            .await?;

        // Build the create request
        let request = Self::build_create_request(pod_config, project, &gpu_type_id, spec_hash);

        // Create the pod
        let pod = self.client.create_pod(&request).await?;

        info!(
            "Created pod: {} (ID: {})",
            full_name, pod.id
        );

        Ok(pod)
    }

    /// Creates a pod and performs post-provisioning setup (model download, engine start).
    ///
    /// # Errors
    ///
    /// Returns an error if the pod cannot be created or setup fails.
    pub async fn create_pod_with_setup(
        &self,
        pod_config: &PodConfig,
        project: &ProjectConfig,
        spec_hash: &str,
    ) -> Result<(Pod, Option<super::executor::PostProvisionResult>)> {
        // Create the pod first
        let pod = self.create_pod(pod_config, project, spec_hash).await?;

        // If there are models to setup, do post-provisioning
        if !pod_config.models.is_empty() {
            info!("Starting post-provisioning setup for pod {}", pod.id);
            
            let executor = super::executor::PodExecutor::new(self.client.clone());
            
            match executor.post_provision_setup(&pod.id, pod_config).await {
                Ok(result) => {
                    info!("Post-provisioning completed: {}", result.summary());
                    return Ok((pod, Some(result)));
                }
                Err(e) => {
                    warn!("Post-provisioning failed (pod still running): {}", e);
                    // Don't fail the whole operation, the pod is still usable
                }
            }
        }

        Ok((pod, None))
    }

    /// Resolves GPU type with fallback support.
    async fn resolve_gpu_type_with_fallback(
        &self,
        gpu_config: &GpuConfig,
        cloud_type: &CloudType,
    ) -> Result<String> {
        let cloud_type_str = match cloud_type {
            CloudType::Secure => "SECURE",
            CloudType::Community => "COMMUNITY",
        };

        // Try primary GPU type
        if let Some(gpu_id) = self.resolve_gpu_type(&gpu_config.gpu_type) {
            if self
                .client
                .is_gpu_available(gpu_id, cloud_type_str)
                .await?
            {
                debug!(
                    "Using primary GPU type: {} ({})",
                    gpu_config.gpu_type, gpu_id
                );
                return Ok(gpu_id.clone());
            }
            warn!(
                "Primary GPU type {} not available in {} cloud",
                gpu_config.gpu_type, cloud_type_str
            );
        }

        // Try fallback GPU types
        for fallback in &gpu_config.fallback {
            if let Some(gpu_id) = self.resolve_gpu_type(fallback) {
                if self
                    .client
                    .is_gpu_available(gpu_id, cloud_type_str)
                    .await?
                {
                    info!(
                        "Using fallback GPU type: {} ({})",
                        fallback, gpu_id
                    );
                    return Ok(gpu_id.clone());
                }
                debug!("Fallback GPU type {fallback} not available");
            }
        }

        Err(HalldyllError::RunPod(RunPodError::GpuNotAvailable {
            gpu_type: gpu_config.gpu_type.clone(),
            region: cloud_type_str.to_string(),
        }))
    }

    /// Builds a pod creation request from configuration.
    fn build_create_request(
        pod_config: &PodConfig,
        project: &ProjectConfig,
        gpu_type_id: &str,
        spec_hash: &str,
    ) -> CreatePodRequest {
        let full_name = pod_config.full_name(project);

        // Build ports string
        let ports = Self::build_ports_string(&pod_config.ports);

        // Calculate volume size
        let volume_gb = pod_config
            .volumes
            .iter()
            .filter_map(|v| v.size_gb)
            .max()
            .unwrap_or_default();
        let volume_gb = if volume_gb == 0 { DEFAULT_VOLUME_GB } else { volume_gb };

        // Get primary volume mount path
        let mount_path = pod_config
            .volumes
            .first()
            .map(|v| v.mount.clone());

        // Build environment variables
        let env = Self::build_env_vars(&pod_config.runtime);

        // Build tags
        let tags = Self::build_tags(pod_config, project, spec_hash);

        let cloud_type = match project.cloud_type {
            CloudType::Secure => "SECURE",
            CloudType::Community => "COMMUNITY",
        };

        let mut request = CreatePodRequest::new(&full_name, gpu_type_id, &pod_config.runtime.image)
            .with_cloud_type(cloud_type)
            .with_gpu_count(pod_config.gpu.count)
            .with_volume_gb(volume_gb)
            .with_container_disk_gb(DEFAULT_CONTAINER_DISK_GB)
            .with_ports(&ports)
            .with_env_map(env)
            .with_tags(tags);

        if let Some(path) = mount_path {
            request = request.with_mount_path(&path);
        }

        request
    }

    /// Builds the ports string for the API request.
    fn build_ports_string(ports: &[PortConfig]) -> String {
        if ports.is_empty() {
            return String::from("8000/http");
        }

        ports
            .iter()
            .map(|p| {
                let protocol = match p.protocol {
                    crate::config::PortProtocol::Tcp => "tcp",
                    crate::config::PortProtocol::Http | crate::config::PortProtocol::Https => "http",
                    crate::config::PortProtocol::Udp => "udp",
                };
                format!("{}/{protocol}", p.port)
            })
            .collect::<Vec<_>>()
            .join(",")
    }

    /// Builds environment variables map.
    fn build_env_vars(runtime: &RuntimeConfig) -> HashMap<String, String> {
        let mut env = runtime.env.clone();

        // Add HF token if available
        if let Ok(hf_token) = std::env::var("HF_TOKEN") {
            env.entry(String::from("HF_TOKEN"))
                .or_insert(hf_token);
        }

        env
    }

    /// Builds tags for the pod.
    fn build_tags(
        pod_config: &PodConfig,
        project: &ProjectConfig,
        spec_hash: &str,
    ) -> HashMap<String, String> {
        let mut tags = pod_config.tags.clone();

        // Add system tags
        tags.insert(String::from("halldyll_project"), project.name.clone());
        tags.insert(String::from("halldyll_env"), project.environment.clone());
        tags.insert(String::from("halldyll_pod"), pod_config.name.clone());
        tags.insert(String::from("halldyll_spec_hash"), spec_hash.to_string());

        tags
    }

    /// Terminates a pod.
    ///
    /// # Errors
    ///
    /// Returns an error if the pod cannot be terminated.
    pub async fn terminate_pod(&self, pod_id: &str) -> Result<()> {
        info!("Terminating pod: {pod_id}");
        self.client.terminate_pod(pod_id).await?;
        info!("Pod terminated: {pod_id}");
        Ok(())
    }

    /// Stops a pod (keeps it for later restart).
    ///
    /// # Errors
    ///
    /// Returns an error if the pod cannot be stopped.
    pub async fn stop_pod(&self, pod_id: &str) -> Result<()> {
        info!("Stopping pod: {pod_id}");
        self.client.stop_pod(pod_id).await?;
        info!("Pod stopped: {pod_id}");
        Ok(())
    }

    /// Resumes a stopped pod.
    ///
    /// # Errors
    ///
    /// Returns an error if the pod cannot be resumed.
    pub async fn resume_pod(&self, pod_id: &str) -> Result<Pod> {
        info!("Resuming pod: {pod_id}");
        let pod = self.client.resume_pod(pod_id).await?;
        info!("Pod resumed: {pod_id}");
        Ok(pod)
    }

    /// Waits for a pod to reach a specific status.
    ///
    /// # Errors
    ///
    /// Returns an error if the timeout is reached or the API call fails.
    pub async fn wait_for_status(
        &self,
        pod_id: &str,
        expected_status: PodStatus,
        timeout_secs: u64,
    ) -> Result<Pod> {
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(timeout_secs);

        loop {
            let pod = self.client.get_pod(pod_id).await?;

            if pod.desired_status == expected_status {
                return Ok(pod);
            }

            if start.elapsed() > timeout {
                return Err(HalldyllError::RunPod(RunPodError::Timeout {
                    pod_id: pod_id.to_string(),
                    expected_state: expected_status.to_string(),
                }));
            }

            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    }

    /// Gets the underlying client reference.
    #[must_use]
    pub const fn client(&self) -> &RunPodClient {
        &self.client
    }
}
