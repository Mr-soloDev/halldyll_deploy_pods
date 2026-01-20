//! Pod observer for monitoring `RunPod` pods.
//!
//! This module provides functionality for observing and querying the state
//! of pods on `RunPod`, including filtering by tags for project-specific queries.

use std::collections::HashMap;
use tracing::{debug, info};

use crate::error::Result;

use super::client::RunPodClient;
use super::types::{Pod, PodStatus};

/// Tag key for project identification.
pub const TAG_PROJECT: &str = "halldyll_project";

/// Tag key for environment identification.
pub const TAG_ENV: &str = "halldyll_env";

/// Tag key for pod name.
pub const TAG_POD: &str = "halldyll_pod";

/// Tag key for spec hash.
pub const TAG_SPEC_HASH: &str = "halldyll_spec_hash";

/// Pod observer for monitoring pods.
#[derive(Debug)]
pub struct PodObserver {
    /// `RunPod` API client.
    client: RunPodClient,
}

/// Observed pod information.
#[derive(Debug, Clone)]
pub struct ObservedPod {
    /// Pod ID.
    pub id: String,
    /// Pod name.
    pub name: String,
    /// Project name (from tags).
    pub project: Option<String>,
    /// Environment (from tags).
    pub environment: Option<String>,
    /// Local pod name (from tags).
    pub pod_name: Option<String>,
    /// Spec hash (from tags).
    pub spec_hash: Option<String>,
    /// Current status.
    pub status: PodStatus,
    /// GPU type.
    pub gpu_type: Option<String>,
    /// GPU count.
    pub gpu_count: u32,
    /// Container image.
    pub image: String,
    /// Endpoints.
    pub endpoints: HashMap<u16, String>,
    /// All tags.
    pub tags: HashMap<String, String>,
}

impl PodObserver {
    /// Creates a new pod observer.
    #[must_use]
    pub const fn new(client: RunPodClient) -> Self {
        Self { client }
    }

    /// Lists all pods owned by the current account.
    ///
    /// # Errors
    ///
    /// Returns an error if the API call fails.
    pub async fn list_all_pods(&self) -> Result<Vec<ObservedPod>> {
        info!("Listing all pods");

        let pods = self.client.list_pods().await?;
        let observed: Vec<ObservedPod> = pods.iter().map(Self::to_observed).collect();

        debug!("Found {} pods", observed.len());
        Ok(observed)
    }

    /// Lists pods belonging to a specific project and environment.
    ///
    /// # Errors
    ///
    /// Returns an error if the API call fails.
    pub async fn list_project_pods(
        &self,
        project: &str,
        environment: &str,
    ) -> Result<Vec<ObservedPod>> {
        info!("Listing pods for project: {project}/{environment}");

        let all_pods = self.list_all_pods().await?;

        let filtered: Vec<ObservedPod> = all_pods
            .into_iter()
            .filter(|p| {
                p.project.as_deref() == Some(project)
                    && p.environment.as_deref() == Some(environment)
            })
            .collect();

        debug!(
            "Found {} pods for {}/{}",
            filtered.len(),
            project,
            environment
        );

        Ok(filtered)
    }

    /// Gets a specific pod by ID.
    ///
    /// # Errors
    ///
    /// Returns an error if the pod is not found or the API call fails.
    pub async fn get_pod(&self, pod_id: &str) -> Result<ObservedPod> {
        debug!("Getting pod: {pod_id}");

        let pod = self.client.get_pod(pod_id).await?;
        Ok(Self::to_observed(&pod))
    }

    /// Finds a pod by its local name within a project.
    ///
    /// # Errors
    ///
    /// Returns an error if the API call fails.
    pub async fn find_pod_by_name(
        &self,
        project: &str,
        environment: &str,
        pod_name: &str,
    ) -> Result<Option<ObservedPod>> {
        let pods = self.list_project_pods(project, environment).await?;

        Ok(pods
            .into_iter()
            .find(|p| p.pod_name.as_deref() == Some(pod_name)))
    }

