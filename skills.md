# Herd — Agent Skills Reference

> This document is for AI agents that are routed through Herd.
> Read this to understand how to make optimal requests.

## What Herd Does

Herd is a reverse proxy that sits between you and one or more Ollama backends.
When you send a request, Herd picks the best backend based on model availability,
GPU load, priority, and tags. If a backend fails, Herd retries on another.

**You do not talk to Ollama directly. You talk to Herd, and Herd routes for you.**

## Base URL

Herd listens on port `40114` by default.

```
http://<herd-host>:40114
```

All examples below assume this base URL.

## Endpoints You Should Use

### Chat Completions (OpenAI-compatible)

```
POST /v1/chat/completions
Content-Type: application/json
```

```json
{
  "model": "qwen2.5-coder:32b",
  "messages": [
    {"role": "system", "content": "You are a helpful assistant."},
    {"role": "user", "content": "Explain TCP handshakes."}
  ],
  "stream": true
}
```

- **Streaming:** Set `"stream": true` for SSE streaming (recommended for long responses).
- **Model:** Always specify the model you need. Herd uses this to route to a backend that already has it loaded, avoiding cold-start model loading.
- **Response format:** Identical to OpenAI's API. Your existing OpenAI client libraries work unchanged.

### List Available Models

```
GET /v1/models
```

Returns all models available across all healthy backends (deduplicated).

```json
{
  "object": "list",
  "data": [
    {"id": "qwen2.5-coder:32b", "object": "model", "owned_by": "ollama"},
    {"id": "llama3.1:8b", "object": "model", "owned_by": "ollama"}
  ]
}
```

**Use this to discover which models are available before making requests.**

### Health Check

```
GET /health
```

Returns `200 OK` with body `"OK"` if Herd is running. No auth required.

### Cluster Status

```
GET /status
```

Returns detailed backend information:

```json
{
  "healthy_backends": [
    {
      "name": "citadel-5090",
      "url": "http://citadel:11434",
      "priority": 100,
      "models": ["qwen2.5-coder:32b", "llama3.1:8b"],
      "model_count": 2,
      "current_model": "qwen2.5-coder:32b",
      "healthy": true,
      "gpu": {
        "utilization": 45.0,
        "memory_used": 18432,
        "memory_total": 24576,
        "temperature": 62.0
      }
    }
  ],
  "unhealthy_backends": [],
  "routing_strategy": "ModelAware",
  "idle_timeout_minutes": 30
}
```

### Ollama Native API (Proxied)

Herd proxies all Ollama endpoints transparently:

```
POST /api/generate      — Single-turn generation
POST /api/chat          — Multi-turn chat
GET  /api/tags          — List models on routed backend
GET  /api/ps            — Running models on routed backend
POST /api/pull          — Pull a model (routes to one backend)
POST /api/show          — Model info
```

These go through the same routing logic. Include `"model"` in your JSON body
so Herd can route to the right backend.

### Analytics

```
GET /analytics?hours=24
```

Returns request statistics: total requests, latency percentiles (p50/p95/p99),
model counts, backend counts, and a timeline.

### Prometheus Metrics

```
GET /metrics
```

Standard Prometheus exposition format with backend gauges, request counters,
and latency histograms.

## Headers You Should Know

### X-Herd-Tags (Request Routing)

Target specific backends by tag:

```
X-Herd-Tags: gpu,fast
```

Backends are configured with tags like `tags: ["gpu", "fast"]`. When you send
this header, Herd only considers backends matching **all** specified tags.

**Use case:** Route coding tasks to high-VRAM GPUs, research to slower but larger-context nodes.

### X-Request-Id (Correlation)

```
X-Request-Id: my-trace-id-123
```

- If you send this header, Herd preserves it through to the backend and returns it in the response.
- If you don't send it, Herd generates a UUID v4 and returns it in the response.
- Use this to correlate requests across distributed systems.

### Authentication (Admin Only)

Admin endpoints require an API key. Regular routing endpoints do not.

```
X-API-Key: your-secret-key
```

or

```
Authorization: Bearer your-secret-key
```

## Routing Strategies

Herd supports four strategies. You don't choose the strategy per-request — it's configured
server-side. But understanding the active strategy helps you make better requests.

| Strategy | Behavior |
|----------|----------|
| `model_aware` | Prefers backends that already have your requested model loaded. Avoids cold starts. **(default)** |
| `priority` | Always routes to the highest-priority healthy backend. |
| `least_busy` | Routes to the backend with the lowest GPU utilization. |
| `weighted_round_robin` | Distributes across backends weighted by priority. |

Check `GET /status` → `routing_strategy` to see which is active.

## Best Practices for Agents

