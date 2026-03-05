# 🦙 Herd

[![GitHub release](https://img.shields.io/github/v/release/swift-innovate/herd)](https://github.com/swift-innovate/herd/releases/latest)
[![GitHub stars](https://img.shields.io/github/stars/swift-innovate/herd?style=social)](https://github.com/swift-innovate/herd/stargazers)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.70+-orange.svg)](https://www.rust-lang.org)
[![Roadmap](https://img.shields.io/badge/roadmap-v0.3%20agent%20gateway-blue)](ROADMAP.md)

**Intelligent Ollama router with GPU awareness, analytics, and real-time monitoring.**

Route your llama herd with intelligence — priority routing, circuit breakers, model awareness, real-time GPU metrics, beautiful dashboards, and OpenAI-compatible endpoints.

## Features

### Core Routing
- **Priority-based routing** — Route to the best GPU first
- **Circuit breaker** — Auto-recover from failed nodes
- **Model-aware** — Route to nodes with models already loaded
- **Model homing** — Auto-load default models on idle nodes
- **Hot reload** — Add/remove nodes without restart via API
- **OpenAI-compatible** — Drop-in `/v1/chat/completions` endpoint for any OpenAI client

### Observability (New in v0.2.0) 📊
- **Request analytics** — JSONL logging with 7-day auto-retention
- **Interactive dashboard** — Real-time charts with Chart.js
  - Request volume timeline (last 20 minutes)
  - Requests by model (top 5)
  - Requests by backend
- **GPU metrics** — Real-time VRAM, utilization, temperature
- **Latency tracking** — P50, P95, P99 percentiles
- **Update checker** — Automatic GitHub release notifications
- **Prometheus metrics** — `/metrics` endpoint for Grafana

> **Herd is growing.** Multi-agent orchestration, session management, and a permission engine are coming in v0.3.0. See the [Roadmap](ROADMAP.md).

## Quick Start

```bash
# Install
cargo install herd

# Run with config
herd --config herd.yaml

# Or with CLI args
herd --port 40114 \
  --backend citadel=http://citadel:11434:100 \
  --backend minipc=http://minipc:11434:80 \
  --backend warden=http://warden:11434:50
```

## Configuration

```yaml
# herd.yaml
server:
  host: "0.0.0.0"
  port: 40114

routing:
  strategy: "model_aware"  # priority | model_aware | least_busy
  timeout: 120s
  retry_count: 2

backends:
  - name: "citadel-5090"
    url: "http://citadel:11434"
    priority: 100
    gpu_hot_url: "http://citadel:1312"  # Optional: GPU metrics

  - name: "minipc-4080"
    url: "http://minipc:11434"
    priority: 80

  - name: "warden-4070"
    url: "http://warden:11434"
    priority: 50
    model_filter: "≤8B"  # Only route small models

circuit_breaker:
  failure_threshold: 3
  timeout: 30s
  recovery_time: 60s

observability:
  metrics: true
  admin_api: true
```

## API Endpoints

### OpenAI-Compatible Endpoints

Point any OpenAI client at Herd and get full model-aware routing across your cluster:

```bash
# Works with OpenAI SDK, Open WebUI, Continue.dev, LiteLLM, Cursor, etc.
base_url: http://herd:40114/v1
api_key: anything   # Ollama doesn't require auth; any value works
```

| Endpoint | Description |
|----------|-------------|
| `GET /v1/models` | List all models from healthy backends |
| `POST /v1/chat/completions` | Chat completions (streaming supported) |
| `POST /v1/completions` | Text completions (streaming supported) |

All `/v1/*` requests use the same intelligent routing as native Ollama calls — model-aware, priority-based, with circuit breakers.

### All Endpoints

| Endpoint | Description |
|----------|-------------|
| `GET /` | Proxy to highest priority backend |
| `POST /api/*` | Forward Ollama API requests |
| `GET /v1/models` | OpenAI-compatible model list |
| `POST /v1/chat/completions` | OpenAI-compatible chat (streaming) |
| `GET /dashboard` | Interactive analytics dashboard |
| `GET /status` | Node health + GPU metrics (JSON) |
| `GET /analytics?hours=N` | Request stats, latency, timeline |
| `GET /update` | Check for new releases |
| `GET /metrics` | Prometheus metrics |
| `GET /health` | K8s liveness probe |
| `POST /admin/backends` | Add backend at runtime |
| `GET /admin/backends/:name` | Get backend details |
| `PUT /admin/backends/:name` | Update backend config |
| `DELETE /admin/backends/:name` | Remove backend |

## Analytics & Monitoring (v0.2.0)

### Dashboard
Access the interactive dashboard at `http://your-herd:40114/dashboard`

**Features:**
- Real-time node status with GPU metrics
- Live request volume chart (updates every 30s)
- Top 5 models by request count
- Backend utilization distribution
- Model homing status and idle timers
- One-click backend management (add/edit/remove)
- Automatic update notifications

### Request Logging
All proxied requests are logged to `~/.herd/requests.jsonl`:

```json
{"timestamp":1709395200,"model":"glm-4.7-flash:latest","backend":"citadel-5090","duration_ms":234,"status":"success","path":"/api/generate"}
```

**Auto-cleanup:** Logs older than 7 days are automatically pruned at 3 AM daily.

### Analytics API
Query statistics programmatically:

```bash
# Last 24 hours (default)
curl http://localhost:40114/analytics

# Last hour
curl http://localhost:40114/analytics?hours=1

# Response
{
  "total_requests": 1523,
  "latency_p50": 145,
  "latency_p95": 892,
  "latency_p99": 1204,
  "model_counts": {
    "glm-4.7-flash:latest": 892,
    "qwen2.5-coder:32b": 431,
    "llama3.1:8b": 200
  },
  "backend_counts": {
    "citadel-5090": 1203,
    "minipc-4080": 320
  },
  "timeline": [[1709395200, 45], [1709395260, 52], ...]
}
```

## Model Homing

Herd keeps idle nodes "warm" by loading their default model after the idle timeout:

```yaml
routing:
  idle_timeout_minutes: 30

backends:
  - name: "citadel"
    url: "http://citadel:11434"
    default_model: "glm-4.7-flash:latest"
```

**How it works:**
1. When a node sits idle for 30 minutes (no model loaded or running a non-default model)
2. Herd sends a warmup request to load the default model
3. Dashboard shows "Homing to default model..." with progress
4. Once loaded, status shows "✓ Running default model"

**Important:** After warming, Ollama may unload the model if no requests come in. This is expected - Ollama frees VRAM when idle. Herd will warm it again on the next cycle.

**Dashboard indicators:**
- 🟢 "Running default model" — Node is on its default model
- 🟡 "Returning to default in 25m" — Active model differs from default, timer counting down
- ⏳ "Homing to default model... 100%" — Warmup request sent, model loading/loaded

## GPU Awareness

Herd integrates with [gpu-hot](https://github.com/psalias2006/gpu-hot) for real-time metrics:

```yaml
# On each GPU node
docker run -d --gpus all -p 1312:1312 \
  -e NODE_NAME=citadel \
  ghcr.io/psalias2006/gpu-hot:latest
```

Then configure Herd to query metrics:

```yaml
backends:
  - name: "citadel"
    url: "http://citadel:11434"
    gpu_hot_url: "http://citadel:1312"
```

**Dashboard GPU section:**
- Displays per-GPU cards with utilization, temperature, memory, power draw
- Auto-polls every 10 seconds
- Automatically hides if gpu-hot is unreachable
- Shows all GPUs on multi-GPU nodes

**Example output:**
```json
{
  "available": true,
  "gpus": {
    "0": {
      "name": "NVIDIA GeForce RTX 5090",
      "temperature": 37.0,
      "utilization": 2.0,
      "memory_used": 3731.48,
      "memory_total": 32607.0,
      "power_draw": 70.0
    }
  }
}
```

Then configure Herd to query metrics:

```yaml
backends:
  - name: "citadel"
    url: "http://citadel:11434"
    gpu_hot_url: "http://citadel:1312"
```

Herd will route based on:
- Model already loaded (via `/api/ps`)
- GPU VRAM available
- Current utilization

## Architecture

```
┌─────────────────────────────────────────────────┐
│                    Herd                          │
├─────────────────────────────────────────────────┤
│  ┌─────────┐  ┌─────────┐  ┌─────────────┐     │
│  │  HTTP   │  │ Router  │  │   Circuit   │     │
│  │  Proxy  │→ │ Engine  │→ │   Breaker   │     │
│  └─────────┘  └─────────┘  └─────────────┘     │
│       ↓            ↓              ↓              │
│  ┌────────────────────────────────────────┐   │
│  │            Backend Pool                 │   │
│  │  ┌─────────┐ ┌─────────┐ ┌─────────┐   │   │
│  │  │ Citadel │ │  minipc │ │  warden │   │   │
│  │  │ :11434  │ │ :11434  │ │ :11434  │   │   │
│  │  │ :1312   │ │ :1312   │ │ :1312   │   │   │
│  │  └─────────┘ └─────────┘ └─────────┘   │   │
│  └────────────────────────────────────────┘   │
└─────────────────────────────────────────────────┘
```

## Comparison to Olla

| Feature | Herd | Olla |
|---------|------|------|
| Priority routing | ✅ | ✅ |
| Circuit breaker | ✅ | ✅ |
| Model awareness | ✅ | ❌ |
| GPU metrics | ✅ | ❌ |
| Observability API | ✅ | ❌ |
| Retry with fallback | ✅ | ❌ |
| Admin API | ✅ | ❌ |
| OpenAI-compatible API | ✅ | ❌ |
| Streaming completions | ✅ | ❌ |
| Language | Rust | Go |

## License

MIT

## Support

If Herd is useful to you, consider sponsoring development:

[![GitHub Sponsors](https://img.shields.io/github/sponsors/swift-innovate?style=social)](https://github.com/sponsors/swift-innovate)

Your support helps keep the project maintained and moving forward. Thank you!

---

**Herd your llamas with intelligence.** 🦙