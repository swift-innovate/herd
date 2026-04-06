# CLAUDE.md — Herd

> **For use with Claude Code.** Project-level instructions for the Herd repository.

---

## Project Overview

| Field | Value |
|-------|-------|
| **Repo** | `swift-innovate/herd` (GitHub) |
| **Language** | Rust |
| **Version** | 0.9.0 |
| **Purpose** | Intelligent Ollama router — GPU-aware routing, circuit breakers, OpenAI compat, agent sessions, fleet management, dashboard |

Herd is a single-binary reverse proxy for Ollama backends. It routes AI workloads across local GPU nodes with model awareness, health tracking, and observability.

As of v0.9.0, the former private "Herd Pro" repository has been merged into this repo. **There is only one Herd repo now.**

## Architecture

- **Framework:** Axum 0.7, Tokio async runtime
- **State:** `Arc<RwLock<...>>` for shared mutable state, `AtomicU64`/`AtomicU32` for lock-free config values
- **Routing:** 4 pluggable strategies (priority, model_aware, least_busy, weighted_round_robin)
- **Persistence:** JSONL for analytics/audit, SQLite (`rusqlite`) for node registry
- **Config:** YAML with hot-reload via file watcher (30s) or `POST /admin/reload`

### Module Map

| Module | Purpose |
|--------|---------|
| `src/server.rs` | AppState, route registration, proxy handler, middleware |
| `src/config.rs` | All config structs, YAML parsing, validation |
| `src/router/` | 4 routing strategies + Router trait |
| `src/backend/` | BackendPool, HealthChecker, ModelDiscovery, ModelWarmer |
| `src/agent/` | Session management, tool execution, permissions, audit, WebSocket |
| `src/nodes/` | SQLite node registry, health polling, herd-tune integration |
| `src/api/` | Admin CRUD, OpenAI compat, agent endpoints, node endpoints |
| `src/classifier.rs` | Task-based tier classification middleware |
| `src/analytics.rs` | JSONL request logging with rotation |
| `src/metrics.rs` | In-memory Prometheus metrics |

## Build & Test

```bash
cargo build          # Debug build
cargo test           # 111 tests (unit + integration)
cargo build --release  # Release build
```

## Code Quality Rules

- All new features default to `enabled: false` — zero overhead when not opted in
- All new config fields must have sensible defaults and not break existing `herd.yaml` files
- New endpoints must appear in `skills.md` and the dashboard Agent Guide tab
- JSONL analytics logging must be extended (not replaced) with new fields
- Tests for each feature — at minimum: enabled/disabled, happy path, edge cases
- All public-facing headers use the `X-Herd-` prefix
- **Never bail! on config errors** — degrade gracefully, warn+disable features

## Commit Format

Use conventional commits: `feat:`, `fix:`, `chore:`, `refactor:`, `docs:`, `test:`

## Roadmap

See `ROADMAP.md`. Next milestone targets: budget caps, routing profiles, multi-model consensus.