    /// Converts a `RunPod` Pod to an `ObservedPod`.
    fn to_observed(pod: &Pod) -> ObservedPod {
        let tags = pod.custom_tags.clone().unwrap_or_default();

        let endpoints = pod
            .endpoints()
            .into_iter()
            .map(|e| (e.port, e.url))
            .collect();

        ObservedPod {
            id: pod.id.clone(),
            name: pod.name.clone(),
            project: tags.get(TAG_PROJECT).cloned(),
            environment: tags.get(TAG_ENV).cloned(),
            pod_name: tags.get(TAG_POD).cloned(),
            spec_hash: tags.get(TAG_SPEC_HASH).cloned(),
            status: pod.desired_status,
            gpu_type: pod.gpu_type_name().map(String::from),
            gpu_count: pod.gpu_count,
            image: pod.image_name.clone(),
            endpoints,
            tags,
        }
    }

    /// Checks if a pod with the given spec hash already exists.
    ///
    /// # Errors
    ///
    /// Returns an error if the API call fails.
    pub async fn find_pod_by_spec_hash(
        &self,
        project: &str,
        environment: &str,
        spec_hash: &str,
    ) -> Result<Option<ObservedPod>> {
        let pods = self.list_project_pods(project, environment).await?;

        Ok(pods
            .into_iter()
            .find(|p| p.spec_hash.as_deref() == Some(spec_hash)))
    }

    /// Gets the current status summary for a project.
    ///
    /// # Errors
    ///
    /// Returns an error if the API call fails.
    pub async fn get_project_status(
        &self,
        project: &str,
        environment: &str,
    ) -> Result<ProjectStatus> {
        let pods = self.list_project_pods(project, environment).await?;

        let mut running = 0;
        let mut stopped = 0;
        let mut error = 0;
        let mut other = 0;

        for pod in &pods {
            match pod.status {
                PodStatus::Running => running += 1,
                PodStatus::Stopped | PodStatus::Exited => stopped += 1,
                PodStatus::Unknown => error += 1,
                _ => other += 1,
            }
        }

        Ok(ProjectStatus {
            project: project.to_string(),
            environment: environment.to_string(),
            total_pods: pods.len(),
            running,
            stopped,
            error,
            other,
            pods,
        })
    }

    /// Gets the underlying client reference.
    #[must_use]
    pub const fn client(&self) -> &RunPodClient {
        &self.client
    }
}

/// Status summary for a project.
#[derive(Debug)]
pub struct ProjectStatus {
    /// Project name.
    pub project: String,
    /// Environment name.
    pub environment: String,
    /// Total number of pods.
    pub total_pods: usize,
    /// Number of running pods.
    pub running: usize,
    /// Number of stopped pods.
    pub stopped: usize,
    /// Number of pods in error state.
    pub error: usize,
    /// Number of pods in other states.
    pub other: usize,
    /// All pods.
    pub pods: Vec<ObservedPod>,
}

impl ProjectStatus {
    /// Returns true if all pods are running.
    #[must_use]
    pub const fn is_healthy(&self) -> bool {
        self.total_pods > 0 && self.running == self.total_pods
    }

    /// Returns true if any pods are in error state.
    #[must_use]
    pub const fn has_errors(&self) -> bool {
        self.error > 0
    }
}

impl ObservedPod {
    /// Returns true if this pod is running.
    #[must_use]
    pub const fn is_running(&self) -> bool {
        matches!(self.status, PodStatus::Running)
    }

    /// Returns true if this pod is managed by Halldyll.
    #[must_use]
    pub const fn is_managed(&self) -> bool {
        self.project.is_some() && self.environment.is_some()
    }

    /// Gets the full qualified name.
    #[must_use]
    pub fn full_name(&self) -> String {
        match (&self.project, &self.environment, &self.pod_name) {
            (Some(proj), Some(env), Some(name)) => format!("{proj}-{env}-{name}"),
            _ => self.name.clone(),
        }
    }
}
