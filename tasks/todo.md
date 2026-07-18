# Herd — Working TODO

> Scratchpad for in-flight work. Milestone tracking in `ROADMAP.md`.

**Last updated:** 2026-07-17

(Previous ACTIVE section — `herd fit` model fit estimator — complete, uncommitted per
dashboard-redesign task doc; folds into Phase 2 below.)

---

## ACTIVE — Dashboard redesign, Phase 1 (shell + read-only screens)

Full brief: `docs/specs/dashboard-redesign-brief.md`. Build plan: `tasks/HERD-DASHBOARD-REDESIGN.md`.
Design source: `Dashboard redesign brief-handoff.zip` (extracted to scratchpad) — Ember direction
(1b) is final: Space Grotesk + JetBrains Mono, warm charcoal (#151210/#08090b), brand amber
#F0A028 chrome-only, health colors #7BC96F/#E3A13C/#F26D6D/#B48EE0 distinct from brand amber.

Ships behind `/dashboard2` (or `?v=2`), old `dashboard.html` untouched until parity + swap.

### Pre-build decision (confirmed, proceeding)

- **File structure:** `build.rs` concat, not hand-authored single file. `src/dashboard2/*`
  partials (design-system.css, shell.html/js, one file per screen) concatenated at compile
  time into `$OUT_DIR/dashboard2.html`, `include_str!(concat!(env!("OUT_DIR"), ...))`. No
  new runtime dependency, no separate toolchain — `cargo build` is still the only command.
  Reason: current dashboard.html (171,180 bytes, confirmed) is the hand-grown-single-file
  anti-pattern this redesign exists to escape; reproducing it verbatim for v2 bakes the same
  problem back in.

### Endpoint map verified against src/api/ + src/server.rs (2026-07-17)

Task doc's map holds. Additions the task doc didn't mention, useful during build:
- `/admin/backends/*` (list/get/put/delete, `:name/models`, `:name/pull`) — raw backend CRUD,
  separate from the `/admin/config/backends/*` overlay. Current Backends tab likely reads both;
  confirm which Fleet merge should use before wiring (probably `/status` + `/admin/config/backends`
  per task doc, since that's the *effective* config).
- `/analytics/agent` — agent-specific analytics, not in task doc's Analytics screen list.
- `/admin/update` (self-update trigger) — may partially satisfy Phase 2's "publish gateway's own
  binary" need (5e). Check before building a new endpoint.
- `/api/budget`, `/api/frontier/costs`, `/metrics` — not mapped to any frame; low priority, note
  for Settings or Analytics if there's a natural home.
- Join key for Fleet dedup confirmed: `nodes::Node.backend_url` / `.node_id` vs backend-pool
  `url`/`name` from `/status`. `Node.source` field distinguishes static/agent-registered.

### Design frames note

The handoff zip's `Herd Structure C.dc.html` contains iteration history, not just finals.
**Turn 5 (Ember) is the approved final skin** — but it only re-skins 5 frames: 5a (Fleet),
5b (Node detail), 5c (Fit grid), 5d (Tasks), 5e (First-run hero/empty state). Two Phase-2
frames — **Add Node modal** and **Update fleet confirm** — exist only in Turn 4's neutral
(pre-Ember) palette; they never got the amber treatment. Not a Phase 1 blocker (both are
Phase 2, backend-gated), but flag before building them — apply Ember tokens by pattern-matching
5a-5e rather than copying Turn 4's neutral colors verbatim.

### Tasks

- [ ] `build.rs`: concat `src/dashboard2/*` into `$OUT_DIR/dashboard2.html`, rerun-if-changed
- [ ] `src/dashboard2/design-system.css`: Ember tokens (color vars, type scale, spacing), component
      set (cards, tables, badges, buttons, modals, toasts, empty states), health-color semantics
- [ ] Shell: sidebar IA (Fleet/Models/Analytics/Sessions/Tasks/Settings + badges), persistent
      header (summary stats from `/status` + `/analytics`), wordmark + mark, connection/refresh
      indicator, API key entry + localStorage + 401 states
- [ ] Wire `/dashboard2` route in `src/server.rs` (parallel to existing `/dashboard`, both live
      until swap)
- [ ] **Fleet table** (frame 5a) — the key frontend work: merge `/api/nodes` + `/status`+
      `/admin/config/backends` into one row set, dedup by `node_id`/`backend_url` identity,
      all 4 health states, degraded/no-telemetry row state (honest gaps, never fake zeros)
- [ ] Node detail page (frame 5b) — Telemetry tab, Models & Routing tab (allowlist, hot models,
      missing badges), Activity tab
- [x] Analytics screen — Chart.js timeline, p50/p95/p99, model/backend counts, hours selector
- [x] Sessions screen — list + modal + WebSocket stream
- [x] Settings screen — config editor w/ secret redaction, overrides table, model routing, Agent Guide
- [ ] All poll/loading/401/unreachable/stale states across screens
- [ ] Parity checklist (`tasks/HERD-DASHBOARD-REDESIGN.md`) fully green before swap

### Added during build (deviations/discoveries)

- Settings' config editor mutates the *whole* `GET /admin/config` object in place and
  PUTs it back whole, touching only the sub-trees the form has UI for. The old dashboard's
  `buildConfigFromForm()` rebuilt the config from scratch each save, which would have
  silently reset `routing.auto`, `routing.scored` (22 scorer weights), `routing_profiles`,
  `tls`, `rate_limiting`, `budget`, `discovery`, `fleet`, `frontier`, `providers`,
  `task_classifier`, and `agent.permissions` to their defaults on every save — confirmed
  live against the real gateway's config (it has real values in all of these). Verified
  the fix: edited `server.rate_limit`, saved, and diffed `routing.scored.weights` /
  `fleet` / `frontier` / `routing_profiles` before and after — byte-identical.
- Routing strategy dropdown needed a 5th option, `scored` (`RoutingStrategy::Scored`,
  added by the scorer work after PR #G3 shipped) — without it the select renders blank
  for any gateway actually running the scored strategy (this repo's `herd.yaml` is one),
  and saving would send an invalid empty string. Old dashboard has the same bug,
  unfixed there (out of scope — not touching the retiring file).
- `agent.permissions` and other no-UI leaf objects are left untouched on the mutated
  config object rather than hardcoded to empty (old dashboard's `buildConfigFromForm()`
  always sent `{deny_file_patterns: [], deny_bash_patterns: [], allow_shell_commands: false}`
  regardless of existing values — a second instance of the same reset-on-save bug class).

---

## BACKLOG — Dashboard redesign, Phase 2 (control plane)

Blocked on backend pieces. Do not start until Phase 1 hits parity. See
`tasks/HERD-DASHBOARD-REDESIGN.md` for full detail — agent-owns-llama-server-lifecycle first
(unblocks everything else), then commit `herd fit` + fit endpoint, heartbeat task envelope,
join tokens + `herd agent --install`, publish-from-UI, registration-path collapse.

---

## Housekeeping (fold in while touching the repo)

- [ ] Version/test-count drift: Cargo says 1.4.0, CLAUDE.md 1.2.0, DECK.md "v1.1.0 / 258 tests",
      CLAUDE.md 321, todo said 650. Reconcile.