### 1. Always specify the model

```json
{"model": "qwen2.5-coder:32b", "messages": [...]}
```

Without a model, Herd can't do model-aware routing and may route you to a backend
that needs to cold-load the model (slow).

### 2. Use streaming for long responses

```json
{"stream": true}
```

Streaming prevents timeout issues on long generations and gives you incremental output.

### 3. Discover models before requesting

```bash
GET /v1/models
```

Don't guess model names. Query available models first to avoid 404s from Ollama.

### 4. Use tags for workload isolation

If your Herd instance has tagged backends:

```
X-Herd-Tags: fast
```

This ensures your request goes to a backend suited for your workload type.

### 5. Handle 503 gracefully

A `503 Service Unavailable` means no healthy backend could serve your request.
This can happen when:
- All backends are down
- No backend has the requested model and `model_aware` routing is active
- Circuit breakers have tripped on all backends

**Retry after a few seconds.** Herd's circuit breaker automatically recovers backends.

### 6. Handle 502 gracefully

A `502 Bad Gateway` means Herd reached a backend but it failed. The response includes
a `request_id` for debugging:

```json
{"error": "Bad Gateway", "request_id": "abc-123-def"}
```

### 7. Don't hard-code backend URLs

Always go through Herd. Never bypass it to talk to individual Ollama instances.
Herd handles failover, load balancing, and model routing — bypassing it defeats the purpose.

### 8. Send correlation IDs for traceability

```
X-Request-Id: agent-task-42-step-3
```

This makes it easy to trace your requests in logs and analytics.

## Circuit Breaker Behavior

When a backend fails repeatedly (default: 3 consecutive failures):
1. Herd marks it **unhealthy** and stops routing to it
2. After a recovery period (default: 60 seconds), Herd probes it again
3. If the probe succeeds, the backend is marked **healthy** and receives traffic

This is automatic. You don't need to do anything — just retry your request and
Herd will route to a healthy backend.

## Rate Limiting

If Herd has rate limiting enabled, you'll receive `429 Too Many Requests` when
the limit is exceeded. Back off and retry.

## Model Homing

Herd automatically loads default models on idle backends. If a backend has
`default_model: "llama3.1:8b"` configured and sits idle past the timeout,
Herd sends a request to load that model. This means popular models stay warm.

## Quick Reference

| Action | Method | Endpoint |
|--------|--------|----------|
| Chat (OpenAI) | POST | `/v1/chat/completions` |
| List models | GET | `/v1/models` |
| Health check | GET | `/health` |
| Cluster status | GET | `/status` |
| GPU metrics | GET | `/gpu` |
| Analytics | GET | `/analytics?hours=24` |
| Prometheus | GET | `/metrics` |
| Update check | GET | `/update` |
| Ollama generate | POST | `/api/generate` |
| Ollama chat | POST | `/api/chat` |
| Ollama models | GET | `/api/tags` |
| Dashboard | GET | `/dashboard` |

## Example: Full Agent Workflow

```python
import requests

HERD = "http://herd:40114"

# 1. Discover available models
models = requests.get(f"{HERD}/v1/models").json()
available = [m["id"] for m in models["data"]]
print(f"Available: {available}")

# 2. Pick the best model for the task
model = "qwen2.5-coder:32b" if "qwen2.5-coder:32b" in available else available[0]

# 3. Send a chat request with correlation ID
response = requests.post(
    f"{HERD}/v1/chat/completions",
    headers={
        "X-Request-Id": "agent-session-001",
        "X-Herd-Tags": "gpu",
    },
    json={
        "model": model,
        "messages": [
            {"role": "system", "content": "You are a senior engineer."},
            {"role": "user", "content": "Review this code for security issues."},
        ],
        "stream": False,
    },
)

# 4. Use the response
result = response.json()
print(result["choices"][0]["message"]["content"])
```

## Configuration Reference (for operators)

Agents don't configure Herd, but understanding the config helps predict behavior:

```yaml
server:
  host: "0.0.0.0"
  port: 40114
  api_key: "secret"       # Required for admin API
  rate_limit: 0           # 0 = unlimited

routing:
  strategy: "model_aware" # priority | model_aware | least_busy | weighted_round_robin
  timeout: 120s           # Per-request timeout
  retry_count: 2          # Retries on failure
  idle_timeout_minutes: 30

backends:
  - name: "citadel"
    url: "http://citadel:11434"
    priority: 100
    default_model: "qwen2.5-coder:32b"
    tags: ["gpu", "fast"]
    model_filter: "qwen|llama"  # Regex: only route matching models

circuit_breaker:
  failure_threshold: 3
  timeout: 120s
  recovery_time: 60s
```
