//! `RunPod` API client implementation.
//!
//! This module provides the HTTP client for interacting with the `RunPod` GraphQL API.

use reqwest::{header, Client};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{debug, trace};

use crate::error::{HalldyllError, Result, RunPodError};

use super::types::{CreatePodRequest, GpuType, Pod, UpdatePodRequest};

/// `RunPod` API base URL.
const RUNPOD_API_URL: &str = "https://api.runpod.io/graphql";

/// Default request timeout in seconds.
const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Maximum number of retries for transient failures.
const MAX_RETRIES: u32 = 3;

/// Delay between retries in milliseconds.
const RETRY_DELAY_MS: u64 = 1000;

/// `RunPod` API client.
#[derive(Debug, Clone)]
pub struct RunPodClient {
    /// HTTP client.
    client: Client,
    /// API key.
    api_key: String,
}

/// GraphQL request structure.
#[derive(Debug, Serialize)]
struct GraphQLRequest {
    query: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    variables: Option<serde_json::Value>,
}

/// GraphQL response structure.
#[derive(Debug, Deserialize)]
struct GraphQLResponse<T> {
    data: Option<T>,
    errors: Option<Vec<GraphQLError>>,
}

/// GraphQL error structure.
#[derive(Debug, Deserialize)]
struct GraphQLError {
    message: String,
}

impl RunPodClient {
    /// Creates a new `RunPod` API client.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be created.
    pub fn new(api_key: &str) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .build()
            .map_err(|e| RunPodError::network(format!("Failed to create HTTP client: {e}")))?;

