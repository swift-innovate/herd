# 🦙 Herd

**Intelligent Ollama router with GPU awareness.**

Route your llama herd with intelligence — priority routing, circuit breakers, model awareness, and real-time GPU metrics.

## Features

- **Priority-based routing** — Route to the best GPU first
- **Circuit breaker** — Auto-recover from failed nodes
- **Model-aware** — Route to nodes with models already loaded
- **GPU metrics** — Real-time VRAM, utilization, temperature
- **Prometheus metrics** — `/metrics` endpoint for Grafana
- **Hot reload** — Add/remove nodes without restart

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

| Endpoint | Description |
|----------|-------------|
| `GET /` | Proxy to highest priority backend |
| `POST /api/*` | Forward Ollama API requests |
| `GET /status` | Node health + GPU metrics |
| `GET /metrics` | Prometheus metrics |
| `GET /health` | K8s liveness probe |
| `POST /admin/backends` | Add/remove backends at runtime |

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
| Language | Rust | Go |

## License

MIT

---

**Herd your llamas with intelligence.** 🦙