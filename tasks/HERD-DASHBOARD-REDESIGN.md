# Herd Dashboard Redesign — Build Task

**Repo:** `swift-innovate/herd`
**Author:** Tom Swift (Director) + Gage
**Date:** 2026-07-17
**Design source:** `docs/specs/dashboard-redesign-brief.md` + six approved frames (Claude Design, Ember/amber system)
**Status:** Ready to build

---

## What this is

A ground-up redesign of `dashboard.html` (currently a 171KB hand-grown single file, 7 tabs accreted over 14 releases). The design was done fresh in Claude Design and approved across five iteration turns. This doc is the handoff: it maps every approved frame to its backend endpoints, splits the work into what ships against today's API vs. what needs backend first, and carries the parity checklist so nothing from the current dashboard is lost.

**Read the brief first** (`dashboard-redesign-brief.md`) for the full identity rules, constraints, and feature inventory. This doc is the implementation plan on top of it.

## Non-negotiable constraints (from the brief)

- **Single static HTML file**, vanilla JS, CSS in `<style>`, Chart.js from CDN, embedded in the binary via `include_str!`. No build step, no framework runtime.
- **Dark-first**, Ember/amber accent system (see "Accent discipline" below).
- **Data is polled** (5s status, 30s analytics, 10s sessions) except agent-session events (WebSocket). Every screen needs loading / gateway-unreachable / 401 / stale states.
- **Admin API key** gates mutating actions; UI reads cleanly in both read-only and admin modes.
- **The llama appears once, at the top, and never speaks.** No ranch theme. Personality only in empty/error states.

## Accent discipline (the one rule that must survive implementation)

Ember/amber is **chrome** — wordmark, primary buttons, hot-model star, activity/progress. **Green/amber/red are reserved for health semantics** (healthy / degraded / offline / circuit-open) and live only in the status dot and status columns. The two never collide because they're disambiguated by **position, shape, and weight**:

- Health colors ride glyphs (status dot, ✓/◐/✗ verdict marks) — never fills.
- Brand amber appears as fills (buttons) and the hot-star — never in a status column.
- Degraded-amber is desaturated vs. brand amber and only ever a glyph.
- Fit-grid proof: offload verdict = amber *text on the ◐ glyph*; Install button = amber *fill*. Different position/shape/weight → no confusion.

If any implementation choice would put brand amber into a health column or a health color into a button fill, it's wrong.

---

## Information architecture (locked)

Sidebar nav replaces the 7-tab bar. **Nodes are first-class objects**, not database views. The current Backends/Fleet split (routing pool vs. node registry — same GPUs, two tabs) collapses into one Fleet view.

```
┌────────────┬──────────────────────────────────────────┐
│  [mark]    │                                           │
│  HERD      │                                           │
│  v · scored│         (main pane)                       │
│            │                                           │
│  Fleet  5/7│                                           │
│  Models    │                                           │
│  Analytics │                                           │
│  Sessions 2│                                           │
│  Tasks    3│                                           │
│  Settings  │                                           │
│            │                                           │
│  ● conn 4s │                                           │
│  key ·admin│                                           │
└────────────┴──────────────────────────────────────────┘
```

**Persistent header** (in main pane, not sidebar): summary stats — nodes online/total, healthy backends, VRAM used/total, tok/s, req 24h — plus Update fleet + Add node actions on Fleet.

Sidebar item count semantics must be consistent: Fleet badge = online/total (define online = healthy|online; degraded/offline excluded). Sessions/Tasks badges = active count.

---

## Frame → endpoint map

Six approved frames. Legend: ✅ endpoint exists · 🔨 needs backend work · 🟡 partial.

### Frame 5a — Fleet overview (dense table)

The landing screen. One row per physical node, merging Backends + Fleet. Columns: node (+ health dot), backend type, VRAM bar (used/total), util, temp, models (+overflow count), agent version. Row actions on hover (restart / disable / …). Filter box appears at scale (shown holding 12 nodes без scroll). Health legend footer.

