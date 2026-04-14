# Herd v2.0 — Node Mode (MVP)

**Date:** 2026-04-14
**Status:** Design approved — ready for implementation planning
**Scope:** Single-node MVP — Herd manages llama-server processes locally, replacing Ollama

---

## Problem

Ollama is unstable. llama-server is stable and fast (44-80% TTFT improvement) but serves one model per process with no dynamic loading. Users need Ollama's UX (send any model, it loads on demand) with llama-server's performance and stability.

## Solution

Herd v2.0 adds **node mode** — a process manager that spawns/kills llama-server instances on demand, manages VRAM via LRU eviction, and exposes an Ollama-compatible API. One binary replaces both Ollama and herd-tune.

**Architecture target:** Host/node split (`herd --host` for fleet routing, `herd --node` for local model management). This MVP implements node mode only; fleet integration comes in a later sprint.

---

## Architecture

When Herd starts with `node.enabled: true` (or no `[[backends]]` configured), it enters node mode:

```
Client request --> Herd (node mode)
                    |-- Model loaded? --> proxy to llama-server :809X --> response
                    +-- Not loaded?
                        |-- VRAM available? --> spawn llama-server --> grace period --> proxy or 202
                        +-- No VRAM? --> LRU evict oldest --> spawn --> grace period --> proxy or 202
```

Three new components:

1. **ModelInventory** — scans GGUF directories, parses filenames to model names, allows explicit name overrides. Knows what's available on disk.

2. **ProcessManager** — spawns/kills llama-server processes. Tracks PID, port, model name, VRAM estimate, last request time. Health checks each child process.

3. **VRAMAllocator** — tracks total VRAM (nvidia-smi or config), current usage (sum of loaded model sizes), decides what to evict when full.

These sit alongside the existing `BackendPool` — each running llama-server process is registered as a dynamic backend, so existing routing strategies work unchanged.

---

## Model Inventory

### Directory scanning

On startup, Herd scans configured directories for `.gguf` files:

```yaml
node:
  enabled: true
  model_dirs:
    - "~/.herd/models"
    - "G:/models"
  models:
    "qwen2.5-coder:32b": "G:/models/qwen2.5-coder-32b-instruct-q4_k_m.gguf"
    "classifier": "G:/models/qwen3-1.7b-q8_0.gguf"
```

### Filename to model name parsing

Best-effort parser extracts model name from GGUF filenames. Common patterns:

- `qwen2.5-coder-32b-instruct-Q4_K_M.gguf` -> `qwen2.5-coder:32b`
- `gemma-4-26B-A4B-it-UD-Q4_K_M.gguf` -> `gemma-4:26b`
- `Meta-Llama-3.1-8B-Instruct-Q6_K.gguf` -> `llama3.1:8b`

The parser strips quant suffixes and normalizes to `name:size` format. Explicit `models:` overrides take precedence for cases where the parser can't handle the filename.

### Rescan

Herd rescans directories periodically (every 60s, configurable via `node.scan_interval_secs`) and on `POST /admin/reload`. New files appear in inventory; deleted files are removed. Running processes for deleted files continue serving until evicted normally.

### Ollama blob integration

The existing `src/blob.rs` extraction feeds into model_dirs. User extracts a GGUF to `~/.herd/models/`, next scan picks it up.

---

## Process Manager

### Spawning

When a model needs to load, ProcessManager:

1. Picks the next available port (starting at 8090, incrementing, skipping any in use)
2. Resolves the GGUF path from ModelInventory
3. Spawns: `llama-server -m {path} -ngl 99 -c {context} --port {port} --host 127.0.0.1`
4. Polls `http://127.0.0.1:{port}/health` until it returns `{"status":"ok"}` (or timeout)
5. Registers as a dynamic backend in the existing BackendPool

### llama-server binary discovery

Search order:
1. Config: `node.llama_server_path`
2. `~/.herd/bin/llama-server` (herd-tune download location)
3. `$PATH` lookup

Herd refuses to start node mode if no binary is found.

### Context length

Default 4096, configurable per-model in the model registry or globally via `node.default_context_len`. VRAMAllocator uses this to estimate memory needs.

### Killing

When LRU eviction selects a process:

1. Send SIGTERM (Unix) or `taskkill` (Windows)
2. Wait up to 5 seconds for graceful shutdown
3. SIGKILL / force kill if still running
4. Remove from BackendPool
5. Free the port

### Health monitoring

Each child llama-server gets polled every 10 seconds via `/health`. If a process dies unexpectedly (crash, OOM), ProcessManager detects it on the next poll, removes it from the pool, and logs a warning. No auto-restart — the next request for that model triggers a fresh spawn.

### State tracking

```rust
struct ManagedProcess {
    pid: u32,
    port: u16,
    model_name: String,
    gguf_path: PathBuf,
    vram_estimate_mb: u64,
    started_at: Instant,
    last_request: Instant,
}
```

---

## VRAM Allocator

### Detection

On startup, Herd detects total VRAM:

1. Parse `nvidia-smi --query-gpu=memory.total --format=csv,noheader,nounits`
2. If nvidia-smi unavailable, fall back to config: `node.vram_total_mb: 32768`
3. If neither, refuse to start node mode

### Estimation

```
vram_mb = (file_size_mb * 1.2) + (context_len / 1024 * 0.5 * param_billions)
```

The 1.2x multiplier accounts for KV cache and compute buffers at default context length. Real usage is observable via llama-server's `/metrics` endpoint after loading — Herd can adjust its tracking.

