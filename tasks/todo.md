# Herd — Working TODO

> Scratchpad for in-flight work. Milestone tracking in `ROADMAP.md`.

**Last updated:** 2026-07-26

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

## QUEUED — Static Placement Doctrine sprint

Specs: `specs/` (6 files, all gate-PASS). Origin: brainstorming session with Gage;
driven by real Ollama pinning problems on the fleet. Reviewed against the repo
2026-07-26 — spec set revised, see `specs/README.md` § "Revised after a repo review".

Baseline at review time: `cargo check --all-targets` clean, v1.4.0, main @ `4f605e7`.
Every line-number citation in `specs/` was verified against that SHA.

Tree state: dashboard2 Phase 1 is committed (`4f605e7`). The `herd fit` work
(`src/fit/`, `Cargo.toml`/`Cargo.lock`, `src/api/models.rs`, `src/cli.rs`,
`src/daemon/capabilities.rs`, `src/lib.rs`, `src/main.rs`) is still uncommitted and
untouched — land or park it before starting, since this sprint touches `src/router/`,
`src/backend/`, `src/config.rs` and `src/server.rs` broadly and a mixed tree makes
bisecting miserable.

### Sequencing (dependency-ordered; parallelism noted)

- [ ] **#1 node-origin** (S) — `NodeOrigin` enum on `BackendState`, API + `herd status`
      ORIGIN column. Independent; can run parallel to #2.
- [ ] **#2 residency-signal** (S) — `resident_models` from `/api/ps` (all entries, not
      `.first()`). **Prerequisite for #3–#6.** Independent of #1.
- [ ] **#3 pin-retirement** (M) — gate warmer + `inject_keep_alive` off by default,
      add startup unpin sweep. Needs #2. Independent of #4–#6 → parallel with #4.
- [ ] **#4 residency-routing** (M) — `ModelGate::Strict` default, `RouteError`,
      proxy 404/503 JSON contract, dim 1 weight → 0.0. Needs #2.
- [ ] **#5 legacy-router-residency** (L) — residency for Priority/ModelAware/LeastBusy/
      WRR + new pool primitives. Needs #4.
- [ ] **#6 model-classes** (L) — classes, `ModelQuery`, trait signature change, body
      rewrite inside the retry loop. Needs #2, #4, #5.

Parallelisable pairs: (#1, #2) then (#3, #4). #5 and #6 are strictly serial after #4.
Per global policy, ≥2 parallel builders ⇒ one git worktree each, lead-resolved base SHA.

### Why the sequence changed from the specs' original order

Original README ordered 1, 2 → 3 → 4 and sized residency-routing `M`. Review found:

- No true residency signal existed (Ollama `models` = `/api/tags` = on-disk), so the
  hard filter would have permitted request-path loads while passing its own ACs. New
  spec #2 inserted as a prerequisite.
- Warmer retirement covered only half the pinning; `inject_keep_alive` in the request
  path is the sharper violation. Spec rescoped and renamed (`warmer-retirement.md`
  deleted → `pin-retirement.md`).
- Residency routing was two jobs — three routers ignore `model` entirely and need it
  built from scratch plus new pool primitives. Split #4 / #5, #5 sized L.

### Verification gates (per global policy — proof, not claims)

- [ ] Each spec's ACs green before its slice is called done
- [ ] `cargo build && cargo test` clean at every slice boundary
- [ ] AC-as-regression-guard for the residency conflation: a request for a model that
      is on disk but not loaded on an Ollama backend must 404, not dispatch
      (`residency-routing.md` AC10, `legacy-router-residency.md` AC7)
- [ ] Four separate CHANGELOG entries — see `specs/README.md` § "Breaks requiring
      CHANGELOG entries"; do not collapse into one line
- [ ] README migration note: `model_warmer.enabled: true` / `routing.default_keep_alive: "5m"`
      to restore legacy behavior
- [ ] `warmer.rs:21` `.unwrap()` + `:59` `.expect()` removed during #3
- [ ] `weighted_round_robin.rs:61` `.expect()` removed during #5

### Decisions taken (2026-07-26)

- [x] **404 retry budget (#4).** A model-endpoint 404 is placement drift, not transport
      failure: it gets its own budget of **exactly one re-route**, independent of
      `routing.retry_count`, and drops the model from that backend's `resident_models`
      on the way through so the pool self-heals and the re-route can't re-pick it.
      Terminal state is 404 `model_not_resident`, never the generic 502.
      Spec'd in `residency-routing.md` §3a + AC11–AC14.
      Three defects this fixes, all found while spec'ing it:
      (i) all-attempts-fail currently returns **502** (`server.rs:1946-1955`), so the
      404 contract would have been false even with `RouteError` added;
      (ii) the loop is `0..=retry_count` (default 2 → 3 attempts), so both #4's and
      #6's "until candidates exhaust" promises were unachievable;
      (iii) each 404 hop can trigger a *real* request-path load on Ollama — the retry
      path reopened the doctrine hole that the routing fix closes.
- [x] **Class name collisions (#6).** Startup **error**, not silent shadowing. Everyone
      has a different opinion about class taxonomies, so collisions are likely, and
      both silent resolutions are bad (classes-win hides a real model; models-win makes
      resolution depend on what's loaded right now). Erroring hands the call to the
      operator, costs one rename, and keeps runtime resolution deterministic for the
      late-appearing collision validation can't see. Also now explicit + testable:
      Herd ships zero classes and hardcodes zero class names (`model-classes.md` AC14).

### Still open (non-blocking)

- [ ] **#1:** whether the `Enrolled` tier survives at all. The enum makes deprecation
      easy later (delete a variant, compiler finds consumers) — no need to decide
      before #1 lands.

### Note on #6 sequencing

Specs #1–#5 deliver the doctrine in full; #6 is a feature built on top of it. It is
also the L-sized, most opinion-heavy item. If class design attracts debate, ship #1–#5
and let #6 settle separately — nothing in the first five depends on it.

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
