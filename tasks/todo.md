# Herd — Current Status

**Version:** 0.4.1
**Branch:** main
**Last pushed:** 2026-03-09

## Completed Milestones

### v0.2.1 — Security Hardening ✅
Circuit breaker, API key auth, proxy hardening, rate limiting

### v0.3.0 — Routing & Reliability ✅
Weighted round-robin, tag-based routing, health check config, hot-reload, OpenAI compat

### v0.4.0/v0.4.1 — Observability & Operations ✅
- Prometheus metrics (`/metrics` endpoint, in-memory counters + latency histogram)
- Correlation IDs (X-Request-Id propagation)
- Log rotation (size-based, configurable retention)
- Auto-update (`herd --update`, `POST /admin/update`, startup check)
- GitHub Actions CI (test 3 platforms, clippy, fmt) + Release (5 target builds)
- v0.4.1 fix: graceful config error handling (no more restart loops)

**Test coverage:** 37 tests
**CI status:** GitHub Actions running on push/PR + release on tag

## Next: v0.5.0 — Scale & Ecosystem

| Item | Complexity | Notes |
|------|-----------|-------|
| Multi-node discovery (mDNS / static fleet) | High | Auto-discover Ollama nodes |
| TLS termination | Medium | rustls integration, addresses SECURITY-REVIEW finding |
| Per-client rate limiting | Medium | Extend token-bucket to per-API-key |
| Plugin system for custom routing | High | Trait-based or WASM |
| Distributed health consensus | High | Gossip protocol |

## Key Files
- `src/server.rs` — AppState, proxy handler, all HTTP handlers
- `src/api/openai.rs` — OpenAI-compatible endpoints
- `src/router/` — 4 routing strategies (priority, model_aware, least_busy, weighted_round_robin)
- `src/metrics.rs` — Prometheus metrics
- `src/updater.rs` — Auto-update from GitHub Releases
- `src/analytics.rs` — JSONL logging + log rotation
- `src/config.rs` — YAML config with serde defaults
- `.github/workflows/` — CI + Release workflows
