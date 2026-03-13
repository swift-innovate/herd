# Design: keep_alive Injection + Hot Models Warmer

**Date:** 2026-03-13
**Status:** Approved
**Target:** v0.4.3

---

## Problem

Ollama unloads models from VRAM after 5 minutes of inactivity by default. Clients like Open WebUI, LiteLLM, and agent frameworks often send `"keep_alive": "5m"` in the request body, which overrides any environment variable set on the node. The result: models get evicted mid-swarm, causing cold-start latency on the next request.

Herd is the ideal place to fix this centrally — it already buffers the request body for model extraction and proxies every request.

---

## Breaking Changes

> **⚠️ These changes remove two existing config fields.**

| Removed field | Migration |
|---|---|
| `backends[].default_model` | Replace with `backends[].hot_models: ["model:tag"]` |
| `routing.idle_timeout_minutes` (model_homing) | Replace with `model_warmer.interval_secs: 240` |

Existing YAML files with these fields will silently ignore them after upgrading (serde default behavior). **No startup error will occur, but the homing behavior will stop working silently.** Users must migrate their config manually.

---

## Design

### 1. Config Shape

```yaml
routing:
  default_keep_alive: "-1"     # injected into every Ollama request; default: "-1"

model_warmer:
  interval_secs: 240           # ping interval; default: 240 (4 min)

backends:
  - name: citadel
    url: http://citadel:11434
    hot_models:                # replaces default_model
      - llama3:8b
      - codellama:7b
```

- `routing.default_keep_alive` — global, applies to all Ollama requests
- `model_warmer.interval_secs` — how often the warmer pings each hot model
- `backends[].hot_models` — per-backend list of models to keep warm

### 2. keep_alive Proxy Injection

In `proxy_handler`, for Ollama-native endpoints only (`/api/generate`, `/api/chat`):

1. Parse `body_bytes` as JSON (already done for model extraction)
2. Set `payload["keep_alive"] = config.routing.default_keep_alive`
3. Re-serialize to `forward_bytes`
4. Use `forward_bytes` in retry loop instead of `body_bytes`

If the body is not valid JSON, skip injection and forward unchanged. `/v1/*` endpoints are not modified (OpenAI format has no `keep_alive` field).

`default_keep_alive` is read once per request via `state.config.read().await` — the same read lock used by other handlers. Hot-reload updates it automatically on the next request.

### 3. ModelWarmer (replaces ModelHoming)

New file: `src/backend/warmer.rs`

```
ModelWarmer { interval: Duration, client: reqwest::Client (30s timeout) }

spawn(pool):
  every interval:
    for each backend in pool.all():
      for each model in backend.config.hot_models:
        POST /api/generate {"model": "...", "prompt": "", "keep_alive": "-1"}
        on error: warn + continue
```

Key behaviors:
- **No idle check** — fires unconditionally. With `keep_alive: "-1"` injected at the proxy, models stay loaded after first use; the warmer handles pre-load and OOM recovery only.
- **Per-model independence** — a slow or failing model ping does not block others.
- **Fire-and-forget** — each ping is spawned independently, not awaited in sequence.

### 4. Removals

| What | Where |
|---|---|
| `ModelHoming` struct | `src/model_homing.rs` — deleted |
| `Config::model_homing` block | `src/config.rs` |
| `Backend::default_model` field | `src/config.rs` |
| `ModelHoming::spawn()` call | `src/server.rs` |

### 5. Server Wiring

- Remove `ModelHoming::spawn()` from `Server::run()`
- Add `ModelWarmer::spawn(pool)` after `ModelDiscovery::spawn()`
- `reload_config()` does not need to restart the warmer — it reads `hot_models` directly from pool backend configs each tick

---

## Tests

| Test | What it verifies |
|---|---|
| `keep_alive_injected_on_api_generate` | Valid JSON body gets `keep_alive` set; re-serialized correctly |
| `keep_alive_not_injected_on_v1_path` | `/v1/chat/completions` body passes through unchanged |
| `keep_alive_passthrough_on_invalid_json` | Binary/malformed body forwarded unchanged, no error |
| `warmer_sends_to_all_hot_models` | With mock pool, verify correct URLs and payloads constructed |

---

## Migration Guide

### Before (v0.4.2)

```yaml
routing:
  idle_timeout_minutes: 30

backends:
  - name: citadel
    url: http://citadel:11434
    default_model: "llama3:8b"
```

### After (v0.4.3)

```yaml
routing:
  default_keep_alive: "-1"

model_warmer:
  interval_secs: 240

backends:
  - name: citadel
    url: http://citadel:11434
    hot_models:
      - llama3:8b
```