| Element | Source | Status |
|---|---|---|
| Node list (agent-registered) | `GET /api/nodes` | ✅ |
| Node list (static/enrolled) | `GET /status` (router pool) + `GET /admin/config/backends` | ✅ |
| **Merge both into one row set** | frontend join on node identity | 🔨 frontend logic |
| GPU VRAM/util/temp | `GET /status` (gpu-hot) + agent telemetry in node record | ✅ 🟡 (may be absent → degraded state) |
| Health states (healthy/degraded/offline/circuit-open) | `GET /status` + circuit breaker state | ✅ |
| Agent version + update-pending | node record `agent_version` vs `fleet.target_agent_version` | ✅ |
| Summary stats header | `GET /status` + `GET /analytics` | ✅ |
| Add node action | → Frame 5e' modal | 🔨 (join tokens) |
| Update fleet action | → Frame (update confirm) | 🟡 (authority exists, no UI) |

**Merge note:** the README already documents that an enrolled node + agent node on the same host coexist as two pool entries (dedup deferred to v1.4). The dashboard must dedup them *in the view* by node identity (node_id + advertised URL) even before the backend does. This is the single most important piece of frontend logic — it's the whole point of the IA.

### Frame 5b — Node detail (full page)

Tabs: **Telemetry** / **Models & Routing** / **Activity**. Header: name, node_id, backend + URL, agent version, uptime, source (static/agent), tags, actions (Restart backend / Install model / Disable / Remove). Telemetry strip: per-GPU VRAM/util/temp/power, queue (slots busy/max), TTFT p50 — each with honest "not reported" when the backend doesn't expose it.

| Element | Source | Status |
|---|---|---|
| Node identity/meta | `GET /api/nodes/:id` | ✅ |
| Telemetry strip | node record + gpu-hot | ✅ 🟡 (honest gaps) |
| Models & Routing: installed list, size, quant, resident state | `GET /admin/config/backends` (models_available) + `GET /api/nodes/:id/models` | ✅ |
| Routing allowlist (checkboxes, all/none/route-all, missing badge) | `PUT /admin/config/backends/:name/models` (`models_enabled`) | ✅ |
| Hot models (pin/unpin) | `PUT /admin/config/backends/:name` (`hot_models`) | ✅ |
| Per-model delete | `DELETE /api/nodes/:id/models/:model` | ✅ (move to overflow menu, confirm) |
| Per-model install → | Frame 5c pre-filtered to node | 🔨 |
| Restart backend / Disable / Remove | `PUT/DELETE /api/nodes/:id`; restart | 🟡 (enable/disable/remove ✅; restart 🔨 task) |
| Activity (requests routed here, errors, circuit events) | `GET /analytics` filtered + circuit state | ✅ 🟡 |

### Frame 5c — Model install / fit grid (THE hero feature)

