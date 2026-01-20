//! Health checking for `RunPod` pods.
//!
//! This module provides health checking functionality for pods,
//! including HTTP endpoint checks and service availability monitoring.

use reqwest::Client;
use std::time::Duration;
use tracing::{debug, warn};

use crate::config::HealthCheckConfig;
use crate::error::Result;

use super::observer::ObservedPod;

/// Default health check timeout in seconds.
const DEFAULT_TIMEOUT_SECS: u64 = 10;

/// Default connection timeout in seconds.
const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 5;

/// Health status for a pod.
#[derive(Debug, Clone)]
pub struct HealthStatus {
    /// Pod ID.
    pub pod_id: String,
    /// Pod name.
    pub pod_name: String,
    /// Overall health status.
    pub healthy: bool,
    /// Individual endpoint checks.
    pub checks: Vec<EndpointCheck>,
    /// Optional error message.
    pub error: Option<String>,
}

/// Result of a single endpoint health check.
#[derive(Debug, Clone)]
pub struct EndpointCheck {
    /// Port number.
    pub port: u16,
    /// Endpoint URL.
    pub url: String,
    /// Whether the check passed.
    pub healthy: bool,
    /// HTTP status code (if applicable).
    pub status_code: Option<u16>,
    /// Response time in milliseconds.
    pub response_time_ms: Option<u64>,
    /// Error message (if any).
    pub error: Option<String>,
}

/// Health checker for pod services.
#[derive(Debug)]
pub struct HealthChecker {
    /// HTTP client for health checks.
    client: Client,
    /// Default health check configuration.
    default_config: HealthCheckConfig,
}

impl HealthChecker {
    /// Creates a new health checker.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be created.
    pub fn new() -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .connect_timeout(Duration::from_secs(DEFAULT_CONNECT_TIMEOUT_SECS))
            .build()
            .map_err(|e| crate::error::HalldyllError::internal(format!("Failed to create HTTP client: {e}")))?;

        Ok(Self {
            client,
            default_config: HealthCheckConfig {
                endpoint: String::from("/health"),
                port: 8000,
                interval_secs: 30,
                timeout_secs: 5,
                failure_threshold: 3,
            },
        })
    }

    /// Creates a health checker with a custom configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be created.
    pub fn with_config(config: HealthCheckConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(u64::from(config.timeout_secs)))
            .connect_timeout(Duration::from_secs(DEFAULT_CONNECT_TIMEOUT_SECS))
            .build()
            .map_err(|e| crate::error::HalldyllError::internal(format!("Failed to create HTTP client: {e}")))?;

        Ok(Self {
            client,
            default_config: config,
        })
    }

    /// Checks the health of a pod.
    pub async fn check_pod(&self, pod: &ObservedPod, config: Option<&HealthCheckConfig>) -> HealthStatus {
        let config = config.map_or(&self.default_config, |c| c);

        debug!("Checking health of pod: {}", pod.id);

        let mut checks = Vec::new();
        let mut all_healthy = true;

        // Check each endpoint
        for (port, url) in &pod.endpoints {
            let check_url = format!("{url}{}", config.endpoint);
            let check = self.check_endpoint(*port, &check_url).await;

            if !check.healthy {
                all_healthy = false;
            }

            checks.push(check);
        }

        // If no endpoints, check if pod is running
        if checks.is_empty() {
            all_healthy = pod.is_running();
        }

        HealthStatus {
            pod_id: pod.id.clone(),
            pod_name: pod.name.clone(),
            healthy: all_healthy && pod.is_running(),
            checks,
            error: if pod.is_running() {
                None
            } else {
                Some(format!("Pod is not running: {}", pod.status))
            },
        }
    }

    /// Checks a single HTTP endpoint.
    async fn check_endpoint(&self, port: u16, url: &str) -> EndpointCheck {
        let start = std::time::Instant::now();

        match self.client.get(url).send().await {
            Ok(response) => {
                let status = response.status();
                let response_time = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);

                let healthy = status.is_success();

                if !healthy {
                    debug!("Endpoint {url} returned status {status}");
                }

                EndpointCheck {
                    port,
                    url: url.to_string(),
                    healthy,
                    status_code: Some(status.as_u16()),
                    response_time_ms: Some(response_time),
                    error: if healthy {
                        None
                    } else {
                        Some(format!("HTTP {status}"))
                    },
                }
            }
            Err(e) => {
                warn!("Health check failed for {url}: {e}");

                EndpointCheck {
                    port,
                    url: url.to_string(),
                    healthy: false,
                    status_code: None,
                    response_time_ms: None,
                    error: Some(e.to_string()),
                }
            }
        }
    }

    /// Checks health of multiple pods.
    pub async fn check_pods(&self, pods: &[ObservedPod]) -> Vec<HealthStatus> {
        let mut results = Vec::with_capacity(pods.len());

        for pod in pods {
            results.push(self.check_pod(pod, None).await);
        }

        results
    }

    /// Waits for a pod to become healthy.
    ///
    /// # Errors
    ///
    /// Returns an error if the timeout is reached.
    pub async fn wait_for_healthy(
        &self,
        pod: &ObservedPod,
        config: Option<&HealthCheckConfig>,
        timeout_secs: u64,
    ) -> Result<HealthStatus> {
        let start = std::time::Instant::now();
        let timeout = Duration::from_secs(timeout_secs);
        let interval_secs = config.map_or(self.default_config.interval_secs, |c| c.interval_secs);
        let check_interval = Duration::from_secs(u64::from(interval_secs));

        loop {
            let status = self.check_pod(pod, config).await;

            if status.healthy {
                return Ok(status);
            }

            if start.elapsed() > timeout {
                return Err(crate::error::HalldyllError::RunPod(
                    crate::error::RunPodError::Timeout {
                        pod_id: pod.id.clone(),
                        expected_state: String::from("healthy"),
                    },
                ));
            }

            tokio::time::sleep(check_interval).await;
        }
    }
}

impl HealthStatus {
    /// Returns true if all endpoints are healthy.
    #[must_use]
    pub fn all_endpoints_healthy(&self) -> bool {
        self.checks.iter().all(|c| c.healthy)
    }

    /// Returns the number of healthy endpoints.
    #[must_use]
    pub fn healthy_endpoint_count(&self) -> usize {
        self.checks.iter().filter(|c| c.healthy).count()
    }

    /// Returns the average response time in milliseconds.
    #[must_use]
    pub fn average_response_time_ms(&self) -> Option<u64> {
        let times: Vec<u64> = self
            .checks
            .iter()
            .filter_map(|c| c.response_time_ms)
            .collect();

        if times.is_empty() {
            None
        } else {
            Some(times.iter().sum::<u64>() / times.len() as u64)
        }
    }
}

impl std::fmt::Display for HealthStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let status = if self.healthy { "healthy" } else { "unhealthy" };
        write!(f, "{}: {status}", self.pod_name)?;

        if !self.checks.is_empty() {
            write!(
                f,
                " ({}/{} endpoints)",
                self.healthy_endpoint_count(),
                self.checks.len()
            )?;
        }

        if let Some(error) = &self.error {
            write!(f, " - {error}")?;
        }

        Ok(())
    }
}
