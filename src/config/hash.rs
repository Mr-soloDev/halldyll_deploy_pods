//! Configuration hashing for change detection.
//!
//! This module provides deterministic hashing of configuration structures
//! to detect changes between deployments and enable idempotent operations.

use sha2::{Digest, Sha256};

use super::spec::{DeployConfig, PodConfig};

/// Hasher for computing configuration hashes.
#[derive(Debug, Default)]
pub struct ConfigHasher;

impl ConfigHasher {
    /// Creates a new configuration hasher.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Computes a hash of the entire deployment configuration.
    ///
    /// This hash changes when any part of the configuration changes.
    #[must_use]
    pub fn hash_config(&self, config: &DeployConfig) -> String {
        let mut hasher = Sha256::new();

        // Hash project info
        hasher.update(config.project.name.as_bytes());
        hasher.update(config.project.environment.as_bytes());
        if let Some(region) = &config.project.region {
            hasher.update(region.as_bytes());
        }

        // Hash each pod
        for pod in &config.pods {
            hasher.update(self.hash_pod(pod).as_bytes());
        }

        // Hash guardrails if present
        if let Some(guardrails) = &config.guardrails {
            if let Some(max_cost) = guardrails.max_hourly_cost {
                hasher.update(max_cost.to_be_bytes());
            }
            if let Some(max_gpus) = guardrails.max_gpus {
                hasher.update(max_gpus.to_be_bytes());
            }
        }

        hex::encode(hasher.finalize())
    }

    /// Computes a hash for a single pod configuration.
    ///
    /// This hash is used to detect changes to individual pods.
    #[must_use]
    pub fn hash_pod(&self, pod: &PodConfig) -> String {
        let mut hasher = Sha256::new();

        // Pod identity
        hasher.update(pod.name.as_bytes());

        // GPU config
        hasher.update(pod.gpu.gpu_type.as_bytes());
        hasher.update(pod.gpu.count.to_be_bytes());
        if let Some(vram) = pod.gpu.min_vram_gb {
            hasher.update(vram.to_be_bytes());
        }
        for fallback in &pod.gpu.fallback {
            hasher.update(fallback.as_bytes());
        }

        // Ports (sorted for determinism)
        let mut ports: Vec<_> = pod.ports.iter().map(|p| p.port).collect();
        ports.sort_unstable();
        for port in ports {
            hasher.update(port.to_be_bytes());
        }

        // Volumes (sorted by name for determinism)
        let mut volumes: Vec<_> = pod.volumes.iter().collect();
        volumes.sort_by(|a, b| a.name.cmp(&b.name));
        for volume in volumes {
            hasher.update(volume.name.as_bytes());
            hasher.update(volume.mount.as_bytes());
            hasher.update(if volume.persistent { [1u8] } else { [0u8] });
            if let Some(size) = volume.size_gb {
                hasher.update(size.to_be_bytes());
            }
        }

        // Runtime
        hasher.update(pod.runtime.image.as_bytes());

        // Environment variables (sorted for determinism)
        let mut env_vars: Vec<_> = pod.runtime.env.iter().collect();
        env_vars.sort_by(|a, b| a.0.cmp(b.0));
        for (key, value) in env_vars {
            hasher.update(key.as_bytes());
            hasher.update(value.as_bytes());
        }

        if let Some(cmd) = &pod.runtime.command {
            for arg in cmd {
                hasher.update(arg.as_bytes());
            }
        }

        if let Some(args) = &pod.runtime.args {
            for arg in args {
                hasher.update(arg.as_bytes());
            }
        }

        // Models (sorted by ID for determinism)
        let mut models: Vec<_> = pod.models.iter().collect();
        models.sort_by(|a, b| a.id.cmp(&b.id));
        for model in models {
            hasher.update(model.id.as_bytes());
            if let Some(repo) = &model.repo {
                hasher.update(repo.as_bytes());
            }
            if let Some(load) = &model.load {
                hasher.update(load.engine.as_bytes());
                if let Some(quant) = &load.quant {
                    hasher.update(quant.as_bytes());
                }
                if let Some(seq_len) = load.max_seq_len {
                    hasher.update(seq_len.to_be_bytes());
                }
            }
        }

        // Tags (sorted for determinism)
        let mut tags: Vec<_> = pod.tags.iter().collect();
        tags.sort_by(|a, b| a.0.cmp(b.0));
        for (key, value) in tags {
            hasher.update(key.as_bytes());
            hasher.update(value.as_bytes());
        }

        hex::encode(hasher.finalize())
    }

    /// Computes a short hash (first 8 characters) for display purposes.
    #[must_use]
    pub fn short_hash(&self, hash: &str) -> String {
        hash.chars().take(8).collect()
    }

    /// Compares two hashes to determine if they are equal.
    #[must_use]
    pub fn hashes_match(hash1: &str, hash2: &str) -> bool {
        // Use constant-time comparison to avoid timing attacks
        if hash1.len() != hash2.len() {
            return false;
        }

        hash1
            .bytes()
            .zip(hash2.bytes())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::spec::{GpuConfig, RuntimeConfig};
    use std::collections::HashMap;

    fn create_test_pod(name: &str) -> PodConfig {
        PodConfig {
            name: name.to_string(),
            gpu: GpuConfig {
                gpu_type: String::from("NVIDIA A40"),
                count: 1,
                min_vram_gb: None,
                fallback: vec![],
            },
            ports: vec![],
            volumes: vec![],
            runtime: RuntimeConfig {
                image: String::from("test:latest"),
                env: HashMap::new(),
                command: None,
                args: None,
            },
            models: vec![],
            health_check: None,
            tags: HashMap::new(),
        }
    }

    #[test]
    fn test_pod_hash_deterministic() {
        let hasher = ConfigHasher::new();
        let pod = create_test_pod("test-pod");

        let hash1 = hasher.hash_pod(&pod);
        let hash2 = hasher.hash_pod(&pod);

        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_different_pods_different_hash() {
        let hasher = ConfigHasher::new();
        let pod1 = create_test_pod("pod-1");
        let pod2 = create_test_pod("pod-2");

        let hash1 = hasher.hash_pod(&pod1);
        let hash2 = hasher.hash_pod(&pod2);

        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_short_hash() {
        let hasher = ConfigHasher::new();
        let full_hash = "abcdef1234567890abcdef1234567890";
        let short = hasher.short_hash(full_hash);

        assert_eq!(short, "abcdef12");
        assert_eq!(short.len(), 8);
    }

    #[test]
    fn test_hashes_match() {
        assert!(ConfigHasher::hashes_match("abc123", "abc123"));
        assert!(!ConfigHasher::hashes_match("abc123", "abc124"));
        assert!(!ConfigHasher::hashes_match("abc123", "abc12"));
    }
}
