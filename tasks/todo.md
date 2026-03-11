# Herd — Current Status

**Version:** 0.4.1
**Branch:** main
**Last pushed:** 2026-03-09

## Current Task: GitHub Sponsors → Herd-Pro Access

### Plan
1. Set up GitHub Sponsors on `swift-innovate` with a Herd Pro tier
2. Create a GitHub Actions workflow that listens for `sponsorship` events
3. On `created` event → add sponsor as collaborator to `herd-pro` (read access)
4. On `cancelled` event → remove collaborator from `herd-pro`
5. Add release workflow to `herd-pro` for binary distribution
6. Document how sponsors access `herd-pro` (clone, releases, auto-update with token)

### Status
- [ ] Research GitHub Sponsors API (in progress)
- [ ] Set up sponsor tier
- [ ] Create sponsorship webhook workflow
- [ ] Add release workflow to herd-pro
- [ ] Update herd-pro auto-update to support auth tokens
- [ ] Documentation

## Completed Milestones

### v0.2.1 — Security Hardening ✅
### v0.3.0 — Routing & Reliability ✅
### v0.4.0/v0.4.1 — Observability & Operations ✅

## Next: v0.5.0 — Scale & Ecosystem
Multi-node discovery, TLS, per-client rate limiting, plugins
