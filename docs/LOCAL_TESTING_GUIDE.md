# Herd Local Testing Guide

For someone with Ollama already installed who wants to test Herd locally — including
the llama-server backend and Ollama blob extraction.

---

## 1. Prerequisites

- **Rust toolchain** — `rustup` installed, `cargo` in PATH
- **Ollama** — running locally with at least one model pulled (`ollama list` should show models)
- **Optional:** llama-server binary (see Section 5)

---

## 2. Build and Run Herd

```powershell
cd G:\Projects\herd
cargo build --release
```

Create a minimal `herd.yaml` in the project root:

```yaml
# herd.yaml — minimal local test config
server:
  host: "127.0.0.1"
  port: 40114
  api_key: "test-key"

routing:
  strategy: "model_aware"
  timeout: "120s"
  retry_count: 2
  default_keep_alive: "-1"

backends:
  - name: "local-ollama"
    url: "http://localhost:11434"
    backend: "ollama"
    priority: 100

observability:
  metrics: true
  admin_api: true
```

Run Herd:

```powershell
.\target\release\herd.exe --config herd.yaml
```

Expected startup output:

```
INFO herd: Starting Herd v1.1.0 on 127.0.0.1:40114
INFO herd: Backend loaded: local-ollama (http://localhost:11434) [ollama]
INFO herd: Dashboard: http://127.0.0.1:40114/dashboard
```

Verify it's up:

```powershell
curl http://localhost:40114/health
# → OK

curl http://localhost:40114/status
# → JSON with backend list and health status
```

Open the dashboard at `http://localhost:40114/dashboard`. The Backends tab should
show `local-ollama` as healthy.

---

## 3. Basic Routing Test

Point any OpenAI-compatible client at `http://localhost:40114`.

**Chat completions (OpenAI-compatible):**

```powershell
curl -X POST http://localhost:40114/v1/chat/completions `
  -H "Content-Type: application/json" `
  -d '{
    "model": "llama3.2:3b",
    "messages": [{"role": "user", "content": "Say hello in one sentence."}],
    "stream": false
  }'
```

Expected response includes `X-Request-Id` in headers and a normal chat response body.

**Native Ollama generate endpoint:**

```powershell
curl -X POST http://localhost:40114/api/generate `
  -H "Content-Type: application/json" `
  -d '{"model": "llama3.2:3b", "prompt": "Hello!", "stream": false}'
```

**Verify routing via response header:**

```powershell
curl -i -X POST http://localhost:40114/v1/chat/completions `
  -H "Content-Type: application/json" `
  -d '{"model": "llama3.2:3b", "messages": [{"role":"user","content":"ping"}], "stream": false}'
```

Look for `X-Request-Id` in the response headers — Herd injects a UUID on every
proxied request. You can also supply your own: `-H "X-Request-Id: my-trace-001"`.

---

## 4. Extracting Models from Ollama for llama-server

Ollama stores models as raw GGUF blobs under `%USERPROFILE%\.ollama\models\blobs\`.
Herd can find and copy these for use with llama-server.

**List extractable models:**

```powershell
curl http://localhost:40114/api/ollama/models
```

Expected output:

```json
[
  {
    "model": "llama3.2",
    "tag": "3b",
    "blob_path": "C:\\Users\\you\\.ollama\\models\\blobs\\sha256-abc123...",
    "size_bytes": 2019483648,
    "digest": "sha256:abc123..."
  }
]
```

**Extract a GGUF to a target path:**

```powershell
curl -X POST http://localhost:40114/api/ollama/extract `
  -H "Content-Type: application/json" `
  -d '{
    "model": "llama3.2",
    "tag": "3b",
    "target": "C:\\models\\llama3.2-3b.gguf"
  }'
```

On Windows, Herd copies the blob (symlinks require admin privileges). The extracted
file is a raw GGUF — no conversion needed.

**Verify the GGUF is valid:**

```powershell
# Check file size matches what Herd reported
(Get-Item "C:\models\llama3.2-3b.gguf").Length

# Inspect GGUF header (first 4 bytes should be "GGUF")
$bytes = [System.IO.File]::ReadAllBytes("C:\models\llama3.2-3b.gguf")[0..3]
[System.Text.Encoding]::ASCII.GetString($bytes)
# → GGUF
```

