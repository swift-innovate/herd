# Herd — Current Status

**Version:** 0.4.2
**Branch:** ollama-features
**Last pushed:** 2026-03-11

## Current Task: Ollama Node Management Features

### Features
1. **VRAM detection** — Passive: on each discovery cycle, read `memory_total` from gpu-hot telemetry (port 1312) and store as `vram_total_mb` on BackendState. Active probe via model pull was discarded (would pull 2GB+ model on every new backend, breaking air-gapped nodes and adding latency).
2. **Model listing in Edit modal** — Surface all models on the node, with delete buttons (calls Ollama `DELETE /api/delete`)
3. **Model pull UI** — Text input + "Pull" button in Edit modal, calls Ollama `POST /api/pull` with streaming progress

### Plan
- [x] Add `vram_total_mb` field to BackendState and `vram_populated` flag
- [x] Populate VRAM passively from gpu-hot telemetry (`memory_total > 0` guard prevents locking in zero on init)
- [x] Add admin API endpoints: `GET /admin/backends/:name/models`, `POST /admin/backends/:name/pull`, `DELETE /admin/backends/:name/models/:model`
- [x] `pull_model` uses dedicated `mgmt_client` (1h timeout) — avoids shared client's 120s circuit-breaker timeout silently capping large pulls
- [x] Update Edit modal in dashboard: show model list with delete buttons, add pull input with progress (DOM-built to prevent XSS)
- [x] Run tests
- [x] Commit

### Ollama API Reference
- `GET /api/tags` — list models (already used)
- `GET /api/ps` — running models with `size_vram` field
- `POST /api/pull` — `{"name":"model"}`, streams `{"status":"...","total":N,"completed":N}`
- `DELETE /api/delete` — `{"name":"model"}`
- `POST /api/generate` — `{"model":"...","prompt":"..."}` for VRAM test

## Next Task: v0.4.3 — keep_alive Injection + Hot Models Warmer

Spec: `docs/superpowers/specs/2026-03-13-keep-alive-hot-models-design.md`

### Plan
- [x] Add `default_keep_alive: String` to `RoutingConfig` (default `"-1"`)
- [x] Add `ModelWarmerConfig { interval_secs: u64 }` to `Config` (default 240)
- [x] Add `hot_models: Vec<String>` to `Backend`, remove `default_model`
- [x] Inject `keep_alive` in proxy_handler for `/api/generate` + `/api/chat`
- [x] Write `src/backend/warmer.rs` (ModelWarmer)
- [x] Delete `src/model_homing.rs`, remove ModelHoming from server.rs + config.rs
- [x] Add 4 unit tests (see spec)
- [x] Update skills.md with new config fields
- [x] Commit + tag v0.4.3

## Parked: GitHub Sponsors → Herd-Pro Access
## Completed: v0.2.1, v0.3.0, v0.4.0/v0.4.1, v0.4.2
