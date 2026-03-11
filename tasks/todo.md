# Herd — Current Status

**Version:** 0.4.2
**Branch:** main
**Last pushed:** 2026-03-11

## Current Task: Routing & Resilience Fixes

### Issues
1. **ModelAware hot-spotting** — `get_by_model` picks highest-priority backend regardless of load, starving other backends
2. **404 not retried** — Proxy retry loop only retries on network errors; a 404 from Ollama (model evicted) is treated as success and proxied through

### Plan
- [x] Analyze proxy retry loop and model_aware router
- [ ] Fix #1: `get_by_model` + `get_by_model_tagged` — when multiple backends have the model, prefer least-busy (GPU utilization) instead of always highest-priority
- [ ] Fix #2: Proxy retry loop — treat 404 responses on model endpoints as retryable (don't break, continue to next backend)
- [ ] Update circuit breaker defaults to less aggressive values (5 threshold, 30s recovery)
- [ ] Run tests
- [ ] Commit and push

## Parked: GitHub Sponsors → Herd-Pro Access

1. Set up GitHub Sponsors on `swift-innovate` with a Herd Pro tier
2. Create a GitHub Actions workflow that listens for `sponsorship` events
3. On `created` event → add sponsor as collaborator to `herd-pro` (read access)
4. On `cancelled` event → remove collaborator from `herd-pro`
5. Add release workflow to `herd-pro` for binary distribution
6. Document how sponsors access `herd-pro` (clone, releases, auto-update with token)

## Completed Milestones

### v0.2.1 — Security Hardening
### v0.3.0 — Routing & Reliability
### v0.4.0/v0.4.1 — Observability & Operations
### v0.4.2 — Agent Guide & Skills