The model is now ready for llama-server.

---

## 5. Setting Up llama-server Backend

### Option A: Download via herd-tune (recommended)

Download the herd-tune script from your running Herd instance:

```powershell
# Downloads the script pre-configured with your Herd endpoint
irm "http://localhost:40114/api/nodes/script?os=windows&backend=llama-server" -OutFile herd-tune.ps1

# Run it — detects GPU, downloads the correct llama-server binary, starts the server
.\herd-tune.ps1 -Backend llama-server -Model "C:\models\llama3.2-3b.gguf" -Herd "http://localhost:40114"
```

herd-tune will:
1. Detect your GPU vendor via `nvidia-smi`, `rocm-smi`, or `sycl-ls`
2. Download the correct llama-server binary to `%USERPROFILE%\.herd\bin\`
3. Start llama-server on port 8090
4. Register the node with Herd via `POST /api/nodes/register`

**NVIDIA Blackwell note:** RTX 5000-series GPUs require CUDA 13.x. herd-tune detects
this automatically. If you use a manually downloaded CUDA 12.x build on a 5090/5080,
llama-server silently falls back to CPU — no error, just ~10x slower.

### Option B: Manual download

1. Go to `https://github.com/ggml-org/llama.cpp/releases/latest`
2. Download the binary matching your GPU:
   - NVIDIA (non-Blackwell): `llama-bXXXX-bin-win-cuda-cu12.4-x64.zip`
   - NVIDIA (Blackwell 5000-series): `llama-bXXXX-bin-win-cuda-cu13.x-x64.zip`
   - AMD: `llama-bXXXX-bin-win-hip-x64.zip`
   - Fallback/CPU: `llama-bXXXX-bin-win-vulkan-x64.zip`
3. Extract to `C:\tools\llama.cpp\`

Start llama-server with the extracted GGUF:

```powershell
C:\tools\llama.cpp\llama-server.exe `
  --model "C:\models\llama3.2-3b.gguf" `
  --host 0.0.0.0 `
  --port 8090 `
  --ctx-size 4096 `
  -ngl 99
```

Verify it's running:

```powershell
curl http://localhost:8090/health
# → {"status":"ok"}

curl http://localhost:8090/v1/models
# → {"object":"list","data":[{"id":"llama3.2-3b.gguf",...}]}
```

### Add llama-server to herd.yaml

```yaml
backends:
  - name: "local-ollama"
    url: "http://localhost:11434"
    backend: "ollama"
    priority: 80

  - name: "local-llama"
    url: "http://localhost:8090"
    backend: "llama-server"
    priority: 100
    tags: ["gpu", "fast"]
```

Reload config without restarting:

```powershell
curl -X POST http://localhost:40114/admin/reload `
  -H "X-API-Key: test-key"
```

The Fleet tab in the dashboard should now show `local-llama` as a registered node.

---

## 6. Mixed Fleet Testing

With both backends active, Herd routes model-aware by default. The model loaded in
llama-server (set at startup) takes priority for that model name; Ollama handles
everything else.

**Config for mixed fleet:**

```yaml
backends:
  - name: "local-llama"
    url: "http://localhost:8090"
    backend: "llama-server"
    priority: 100
    tags: ["gpu", "fast"]

  - name: "local-ollama"
    url: "http://localhost:11434"
    backend: "ollama"
    priority: 80
    tags: ["gpu"]
```

**Route a request and verify which backend handled it:**

```powershell
# Request analytics after a few requests
curl http://localhost:40114/analytics | ConvertFrom-Json | Select-Object -ExpandProperty backend_counts
```

Expected:

```json
{
  "local-llama": 5,
  "local-ollama": 3
}
```

**Check the Fleet tab:** `http://localhost:40114/dashboard` → Fleet shows each node
with its backend type badge (Ollama vs llama-server), GPU info, and health status.

**Verify model list includes both backends:**

```powershell
curl http://localhost:40114/v1/models
```

Herd aggregates models from all healthy backends — Ollama uses `/api/tags`, llama-server
uses `/v1/models`. The unified list covers your entire fleet.

---

## 7. Auto Mode Testing