### Eviction

When a new model needs `N` MB and `available < N`:

1. Sort running processes by `last_request` (oldest first)
2. Accumulate VRAM from eviction candidates until `freed >= N`
3. Kill those processes
4. If evicting everything still isn't enough, return error ("model too large for available VRAM")

### Reservation

While a model is loading (between spawn and healthy), its estimated VRAM is reserved so concurrent requests don't double-book.

---

## Request Flow and Grace Period

When a request arrives for model X:

```
1. Is model X running? (check BackendPool)
   -> YES: proxy immediately (fast path)
   -> NO: continue

2. Is model X in inventory? (check ModelInventory)
   -> NO: return 404 {"error": "Model 'X' not found"}
   -> YES: continue

3. Start loading model X (async):
   - VRAMAllocator checks space, evicts if needed
   - ProcessManager spawns llama-server
   - Polls /health until ready

4. Grace period (configurable, default 10s, node.grace_period_ms):
   - Hold the request, waiting for model to become healthy
   - If healthy within grace period:
     -> proxy the request, return response
     -> add X-Herd-Cold-Load: true, X-Herd-Load-Time-Ms: {ms} headers
   - If grace period expires:
     -> return 202 with:
        {"status": "loading", "model": "X", "retry_after_secs": 5}
        Retry-After: 5 header
     -> loading continues in background
```

### Concurrent requests

Only one spawn per model. If a second request arrives while model X is loading, it joins the same grace period wait. No duplicate processes.

---

## Ollama-Compatible API Surface

For drop-in Ollama replacement, node mode serves these endpoints:

| Endpoint | Behavior |
|----------|----------|
| `POST /api/generate` | Translate to `/v1/completions` on target llama-server, translate response back to Ollama format |
| `POST /api/chat` | Translate to `/v1/chat/completions`, translate response back |
| `GET /api/tags` | Return all models from ModelInventory (available on disk), not just loaded |
| `GET /api/ps` | Return currently running llama-server processes with model name, VRAM usage, idle time |
| `POST /api/pull` | Download GGUF from HuggingFace (existing search API) into model_dirs |
| `DELETE /api/delete` | Delete GGUF file from disk, kill process if running |
| `GET /api/show` | Return model metadata parsed from GGUF header (param count, quant, context length) |

### Translation layer

llama-server speaks OpenAI format. Ollama clients speak Ollama format. Herd translates:

- Ollama `prompt` field -> OpenAI `messages` with single user message
- Ollama `options.temperature` -> OpenAI `temperature`
- Ollama response `eval_count`, `prompt_eval_duration` etc. -> computed from OpenAI `usage` + request timing
- Streaming: Ollama NDJSON -> translated from OpenAI SSE

This means Open WebUI, LiteLLM, agents — anything that talks to Ollama — works unchanged.

---

## Configuration

```yaml
node:
  enabled: true                              # false by default
  llama_server_path: null                    # null = auto-discover (PATH, ~/.herd/bin)
  model_dirs:
    - "~/.herd/models"
  models: {}                                 # explicit name -> path overrides
  vram_total_mb: 0                           # 0 = auto-detect via nvidia-smi
  default_context_len: 4096
  grace_period_ms: 10000                     # how long to hold requests while loading
  scan_interval_secs: 60                     # how often to rescan model_dirs
  base_port: 8090                            # first port for llama-server processes
  health_poll_secs: 10                       # health check interval per child process
  extra_args: []                             # additional llama-server flags (e.g., ["-ngl", "99"])
```

All fields have sensible defaults. An empty `node:` section with just `enabled: true` works if llama-server is on PATH and GGUFs are in `~/.herd/models`.

---

## File Structure

```
src/
  node/                    # NEW
    mod.rs                 # NodeManager: top-level coordinator, startup, request flow
    inventory.rs           # ModelInventory: GGUF scanning, name parsing, explicit overrides
    process.rs             # ProcessManager: spawn/kill llama-server, health polling, port allocation
    vram.rs                # VRAMAllocator: detection, estimation, LRU eviction
    ollama_api.rs          # Ollama-format API translation (generate, chat, tags, ps, pull, delete)
  config.rs               # MODIFY — add NodeConfig struct
  server.rs               # MODIFY — detect node mode, mount node routes, integrate with request flow
  lib.rs                   # MODIFY — add pub mod node;
```

Existing modules unchanged: BackendPool, Router, Metrics, Analytics, classifier_auto. Node mode registers managed processes as dynamic backends in the existing pool.

---

## What's NOT in this MVP

- `--host` / `--node` CLI flags (detect from config instead)
- Fleet routing to remote node-mode instances
- Auto-download models from HuggingFace on first request
- mDNS discovery between nodes
- Distributed VRAM allocation across fleet
- Dashboard model management UI for node mode
- llama.cpp RPC tensor-parallel sharding
- AMD/Intel VRAM detection (nvidia-smi only for MVP, config fallback for others)

---

## Success Criteria

1. `herd` with `node.enabled: true` and GGUFs in `~/.herd/models` serves requests — no Ollama needed
2. First request for an unloaded model triggers llama-server spawn, responds within grace period or returns 202
3. Subsequent requests for the same model are fast (llama-server already running)
4. When VRAM is full, requesting a new model evicts the least-recently-used model
5. `GET /api/tags` shows all available GGUFs; `GET /api/ps` shows running processes
6. Open WebUI pointed at Herd works the same as it did with Ollama
7. Auto mode classifier works with node-managed models
