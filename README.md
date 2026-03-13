# 🦙 Herd

[![GitHub release](https://img.shields.io/github/v/release/swift-innovate/herd)](https://github.com/swift-innovate/herd/releases/latest)
[![GitHub stars](https://img.shields.io/github/stars/swift-innovate/herd?style=social)](https://github.com/swift-innovate/herd/stargazers)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.70+-orange.svg)](https://www.rust-lang.org)
[![Roadmap](https://img.shields.io/badge/roadmap-v0.4%20observability%20%26%20operations-blue)](ROADMAP.md)

## Built for OpenClaw swarms 🦞
Running multiple agents + parallel tool calls? Herd stops the GPU wars.
Just set your OpenClaw `baseUrl` to `http://your-herd:40114` — done.
Model homing + live VRAM routing keeps every agent on the fastest node.

> **Pro tip:** Point your OpenClaw agents at Herd and they instantly become GPU-smart — live VRAM routing, model homing, zero cold-start tax, and buttery parallel tool calls across your whole swarm.
<img width="1735" height="803" alt="image" src="https://github.com/user-attachments/assets/d625b30f-8110-482e-80cd-e3297a5ff428" />



**Intelligent Ollama router with GPU awareness, analytics, and real-time monitoring.**

Route your llama herd with intelligence — priority routing, circuit breakers, model awareness, real-time GPU metrics, beautiful dashboards, and OpenAI-compatible endpoints.

## Features

### Core Routing
- **Priority-based routing** — Route to the best GPU first
- **Model-aware routing** — Route to nodes with models already loaded
- **Weighted round-robin** — Distribute by priority weight (new in v0.3.0)
- **Least-busy routing** — Route to lowest GPU utilization
- **Tag-based routing** — Filter by `X-Herd-Tags` header (new in v0.3.0)
- **Circuit breaker** — Auto-recover from failed nodes
- **keep_alive injection** — Override `keep_alive` on every Ollama request centrally; prevents clients from accidentally evicting models (new in v0.4.3)
- **Hot models warmer** — Declare `hot_models` per backend; Herd pre-loads and keeps them warm automatically (new in v0.4.3)
- **Hot-reload config** — File watcher + `POST /admin/reload` (new in v0.3.0)
- **Rate limiting** — Global token-bucket rate limiter
- **OpenAI-compatible** — Drop-in `/v1/chat/completions` endpoint
- **Auto-update** — `herd --update` or `POST /admin/update` (new in v0.4.0)

### Agent-Friendly
- **Agent skills reference** — [`skills.md`](skills.md) teaches AI agents how to use Herd's API, routing, and headers
- **Dashboard Agent Guide** — Built-in tab at `/dashboard` with endpoint tables, best practices, and error handling
- **OpenAI drop-in** — Point any agent's `base_url` to Herd and it just works
- **Correlation IDs** — `X-Request-Id` propagation for distributed agent tracing
- **Tag-based routing** — Agents can target specific backends via `X-Herd-Tags` header

### Observability
- **Prometheus metrics** — `/metrics` endpoint with request counters, backend gauges, and latency histogram
- **Log rotation** — Size-based rotation with configurable retention (days, max size, max files)
- **Request analytics** — JSONL logging with auto-retention
- **Interactive dashboard** — Real-time charts with Chart.js (Backends, Analytics, Agent Guide tabs)
- **GPU metrics** — Real-time VRAM, utilization, temperature
- **Latency tracking** — P50, P95, P99 percentiles
- **Update checker** — Automatic GitHub release notifications

> **v0.4.1** — Agent Guide dashboard tab, skills.md reference, Prometheus metrics, correlation IDs, log rotation. See the [Roadmap](ROADMAP.md) for what's next.

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

## For AI Agents

Herd ships with built-in documentation for AI agents routed through it:

- **`GET /skills`** — JSON endpoint agents can fetch at startup for best practices, endpoints, headers, and error codes. Self-service onboarding.
- **[`skills.md`](skills.md)** — Complete API reference with examples. Point your agent at this file for the full guide.
- **Dashboard Agent Guide** — The `/dashboard` includes an "Agent Guide" tab with endpoint tables, do/don't checklists, and error handling.

```bash
# Agent self-onboarding: fetch skills at startup
curl http://herd:40114/skills | jq .best_practices
```

**Key things agents should know:**
1. Always specify `"model"` in requests for optimal routing
2. Use `"stream": true` for long responses
3. Send `X-Herd-Tags` to target specific backends
4. Send `X-Request-Id` for traceability across distributed systems
5. Query `GET /v1/models` to discover available models before requesting

## Configuration

```yaml
# herd.yaml
server:
  host: "0.0.0.0"
  port: 40114
  api_key: "your-secret-key"  # Required for admin API
  rate_limit: 0               # Requests/sec (0 = unlimited)

routing:
  strategy: "model_aware"  # priority | model_aware | least_busy | weighted_round_robin
  timeout: 120s
  retry_count: 2
  default_keep_alive: "-1"  # inject into every Ollama request (v0.4.3+)

model_warmer:              # v0.4.3+: replaces model_homing
  interval_secs: 240       # ping hot_models every 4 min

backends:
  - name: "citadel-5090"
    url: "http://citadel:11434"
    priority: 100
    gpu_hot_url: "http://citadel:1312"
    tags: ["gpu", "fast"]              # For tag-based routing
    health_check_path: "/api/version"  # Custom health endpoint

  - name: "minipc-4080"
    url: "http://minipc:11434"
    priority: 80
    hot_models:                # keep these loaded at all times (v0.4.3+)
      - "glm-4.7-flash:latest"

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
  log_retention_days: 7      # Auto-prune logs older than N days
  log_max_size_mb: 100       # Rotate log file when it exceeds N MB
  log_max_files: 5           # Keep N rotated log files
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

### Correlation IDs

Every request gets an `X-Request-Id` header for end-to-end tracing:

```bash
# Herd generates a UUID v4 if you don't provide one
curl http://localhost:40114/v1/chat/completions -d '...'
# Response includes: X-Request-Id: 550e8400-e29b-41d4-a716-446655440000

# Or provide your own — Herd forwards it to the upstream backend
curl -H "X-Request-Id: my-trace-123" http://localhost:40114/api/generate -d '...'
```

Request IDs are included in JSONL analytics logs for correlation across systems.

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
| `GET /skills` | Agent skills JSON (endpoints, headers, best practices) |
| `POST /admin/backends` | Add backend at runtime |
| `GET /admin/backends/:name` | Get backend details |
| `PUT /admin/backends/:name` | Update backend config |
| `DELETE /admin/backends/:name` | Remove backend |
| `POST /admin/reload` | Hot-reload config file (when enabled, API key required) |
| `POST /admin/update` | Self-update from GitHub Releases (API key required) |

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
- **Agent Guide tab** — API reference, best practices, and error handling for AI agents

### Request Logging
All proxied requests are logged to `~/.herd/requests.jsonl`:

```json
{"timestamp":1709395200,"model":"glm-4.7-flash:latest","backend":"citadel-5090","duration_ms":234,"status":"success","path":"/api/generate","request_id":"550e8400-e29b-41d4-a716-446655440000"}
```

**Log management:**
- Logs older than `log_retention_days` (default 7) are pruned daily at 3 AM
- Log files are rotated when they exceed `log_max_size_mb` (default 100 MB)
- Up to `log_max_files` (default 5) rotated files are kept

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

## Auto-Update

Herd can update itself from GitHub Releases:

```bash
# CLI: check and install update
herd --update

# API: trigger update remotely (requires API key)
curl -X POST -H "X-API-Key: your-key" http://localhost:40114/admin/update

# Check without installing (no auth required)
curl http://localhost:40114/update
```

On startup, Herd checks for updates in the background and logs a notification if a newer version is available.

**Note:** After updating via `--update` or `/admin/update`, the server must be restarted to run the new version. The previous binary is kept as a backup for rollback.

## Hot Models & keep_alive (v0.4.3)

> **⚠️ Breaking change in v0.4.3:** `default_model` and `routing.idle_timeout_minutes` are removed. See the migration guide below.

Herd v0.4.3 solves the model eviction problem centrally. Ollama unloads models after 5 minutes by default, and clients like Open WebUI often send `"keep_alive": "5m"` which overrides any node-level env var. Herd fixes this at the proxy layer.

### keep_alive Injection

Add to `herd.yaml`:

```yaml
routing:
  default_keep_alive: "-1"   # never unload; set on every Ollama request
```

Herd injects this into every `/api/generate` and `/api/chat` request body, overriding whatever the client sent. `/v1/*` (OpenAI format) requests are passed through unchanged.

### Hot Models Warmer

```yaml
model_warmer:
  interval_secs: 240   # ping every 4 min (default); safely under Ollama's 5-min eviction window

backends:
  - name: "citadel"
    url: "http://citadel:11434"
    hot_models:
      - "glm-4.7-flash:latest"
      - "llama3:8b"
```

Herd pre-loads declared models on startup and re-loads them after OOM eviction by sending a minimal `keep_alive: "-1"` ping on every interval. No idle timer — models are always warm.

### Migration from v0.4.2

| Before | After |
|---|---|
| `backends[].default_model: "model:tag"` | `backends[].hot_models: ["model:tag"]` |
| `routing.idle_timeout_minutes: 30` | `model_warmer.interval_secs: 240` |

Old config keys are silently ignored after upgrading — **no startup error, but homing stops working**. Update your `herd.yaml` before upgrading.

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
| keep_alive injection | ✅ | ❌ |
| Hot models warmer | ✅ | ❌ |
| GPU metrics | ✅ | ❌ |
| Observability API | ✅ | ❌ |
| Retry with fallback | ✅ | ❌ |
| Admin API | ✅ | ❌ |
| OpenAI-compatible API | ✅ | ❌ |
| Streaming completions | ✅ | ❌ |
| Tag-based routing | ✅ | ❌ |
| Hot-reload config | ✅ | ❌ |
| Rate limiting | ✅ | ❌ |
| Prometheus metrics | ✅ | ❌ |
| Correlation IDs | ✅ | ❌ |
| Log rotation | ✅ | ❌ |
| Auto-update | ✅ | ❌ |
| Language | Rust | Go |

## License

MIT

## Support

If Herd is useful to you, consider sponsoring development:

[![GitHub Sponsors](https://img.shields.io/github/sponsors/swift-innovate?style=social)](https://github.com/sponsors/swift-innovate)

Your support helps keep the project maintained and moving forward. Thank you!

---

**Herd your llamas with intelligence.** 🦙
