# Halldyll Deploy Pods

[![Crates.io](https://img.shields.io/crates/v/halldyll_deploy_pods.svg)](https://crates.io/crates/halldyll_deploy_pods)
[![Documentation](https://docs.rs/halldyll_deploy_pods/badge.svg)](https://docs.rs/halldyll_deploy_pods)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/rust-1.85+-orange.svg)](https://www.rust-lang.org)

**A declarative, idempotent, and reconcilable deployment system for [RunPod](https://runpod.io) GPU pods.**

Think of it as **Terraform/Kubernetes for RunPod** — define your GPU infrastructure as code, and let Halldyll handle the rest.

## Features

- **Declarative** — Define your infrastructure in a simple YAML file
- **Idempotent** — Run `apply` multiple times, get the same result
- **Drift Detection** — Automatically detect and fix configuration drift
- **Reconciliation Loop** — Continuously converge to desired state
- **State Management** — Track deployments locally or on S3
- **Multi-environment** — Support for dev, staging, prod environments
- **Guardrails** — Cost limits, GPU limits, TTL auto-stop
- **Auto Model Download** — Automatically download HuggingFace models on pod startup
- **Inference Engines** — Auto-start vLLM, TGI, or Ollama with your models

## Installation

### CLI Tool (From Crates.io)

```bash
cargo install halldyll_deploy_pods
```

### As a Rust Library

Add to your `Cargo.toml`:

```toml
[dependencies]
halldyll_deploy_pods = "0.1.0"
```

### From Source

```bash
git clone https://github.com/Mr-soloDev/halldyll_deploy_pods.git
cd halldyll_deploy_pods
cargo install --path .
```

## Quick Start

### 1. Initialize a new project

```bash
halldyll init my-project
cd my-project
```

### 2. Configure your deployment

Edit `halldyll.deploy.yaml`:

```yaml
project:
  name: "my-ml-stack"
  environment: "prod"
  cloud_type: SECURE

state:
  backend: local

pods:
  - name: "inference"
    gpu:
      type: "NVIDIA A40"
      count: 1
    runtime:
      image: "vllm/vllm-openai:latest"
      env:
        MODEL_NAME: "meta-llama/Llama-3-8B"
    ports:
      - "8000/http"
    volumes:
      - name: "hf-cache"
        mount: "/root/.cache/huggingface"
        persistent: true
```

### 3. Set your RunPod API key

```bash
export RUNPOD_API_KEY="your-api-key"
```

### 4. Deploy!

```bash
halldyll plan      # Preview changes
halldyll apply     # Deploy to RunPod
halldyll status    # Check deployment status
```

## Commands

| Command | Description |
|---------|-------------|
| `halldyll init [path]` | Initialize a new project |
| `halldyll validate` | Validate configuration file |
| `halldyll plan` | Show deployment plan (dry-run) |
| `halldyll apply` | Apply the deployment plan |
| `halldyll status` | Show current deployment status |
| `halldyll reconcile` | Auto-fix drift from desired state |
| `halldyll drift` | Detect configuration drift |
| `halldyll destroy` | Destroy all deployed resources |
| `halldyll logs <pod>` | View pod logs |
| `halldyll state` | Manage deployment state |

## Configuration Reference

### Project Configuration

```yaml
project:
  name: "my-project"          # Required: unique project name
  environment: "dev"          # Optional: dev, staging, prod (default: dev)
  region: "EU"                # Optional: EU, US, etc.
  cloud_type: SECURE          # Optional: SECURE or COMMUNITY
  compute_type: GPU           # Optional: GPU or CPU
```

### State Backend

```yaml
state:
  backend: local              # local or s3
  # For S3:
  bucket: "my-state-bucket"
  prefix: "halldyll/my-project"
  region: "us-east-1"
```

### Pod Configuration

```yaml
pods:
  - name: "my-pod"
    gpu:
      type: "NVIDIA A40"      # GPU type
      count: 1                # Number of GPUs
      min_vram_gb: 40         # Optional: minimum VRAM
      fallback:               # Optional: fallback GPU types
        - "NVIDIA L40S"
        - "NVIDIA RTX A6000"
    
    ports:
      - "22/tcp"              # SSH
      - "8000/http"           # HTTP endpoint
    
    volumes:
      - name: "data"
        mount: "/data"
        persistent: true
        size_gb: 100
    
    runtime:
      image: "runpod/pytorch:2.1.0-py3.10-cuda11.8.0"
      env:
        MY_VAR: "value"
    
    health_check:
      endpoint: "/health"
      port: 8000
      interval_secs: 30
      timeout_secs: 5
```

### Model Configuration (Auto-download and Start)

```yaml
pods:
  - name: "llm-server"
    gpu:
      type: "NVIDIA A40"
      count: 1
    runtime:
      image: "vllm/vllm-openai:latest"
    ports:
      - "8000/http"
    
    # Models are automatically downloaded and engines started
    models:
      - id: "llama-3-8b"
        provider: huggingface           # huggingface, bundle, or custom
        repo: "meta-llama/Meta-Llama-3-8B-Instruct"
        load:
          engine: vllm                  # vllm, tgi, ollama, or transformers
          quant: awq                    # Optional: awq, gptq, fp8
          max_seq_len: 8192             # Optional: max sequence length
          options:                      # Optional: engine-specific options
            tensor-parallel-size: 1
```

### Supported Inference Engines

| Engine | Description | Auto-Start | Use Case |
|--------|-------------|------------|----------|
| `vllm` | High-performance LLM serving | Yes | Production LLM APIs, OpenAI-compatible |
| `tgi` | HuggingFace Text Generation Inference | Yes | HuggingFace models, streaming |
| `ollama` | Easy-to-use LLM runner | Yes | Local development, quick testing |
| `transformers` | HuggingFace Transformers library | No | Custom scripts, fine-tuning |

### Multi-Model Deployment Example

Deploy different models on different pods:

```yaml
pods:
  # LLM API Server
  - name: "llm-api"
    gpu:
      type: "NVIDIA A40"
      count: 1
    runtime:
      image: "vllm/vllm-openai:latest"
    ports:
      - "8000/http"
    models:
      - id: "llama-3-8b"
        provider: huggingface
        repo: "meta-llama/Meta-Llama-3-8B-Instruct"
        load:
          engine: vllm
          max_seq_len: 8192

  # Embedding Server
  - name: "embeddings"
    gpu:
      type: "NVIDIA RTX 4090"
      count: 1
    runtime:
      image: "ghcr.io/huggingface/text-embeddings-inference:latest"
    ports:
      - "8080/http"
    models:
      - id: "bge-large"
        provider: huggingface
        repo: "BAAI/bge-large-en-v1.5"
        load:
          engine: tgi

  # Vision Model
  - name: "vision-api"
    gpu:
      type: "NVIDIA A40"
      count: 1
    runtime:
      image: "ghcr.io/huggingface/text-generation-inference:latest"
    ports:
      - "8000/http"
    models:
      - id: "llava"
        provider: huggingface
        repo: "llava-hf/llava-v1.6-mistral-7b-hf"
        load:
          engine: tgi
```

### Quantization Options

Reduce memory usage with quantization:

```yaml
models:
  - id: "llama-70b-awq"
    provider: huggingface
    repo: "TheBloke/Llama-2-70B-Chat-AWQ"
    load:
      engine: vllm
      quant: awq              # 4-bit AWQ quantization
      max_seq_len: 4096
```

| Quant Method | Memory Reduction | Quality | Speed |
|--------------|------------------|---------|-------|
| `awq` | ~75% | High | Fast |
| `gptq` | ~75% | High | Medium |
| `fp8` | ~50% | Very High | Fast |

### Guardrails (Optional)

```yaml
guardrails:
  max_hourly_cost: 10.0       # Maximum hourly cost in USD
  max_gpus: 4                 # Maximum total GPUs
  ttl_hours: 24               # Auto-stop after N hours
  allow_gpu_fallback: false   # Allow fallback to other GPU types
```

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│              halldyll.deploy.yaml                       │
│                 (Desired State)                         │
└───────────────────────┬─────────────────────────────────┘
                        │
                        ▼
┌─────────────────────────────────────────────────────────┐
│            ConfigParser + Validator                     │
└───────────────────────┬─────────────────────────────────┘
                        │
         ┌──────────────┴──────────────┐
         ▼                             ▼
┌─────────────────┐          ┌─────────────────┐
│   StateStore    │          │  PodObserver    │
│ (Local or S3)   │          │ (RunPod API)    │
└────────┬────────┘          └────────┬────────┘
         │                            │
         └──────────────┬─────────────┘
                        ▼
┌─────────────────────────────────────────────────────────┐
│                    DiffEngine                           │
│           (Compare Desired vs Observed)                 │
└───────────────────────┬─────────────────────────────────┘
                        ▼
┌─────────────────────────────────────────────────────────┐
│                  Reconciler                             │
│        (Execute Plan → Converge State)                  │
└─────────────────────────────────────────────────────────┘
```

## Library Usage

You can use Halldyll as a Rust library in your projects:

### Add to Cargo.toml

```toml
[dependencies]
halldyll_deploy_pods = "0.1.0"
tokio = { version = "1", features = ["full"] }
```

### Basic Example: Parse and Validate Config

```rust
use halldyll_deploy_pods::{ConfigParser, ConfigValidator};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse configuration from YAML file
    let config = ConfigParser::parse_file("halldyll.deploy.yaml")?;
    
    // Validate the configuration
    ConfigValidator::validate(&config)?;
    
    println!("Project: {}", config.project.name);
    println!("Pods: {}", config.pods.len());
    
    Ok(())
}
```

### Full Example: Deploy Pods with Model Setup

```rust
use halldyll_deploy_pods::{
    ConfigParser, ConfigValidator,
    RunPodClient, PodProvisioner, PodObserver, PodExecutor,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse and validate configuration
    let config = ConfigParser::parse_file("halldyll.deploy.yaml")?;
    ConfigValidator::validate(&config)?;
    
    // Create RunPod client
    let api_key = std::env::var("RUNPOD_API_KEY")?;
    let client = RunPodClient::new(&api_key)?;
    
    // Create provisioner
    let mut provisioner = PodProvisioner::new(client.clone());
    provisioner.init_gpu_types().await?;
    
    // Deploy pod with automatic model download and engine startup
    let (pod, setup_result) = provisioner.create_pod_with_setup(
        &config.pods[0],
        &config.project,
        "config-hash"
    ).await?;
    
    println!("Pod created: {} (ID: {})", pod.name, pod.id);
    
    // Check model setup results
    if let Some(result) = setup_result {
        println!("Setup: {}", result.summary());
        for model in &result.model_results {
            println!("  Model '{}': {}", model.model_id, 
                if model.success { "OK" } else { "FAILED" });
        }
    }
    
    Ok(())
}
```

### Example: Execute Commands on a Pod

```rust
use halldyll_deploy_pods::{RunPodClient, PodExecutor};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = RunPodClient::new(&std::env::var("RUNPOD_API_KEY")?)?;
    let executor = PodExecutor::new(client);
    
    // Execute a command on a running pod
    let result = executor.execute_command(
        "pod-id-here",
        "nvidia-smi",
        Some(30)
    ).await?;
    
    println!("Output: {}", result.stdout);
    
    Ok(())
}
```

### Available Types

| Type | Description |
|------|-------------|
| `ConfigParser` | Parse YAML configuration files |
| `ConfigValidator` | Validate configuration |
| `DeployConfig` | Configuration struct |
| `RunPodClient` | RunPod API client |
| `PodProvisioner` | Create and manage pods |
| `PodObserver` | Observe pod states |
| `PodExecutor` | Execute commands on pods |
| `Reconciler` | Reconcile desired vs actual state |
| `LocalStateStore` | Local state storage |
| `S3StateStore` | S3 state storage |

## Environment Variables

| Variable | Description | Required |
|----------|-------------|----------|
| `RUNPOD_API_KEY` | Your RunPod API key | Yes |
| `HF_TOKEN` | HuggingFace API token (for gated models like Llama) | For gated models |
| `HALLDYLL_CONFIG` | Path to config file | No |
| `AWS_ACCESS_KEY_ID` | AWS credentials (for S3 state) | No |
| `AWS_SECRET_ACCESS_KEY` | AWS credentials (for S3 state) | No |

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add some amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