        Ok(Self {
            client,
            api_key: api_key.to_string(),
        })
    }

    /// Creates a client with a custom timeout.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be created.
    pub fn with_timeout(api_key: &str, timeout_secs: u64) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .map_err(|e| RunPodError::network(format!("Failed to create HTTP client: {e}")))?;

        Ok(Self {
            client,
            api_key: api_key.to_string(),
        })
    }

    /// Executes a GraphQL query.
    async fn execute<T: for<'de> Deserialize<'de>>(
        &self,
        query: &str,
        variables: Option<serde_json::Value>,
    ) -> Result<T> {
        let request = GraphQLRequest {
            query: query.to_string(),
            variables,
        };

        let mut last_error = None;

        for attempt in 0..MAX_RETRIES {
            if attempt > 0 {
                debug!("Retry attempt {attempt} of {MAX_RETRIES}");
                tokio::time::sleep(Duration::from_millis(RETRY_DELAY_MS * u64::from(attempt)))
                    .await;
            }

            match self.execute_once::<T>(&request).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    if e.is_retryable() {
                        last_error = Some(e);
                        continue;
                    }
                    return Err(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            HalldyllError::RunPod(RunPodError::NetworkError {
                message: String::from("Max retries exceeded"),
            })
        }))
    }

    /// Executes a single GraphQL request.
    async fn execute_once<T: for<'de> Deserialize<'de>>(
        &self,
        request: &GraphQLRequest,
    ) -> Result<T> {
        trace!("Executing GraphQL query: {}", request.query);

        let response = self
            .client
            .post(RUNPOD_API_URL)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {}", self.api_key))
            .json(request)
            .send()
            .await
            .map_err(|e| {
                HalldyllError::RunPod(RunPodError::NetworkError {
                    message: format!("Request failed: {e}"),
                })
            })?;

        let status = response.status();

        if status.as_u16() == 429 {
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse().ok())
                .unwrap_or_default();
            let retry_after = if retry_after == 0 { 60 } else { retry_after };

            return Err(HalldyllError::RunPod(RunPodError::RateLimited {
                retry_after_secs: retry_after,
            }));
        }

        if status.as_u16() == 401 || status.as_u16() == 403 {
            return Err(HalldyllError::RunPod(RunPodError::AuthenticationFailed {
                message: String::from("Invalid API key"),
            }));
        }

        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(HalldyllError::RunPod(RunPodError::api_error(
                status.as_u16(),
                body,
            )));
        }

        let gql_response: GraphQLResponse<T> = response.json().await.map_err(|e| {
            HalldyllError::RunPod(RunPodError::InvalidResponse {
                message: format!("Failed to parse response: {e}"),
            })
        })?;

        if let Some(errors) = gql_response.errors.filter(|e| !e.is_empty()) {
            let message = errors
                .iter()
                .map(|e| e.message.as_str())
                .collect::<Vec<_>>()
                .join("; ");
            return Err(HalldyllError::RunPod(RunPodError::api_error(400, message)));
        }

        gql_response.data.ok_or_else(|| {
            HalldyllError::RunPod(RunPodError::InvalidResponse {
                message: String::from("No data in response"),
            })
        })
    }

    /// Lists all pods.
    ///
    /// # Errors
    ///
    /// Returns an error if the API call fails.
    pub async fn list_pods(&self) -> Result<Vec<Pod>> {
        #[derive(Deserialize)]
        struct Response {
            myself: MyselfResponse,
        }
        #[derive(Deserialize)]
        struct MyselfResponse {
            pods: Vec<Pod>,
        }

        let query = r"
            query {
                myself {
                    pods {
                        id
                        name
                        desiredStatus
                        imageName
                        gpuCount
                        volumeInGb
                        containerDiskInGb
                        memoryInGb
                        vcpuCount
                        ports
                        machine {
                            gpuTypeId
                        }
                        runtime {
                            ports {
                                ip
                                privatePort
                                publicPort
                                type
                            }
                            gpus {
                                id
                                gpuUtilizationPercent
                                memoryUtilizationPercent
                            }
                            uptimeInSeconds
                        }
                        env {
                            key
                            value
                        }
                    }
                }
            }
        ";

        let response: Response = self.execute(query, None).await?;
        Ok(response.myself.pods)
    }

    /// Gets a pod by ID.
    ///
    /// # Errors
    ///
    /// Returns an error if the pod is not found or the API call fails.
    pub async fn get_pod(&self, pod_id: &str) -> Result<Pod> {
        #[derive(Deserialize)]
        struct Response {
            pod: Option<Pod>,
        }

        let query = r"
            query Pod($podId: String!) {
                pod(input: { podId: $podId }) {
                    id
                    name
                    desiredStatus
                    imageName
                    gpuCount
                    volumeInGb
                    containerDiskInGb
                    memoryInGb
                    vcpuCount
                    ports
                    machine {
                        gpuTypeId
                    }
                    runtime {
                        ports {
                            ip
                            privatePort
                            publicPort
                            type
                        }
                        gpus {
                            id
                            gpuUtilizationPercent
                            memoryUtilizationPercent
                        }
                        uptimeInSeconds
                    }
                    env {
                        key
                        value
                    }
                }
            }
        ";

        let variables = serde_json::json!({ "podId": pod_id });
        let response: Response = self.execute(query, Some(variables)).await?;

        response.pod.ok_or_else(|| {
            HalldyllError::RunPod(RunPodError::PodNotFound {
                pod_id: pod_id.to_string(),
            })
        })
    }

    /// Creates a new pod.
    ///
    /// # Errors
    ///
    /// Returns an error if the pod cannot be created.
    pub async fn create_pod(&self, request: &CreatePodRequest) -> Result<Pod> {
        #[derive(Deserialize)]
        struct Response {
            #[serde(rename = "podFindAndDeployOnDemand")]
            pod: Pod,
        }

        let query = r"
            mutation CreatePod($input: PodFindAndDeployOnDemandInput!) {
                podFindAndDeployOnDemand(input: $input) {
                    id
                    name
                    desiredStatus
                    imageName
                    gpuCount
                    volumeInGb
                    containerDiskInGb
                    memoryInGb
                    vcpuCount
                    ports
                    machine {
                        gpuTypeId
                    }
                    env {
                        key
                        value
                    }
                }
            }
        ";

        let input = serde_json::json!({
            "cloudType": request.cloud_type,
            "gpuTypeId": request.gpu_type_id,
            "gpuCount": request.gpu_count,
            "name": request.name,
            "imageName": request.image_name,
            "volumeInGb": request.volume_in_gb,
            "containerDiskInGb": request.container_disk_in_gb,
            "volumeMountPath": request.volume_mount_path,
            "ports": request.ports,
            "env": request.env.iter().map(|e| {
                serde_json::json!({ "key": e.key, "value": e.value })
            }).collect::<Vec<_>>(),
            "dockerArgs": request.docker_args,
            "dataCenterId": request.data_center_id,
        });

        let variables = serde_json::json!({ "input": input });
        let response: Response = self.execute(query, Some(variables)).await?;

        Ok(response.pod)
    }

    /// Stops a pod.
    ///
    /// # Errors
    ///
    /// Returns an error if the pod cannot be stopped.
    pub async fn stop_pod(&self, pod_id: &str) -> Result<()> {
        #[derive(Deserialize)]
        struct Response {
            #[serde(rename = "podStop")]
            _pod: Option<serde_json::Value>,
        }

        let query = r"
            mutation StopPod($podId: String!) {
                podStop(input: { podId: $podId }) {
                    id
                }
            }
        ";

        let variables = serde_json::json!({ "podId": pod_id });
        let _: Response = self.execute(query, Some(variables)).await?;

        Ok(())
    }

    /// Resumes a stopped pod.
    ///
    /// # Errors
    ///
    /// Returns an error if the pod cannot be resumed.
    pub async fn resume_pod(&self, pod_id: &str) -> Result<Pod> {
        #[derive(Deserialize)]
        struct Response {
            #[serde(rename = "podResume")]
            pod: Pod,
        }

        let query = r"
            mutation ResumePod($podId: String!) {
                podResume(input: { podId: $podId }) {
                    id
                    name
                    desiredStatus
                    imageName
                    gpuCount
                }
            }
        ";

        let variables = serde_json::json!({ "podId": pod_id });
        let response: Response = self.execute(query, Some(variables)).await?;

        Ok(response.pod)
    }

    /// Terminates (deletes) a pod.
    ///
    /// # Errors
    ///
    /// Returns an error if the pod cannot be terminated.
    pub async fn terminate_pod(&self, pod_id: &str) -> Result<()> {
        #[derive(Deserialize)]
        struct Response {
            #[serde(rename = "podTerminate")]
            _result: Option<serde_json::Value>,
        }

        let query = r"
            mutation TerminatePod($podId: String!) {
                podTerminate(input: { podId: $podId })
            }
        ";

        let variables = serde_json::json!({ "podId": pod_id });
        let _: Response = self.execute(query, Some(variables)).await?;

        Ok(())
    }

    /// Updates a pod's configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the pod cannot be updated.
    pub async fn update_pod(&self, request: &UpdatePodRequest) -> Result<Pod> {
        #[derive(Deserialize)]
        struct Response {
            #[serde(rename = "podEditJob")]
            pod: Pod,
        }

        let query = r"
            mutation UpdatePod($input: PodEditJobInput!) {
                podEditJob(input: $input) {
                    id
                    name
                    desiredStatus
                    imageName
                }
            }
        ";

        let mut input = serde_json::json!({ "podId": request.pod_id });

        if let Some(image) = &request.image_name {
            input["imageName"] = serde_json::json!(image);
        }

        if let Some(env) = &request.env {
            input["env"] = serde_json::json!(env
                .iter()
                .map(|e| serde_json::json!({ "key": e.key, "value": e.value }))
                .collect::<Vec<_>>());
        }

        let variables = serde_json::json!({ "input": input });
        let response: Response = self.execute(query, Some(variables)).await?;

        Ok(response.pod)
    }

    /// Gets available GPU types.
    ///
    /// # Errors
    ///
    /// Returns an error if the API call fails.
    pub async fn list_gpu_types(&self) -> Result<Vec<GpuType>> {
        #[derive(Deserialize)]
        struct Response {
            #[serde(rename = "gpuTypes")]
            gpu_types: Vec<GpuType>,
        }

        let query = r"
            query {
                gpuTypes {
                    id
                    displayName
                    memoryInGb
                    secureCloud
                    communityCloud
                    securePrice
                    communityPrice
                }
            }
        ";

        let response: Response = self.execute(query, None).await?;
        Ok(response.gpu_types)
    }

    /// Checks if a GPU type is available.
    ///
    /// # Errors
    ///
    /// Returns an error if the API call fails.
    pub async fn is_gpu_available(&self, gpu_type_id: &str, cloud_type: &str) -> Result<bool> {
        let gpu_types = self.list_gpu_types().await?;

        for gpu in gpu_types {
            if gpu.id == gpu_type_id || gpu.display_name == gpu_type_id {
                return Ok(match cloud_type {
                    "SECURE" => gpu.secure_cloud,
                    "COMMUNITY" => gpu.community_cloud,
                    _ => false,
                });
            }
        }

        Ok(false)
    }

    /// Validates the API key by making a test request.
    ///
    /// # Errors
    ///
    /// Returns an error if the API key is invalid.
    pub async fn validate_api_key(&self) -> Result<bool> {
        #[derive(Deserialize)]
        struct Response {
            #[serde(rename = "myself")]
            _myself: MyselfResponse,
        }
        #[derive(Deserialize)]
        struct MyselfResponse {
            #[serde(rename = "id")]
            _id: String,
        }

        let query = r"
            query {
                myself {
                    id
                }
            }
        ";

        match self.execute::<Response>(query, None).await {
            Ok(_) => Ok(true),
            Err(HalldyllError::RunPod(RunPodError::AuthenticationFailed { .. })) => Ok(false),
            Err(e) => Err(e),
        }
    }
}
