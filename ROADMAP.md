# Herd Roadmap

**Updated:** March 9, 2026

## Vision

Herd is the **fastest way to route AI workloads across local Ollama backends**.

One fast, single Rust binary gives you:
- GPU-aware routing across multiple Ollama nodes
- Circuit breaker resilience with configurable failure thresholds
- Unified observability: metrics, analytics, and a live dashboard
- OpenAI-compatible API for drop-in compatibility

No cloud dependency. No API keys exposed. Full local control.

## Roadmap

### v0.2.1 — Security Hardening

- Configurable circuit breaker (failure threshold, recovery time)
- API key authentication for admin endpoints
- Proxy hardening (body size cap, header forwarding, query string preservation)
- Analytics race condition fix
- CLI backend specification parser
- Conditional route registration (admin API off by default)

### v0.3.0 — Routing & Reliability ✅

- ~~Retry loop with configurable attempt count~~ ✅ (shipped v0.2.1)
- ~~Request timeout enforcement per routing strategy~~ ✅ (shipped v0.2.1)
- ~~Weighted round-robin routing strategy~~ ✅
- ~~OpenAI `/v1/chat/completions` full compatibility layer~~ ✅ (pulled forward from v0.4.0)
- ~~Rate limiting (global token bucket)~~ ✅ (pulled forward from v0.5.0)
- ~~Model filter (regex-based per-backend)~~ ✅
- ~~Dashboard polish (stats, tabs, latency percentiles, mobile responsive)~~ ✅
- ~~Backend tagging and tag-based routing~~ ✅
- ~~Health check endpoint customization (configurable path and expected status)~~ ✅
- ~~Hot-reload configuration without restart~~ ✅ (file polling + POST /admin/reload)

### v0.4.0 — Observability & Operations ✅ (v0.4.1)

- ~~Prometheus-native metrics export~~ ✅ (in-memory counters + histogram, `/metrics` endpoint)
- ~~Request tracing with correlation IDs~~ ✅ (X-Request-Id propagation + UUID v4 generation)
- ~~Log rotation and retention policies~~ ✅ (size-based rotation, configurable retention days)
- ~~Auto-update from GitHub Releases~~ ✅ (`herd --update`, `POST /admin/update`)
- ~~GitHub Actions CI/CD~~ ✅ (test on 3 platforms, release builds for 5 targets)
- ~~Graceful config error handling~~ ✅ (v0.4.1 — warn+disable instead of crash)

### v0.4.3 — Keep-Alive & Hot Models (Breaking)

> **Breaking changes:** `default_model` and `routing.idle_timeout_minutes` are removed. See README migration guide.

- `keep_alive` injection — override `keep_alive` on every proxied Ollama request centrally; prevents clients (Open WebUI, LiteLLM, agents) from accidentally evicting models
- Hot models warmer — `hot_models: [...]` per backend; background warmer pings every 4 min with `keep_alive: "-1"` for pre-load and OOM recovery
- Removes `ModelHoming` and `default_model` — superseded by `hot_models` + proxy injection

### v0.5.0+ — Scale & Ecosystem (Q3 2026)

- Multi-node discovery (mDNS / static fleet config)
- TLS termination
- Rate limiting per client / API key
- Plugin system for custom routing strategies
- Distributed health consensus

## Get Involved

If you're interested in:
- Testing pre-release builds
- Contributing routing strategies or backend integrations
- Sharing real-world deployment patterns

...please open an issue or discussion.

— swift-innovate