Auto Mode classifies requests with `"model": "auto"` using a small local model and
routes to the best match in your `model_map`.

Enable in `herd.yaml` (requires a small classifier model pulled in Ollama, e.g. `qwen3:1.7b`):

```yaml
routing:
  strategy: "model_aware"
  auto:
    enabled: true
    classifier_model: "qwen3:1.7b"
    classifier_timeout_ms: 3000
    fallback_model: "llama3.2:3b"
    cache_ttl_secs: 60
    model_map:
      light:
        general: "llama3.2:3b"
      heavy:
        code: "qwen2.5-coder:32b"
        general: "llama3.2:3b"
```

Reload config, then send a request with `"model": "auto"`:

```powershell
curl -i -X POST http://localhost:40114/v1/chat/completions `
  -H "Content-Type: application/json" `
  -d '{
    "model": "auto",
    "messages": [{"role": "user", "content": "Write a Python function to sort a list."}],
    "stream": false
  }'
```

Check the response headers:

```
X-Herd-Auto-Tier: light
X-Herd-Auto-Capability: code
X-Herd-Auto-Model: llama3.2:3b
```

Classification results are cached by message hash for `cache_ttl_secs` seconds —
identical requests won't re-classify.

---

## 8. Frontier Gateway Testing (Optional)

Frontier lets Herd route requests to cloud providers (Anthropic, OpenAI, etc.) when
needed. API keys are read from environment variables only — never stored in config.

Set your API key:

```powershell
$env:ANTHROPIC_API_KEY = "sk-ant-..."
```

Add frontier config to `herd.yaml`:

```yaml
frontier:
  enabled: true
  require_header: true
  log_all_requests: true
  warn_threshold: 0.80
  block_threshold: 1.00

providers:
  - name: "anthropic"
    api_url: "https://api.anthropic.com/v1"
    api_key_env: "ANTHROPIC_API_KEY"
    models: ["claude-sonnet-4-20250514"]
    rate_limit: 50
    monthly_budget: 10.00
    priority: 50
```

Send a frontier request:

```powershell
curl -X POST http://localhost:40114/v1/chat/completions `
  -H "Content-Type: application/json" `
  -H "X-Herd-Frontier: true" `
  -d '{
    "model": "claude-sonnet-4-20250514",
    "messages": [{"role": "user", "content": "Hello"}],
    "stream": false
  }'
```

Check spend tracking:

```powershell
curl http://localhost:40114/api/frontier/costs
```

The `X-Herd-Frontier: true` header is required when `require_header: true` (default).
Without it, the request routes to local backends only.

---

## 9. Monitoring

### Dashboard

`http://localhost:40114/dashboard` — seven tabs:

| Tab | What you see |
|-----|-------------|
| Backends | Live node status, GPU metrics, circuit breaker state |
| Analytics | Request volume, top models, P50/P95/P99 latency |
| Sessions | Agent session management |
| Fleet | Registered nodes with GPU badge, backend type, health |
| Models | HuggingFace GGUF search with VRAM compatibility |
| Agent Guide | API reference for agents routing through Herd |
| Settings | Config editor (secrets redacted) |

### Prometheus metrics

```powershell
curl http://localhost:40114/metrics
```

Key metrics:

```
herd_requests_total{status="success"} 42
herd_requests_by_backend{backend="local-llama"} 30
herd_tokens_total{direction="out", model="llama3.2:3b"} 15200
herd_tokens_per_second 98.4
herd_request_duration_ms_bucket{le="1000"} 38
```

### Request analytics

```powershell
# Last 24 hours
curl http://localhost:40114/analytics

# Last hour only
curl "http://localhost:40114/analytics?hours=1"
```

### JSONL request log

Every proxied request is appended to:

```
%USERPROFILE%\.herd\requests.jsonl
```

Sample entry:

```json
{"timestamp":1744118400,"model":"llama3.2:3b","backend":"local-llama","duration_ms":312,"status":"success","path":"/v1/chat/completions","request_id":"550e8400-e29b-41d4-a716-446655440000","tier":"light"}
```

Logs auto-rotate at 100 MB and prune entries older than 7 days (configurable via
`observability.log_max_size_mb` and `observability.log_retention_days`).