Models page. HF GGUF search → select model+quant → **per-node fit verdict grid** in the same table shape as Fleet: node, verdict (✓ fits / ◐ offload+layer count / ✗ won't fit), max context estimate, "GB free after", note, install checkbox. Ollama blob-reuse surfaced inline on applicable rows + as a summary nudge. Install-on-N button.

| Element | Source | Status |
|---|---|---|
| HF GGUF search | `GET /api/models/search?q=` | ✅ |
| Quant selection | search result parsing | ✅ |
| **Per-node fit verdict** (fits/offload/won't-fit, max-ctx, GB-free-after) | `herd fit` engine (`src/fit/`) run per-node against live telemetry | 🔨 **not yet an endpoint** — fit module exists, uncommitted, standalone |
| Ollama blob-reuse detection | `GET /api/ollama/models` per node | ✅ |
| Install to node(s) | model download → | 🔨 (task envelope) |
| Ollama pull path | `POST /api/nodes/:id/models/download` | ✅ (Ollama only) |

**Critical dependency:** the fit grid is the differentiator and it depends on `herd fit`, which is (a) uncommitted and (b) not exposed as an endpoint. **Commit it first**, then wire a `POST /api/fit` (or `GET /api/nodes/:id/fit?repo=&quant=`) that runs the fit math against each node's reported hardware/telemetry. Without this, 5c renders empty.

### Frame 5d — Tasks pane

Coarse polled task states: queued → downloading (%, resumable) → verifying → loading → live, or → failed (with recovery affordance, e.g. "Retry when online"). Progress bar = chrome amber (activity, not health). Done = green, failed = red. "Steppers advance only on heartbeat — no fake smooth progress." Completed tasks toast + age out after 24h.

| Element | Source | Status |
|---|---|---|
| Task list + states | heartbeat task envelope | 🔨 **new** |
| Cancel / Retry | task control endpoints | 🔨 **new** |

**This is the substrate piece.** Everything mutating (install, restart, agent update) flows through a heartbeat task envelope: gateway attaches tasks to the heartbeat *response*, agent executes one at a time and reports progress on subsequent heartbeats. Pull-only, firewall-friendly, idempotent by task ID. Generalize the *existing* update-offer mechanism (gateway already attaches update offers to heartbeat responses) into a small task envelope: `install_model`, `remove_model`, `restart_backend`, `update_binary`. This is also the v1.4 speculative/pipeline orchestration substrate — build it well.

### Frame 5e (update confirm) — Fleet update

Confirm dialog: gateway version vs. per-node agent versions table, one button. Then in-flight: the **Agent column on the Fleet table becomes the progress surface** (`updating · verifying sha256`, `✓ updated → 1.2.0`, `skipped — offline`). Nodes never lock — keep serving until their restart step. Offline nodes skipped-and-flagged, not failed.

| Element | Source | Status |
|---|---|---|
| Version authority / publish / self-update | exists (v1.2) | ✅ backend |
| Set target version | `fleet.target_agent_version` (hot-reload) via `PUT /admin/config` | ✅ |
| Publish gateway's own binary | `herd publish` logic | 🟡 (CLI exists; needs an admin endpoint to trigger from UI) |
| Per-node convergence display | node record `agent_version` polling | ✅ |

### Frame 5e' — First-run empty state (README hero)

Fleet pane at 0/0. Three-llama mark (larger, centered), "No nodes in the herd yet", the join one-liner front and center (bash/PowerShell toggle, copy primary), one quiet line. Sidebar present, Fleet-only populated.

| Element | Source | Status |
|---|---|---|
| Empty fleet detection | `GET /api/nodes` + `/status` both empty | ✅ |
| Join one-liner + token | join-token mint | 🔨 **new** |

---

## Backend work required (net-new, in dependency order)

Design draws ahead of the API. Four backend pieces unlock the hero frames:

1. **Commit `herd fit`** — it's uncommitted (per `tasks/todo.md`), standalone, not wired. Prereq for the fit grid. Commit as-is first, then expose.
2. **Fit endpoint** — `POST /api/fit { repo, quant }` → per-node verdicts (fits/offload/won't-fit, max-ctx, GB-free-after) using `src/fit/` math against each node's reported hardware. Powers 5c.
3. **Heartbeat task envelope** — generalize the existing update-offer-on-heartbeat into a task queue: `install_model`, `remove_model`, `restart_backend`, `update_binary`. Agent pulls tasks on heartbeat, executes serially, reports progress on subsequent beats. Powers 5d + install + restart + update. Idempotent by task ID. Gate all task-issuing endpoints behind the admin key.
4. **Join tokens + `herd agent --install`** — one-time enrollment tokens minted from the dashboard (`POST /api/join/token`), served join script (`GET /join/:token`) that downloads the binary from the gateway, runs `herd agent --install` (register systemd/Windows service, gateway URL + token baked in), starts it. Replaces the shared `enrollment_key` (documented 401 trap) and folds herd-tune's GPU-detect/provision into `--install`. Powers 5e' + Add node.

**Prerequisite for #3 and #4:** the agent must own llama-server's lifecycle (spawn/restart/flags), or "restart backend with new model" and "install model then load" have nothing to act on. This is the same prerequisite flagged in the architecture assessment — the agent currently heartbeats next to a backend the user starts by hand. Owning the process is what makes the whole control-plane pay off. **If this isn't done, phase 2 below is blocked** — do it as the first backend task.

## Security guardrails (do not skip)

- Every task-issuing endpoint behind the admin API key. "Dashboard can make every node download a file and restart a process" is a real attack surface.
- Model downloads constrained to HuggingFace + a configured allowlist.
- Join tokens: one-time, short TTL, regenerable. Token embeds in the join command (secret in cleartext at copy time) — copy is the primary affordance; consider masking until copy.
- Current `herd.yaml` ships `api_key: "dev-local-key-change-me"` and `enrollment_key: "warden-fleet"`. Ship the redesign with a first-run nag if the key is still default.

---

## Build phases

### Phase 1 — Shell + read-only screens (ships against today's API)

No backend work. Delivers the new IA and most screens as a real, standalone redesign. Feature-flag behind `/dashboard2` (or `?v=2`) until parity, then swap.

- [x] Sidebar IA + persistent header + summary stats (`/status`, `/analytics`)
- [x] Design system in `<style>`: Ember tokens, type scale, component set (cards, tables, badges, buttons, modals, toasts, empty states), health-color semantics (colorblind-safe, distinct from brand amber)
- [x] Wordmark + mark in header; small-size variant noted for favicon/tray
- [x] **Fleet table** with the Backends+Fleet merge/dedup logic (the key frontend work)
- [x] Node detail page: Telemetry + Models & Routing (allowlist, hot models, missing badges — all endpoints exist) + Activity
- [x] Analytics (Chart.js timeline, p50/p95/p99, model/backend counts, hours selector)
- [x] Sessions (list + modal + WebSocket stream)
- [x] Settings (config editor w/ secret redaction, overrides table, model routing, Agent Guide) — routing_profiles editor UI intentionally out of scope this pass (round-trips unchanged, see decision note below)
- [ ] All poll/loading/401/unreachable/stale states
- [ ] Parity checklist (below) fully green before swap

### Phase 2 — Control plane (needs backend, in order)

Each backend piece unlocks its frame. **Agent-owns-lifecycle first** (unblocks everything).

- [ ] Agent owns llama-server lifecycle (spawn/restart/flags) — **prerequisite**
- [ ] Commit `herd fit`
- [ ] Fit endpoint → wire **Fit grid (5c)**
- [ ] Heartbeat task envelope → wire **Tasks pane (5d)** + install progress + restart
- [ ] Join tokens + `herd agent --install` → wire **Add node modal + empty state (5e')**
- [ ] Publish-from-UI endpoint → wire **Update fleet (5e)** confirm + in-flight
- [ ] Registration-path collapse: fold herd-tune into `--install`, alias/retire standalone enrollment, dedup enrolled+agent at the backend (was v1.4 — pull forward; it's the #1 user-facing confusion)

---

## Parity checklist (nothing from the current dashboard may be lost)

Every current behavior needs a home in the new IA before the old dashboard is retired:

- [ ] Backends + Fleet → merged Fleet view (dedup by identity)
- [ ] Header infra summary (Fleet Nodes Online w/ fallback to Router Backends Online)
- [ ] Gateway version footer, update badge, strategy badge, connection-pulse indicator, refresh countdown
- [ ] Backend cards: health dot, gpu-hot metrics (auto-hide when unreachable), model lists
- [x] Per-backend routing allowlists (`models_enabled`), hot-model selection, "missing" badges (Settings' global Model Routing card + per-node tab in Fleet/Node detail)
- [x] Config overrides table (scope/key/value/updated/delete) + "running pure herd.yaml" empty state
- [x] Settings config editor with secret redaction (GET/PUT `/admin/config`)
- [x] Analytics: totals, p50/p95/p99, model/backend counts, timeline chart, hours selector
- [x] Sessions: list, modal, WebSocket event stream
- [ ] Models: HF GGUF search, VRAM-compat hints (→ becomes the fit grid), download modal, download-to-Ollama-node
- [x] Agent Guide: static API reference content (home: collapsible section at the bottom of Settings)
- [ ] API key entry, localStorage persistence, 401 states throughout
- [ ] Self-enroll command generation → becomes join-token flow
- [ ] Modals close on Escape + overlay-click; toast notifications
- [ ] Auto-refresh: 5s status, 30s analytics, 10s sessions, per-view start/stop, last-view persistence

## Housekeeping (fold in while touching the repo)

- [ ] Version/test-count drift: Cargo says 1.4.0, CLAUDE.md 1.2.0, DECK.md "v1.1.0 / 258 tests", CLAUDE.md 321, todo 650. Reconcile.
- [ ] `Agent Guide` content and `skills.md` stay in sync with any endpoint changes (repo rule: new endpoints appear in both).

---

## Definition of done

- Phase 1: new dashboard reaches full parity, swapped in as the default `dashboard.html`, old file removed. Single static file, `include_str!`'d, no build step.
- Phase 2: each hero frame (fit grid, Tasks, Add node, Update fleet) live against its backend, admin-key-gated, HF-allowlisted.
- The empty state is the README hero shot.
- Accent discipline holds: no brand amber in health columns, no health color in button fills.
