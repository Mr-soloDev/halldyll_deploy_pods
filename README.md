# 🚀 Halldyll Deploy Pods

[![Crates.io](https://img.shields.io/crates/v/halldyll_deploy_pods.svg)](https://crates.io/crates/halldyll_deploy_pods)
[![Documentation](https://docs.rs/halldyll_deploy_pods/badge.svg)](https://docs.rs/halldyll_deploy_pods)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/rust-1.85+-orange.svg)](https://www.rust-lang.org)

**A declarative, idempotent, and reconcilable deployment system for [RunPod](https://runpod.io) GPU pods.**

Think of it as **Terraform/Kubernetes for RunPod** — define your GPU infrastructure as code, and let Halldyll handle the rest.

## ✨ Features

- 📝 **Declarative** — Define your infrastructure in a simple YAML file
- 🔄 **Idempotent** — Run `apply` multiple times, get the same result
- 🔍 **Drift Detection** — Automatically detect and fix configuration drift
- 🔁 **Reconciliation Loop** — Continuously converge to desired state
- 💾 **State Management** — Track deployments locally or on S3
- 🏷️ **Multi-environment** — Support for dev, staging, prod environments
- 🛡️ **Guardrails** — Cost limits, GPU limits, TTL auto-stop

## 📦 Installation

### From Crates.io

```bash
cargo install halldyll_deploy_pods
```

### From Source

```bash
git clone https://github.com/Mr-soloDev/halldyll_deploy_pods.git
cd halldyll_deploy_pods
cargo install --path .
```

## 🚀 Quick Start

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

## 📖 Commands

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

## ⚙️ Configuration Reference

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

### Guardrails (Optional)

```yaml
guardrails:
  max_hourly_cost: 10.0       # Maximum hourly cost in USD
  max_gpus: 4                 # Maximum total GPUs
  ttl_hours: 24               # Auto-stop after N hours
  allow_gpu_fallback: false   # Allow fallback to other GPU types
```

## 🏗️ Architecture

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

## 🔧 Library Usage

You can also use Halldyll as a library in your Rust projects:

```rust
use halldyll_deploy_pods::{
    ConfigParser, ConfigValidator, DeployConfig,
    RunPodClient, PodProvisioner, PodObserver,
    Reconciler, StateStore, LocalStateStore,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse configuration
    let config = ConfigParser::parse_file("halldyll.deploy.yaml")?;
    
    // Validate
    ConfigValidator::validate(&config)?;
    
    // Create RunPod client
    let client = RunPodClient::new(&std::env::var("RUNPOD_API_KEY")?)?;
    
    // ... use provisioner, observer, reconciler
    
    Ok(())
}
```

## 🌍 Environment Variables

| Variable | Description | Required |
|----------|-------------|----------|
| `RUNPOD_API_KEY` | Your RunPod API key | Yes |
| `HALLDYLL_CONFIG` | Path to config file | No |
| `AWS_ACCESS_KEY_ID` | AWS credentials (for S3 state) | No |
| `AWS_SECRET_ACCESS_KEY` | AWS credentials (for S3 state) | No |

## 🤝 Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add some amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## 👤 Author

**Geryan Roy** ([@Mr-soloDev](https://github.com/Mr-soloDev))

- Email: geryan.roy@icloud.com

## 🙏 Acknowledgments

- [RunPod](https://runpod.io) for the amazing GPU cloud platform
- Inspired by Terraform, Kubernetes, and other declarative infrastructure tools
