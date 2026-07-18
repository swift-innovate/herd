# Handoff — Dashboard redesign, Phase 1 (dashboard2)

**Base commit SHA:** `a6cf4da9d10ab4c6f16ad04d5a2153302a5587e1` (main, HEAD — re-verified at the start of this Settings slice, still current; no commits have landed on `main` since this branch of work started, so the SHA has been stable across every session in this handoff)
**Working tree:** dirty — new/modified files below are uncommitted. Note: `git status` also
shows unrelated uncommitted changes (`src/fit/`, `Cargo.toml`/`Cargo.lock`, `src/api/models.rs`,
`src/cli.rs`, `src/daemon/capabilities.rs`, `src/lib.rs`, `src/main.rs`) from the prior `herd fit`
model-fit-estimator work (see `herd-fit-estimator` memory) — pre-existing, not touched this
session, left as-is.

## Settings screen added (this session, on top of the Sessions slice below)

Built `src/dashboard2/settings.js` (already listed in `build.rs`'s `PARTS`, no build.rs edit
needed) — the last Phase 1 screen. Four sections stacked in one scrollable view (matches the
old dashboard's flat-card-stack layout, not a sub-tab strip): Model Routing, Config Overrides,
a 6-card config editor grid (Server/Routing/Circuit Breaker/Observability/Agent/Model Warmer)
+ editable Backends table + Save/Reset, and a collapsible Agent Guide reference at the bottom.
No auto-poll on this view (matches old dashboard) — refetching under an in-progress edit would
clobber it; refresh is manual via per-section Refresh buttons.

**Real bug found and fixed, not just ported:** the old dashboard's `buildConfigFromForm()`
(PR #G3, `dashboard.html`) rebuilds the entire config object from just the fields it has UI
for, on every save. `Config` has `#[serde(default)]` on every top-level field, so any field the
form doesn't reconstruct — `routing.auto`, `routing.scored` (22 scorer weights), `routing_profiles`,
`tls`, `rate_limiting`, `budget`, `discovery`, `fleet`, `frontier`, `providers`, `task_classifier`,
`agent.permissions` — silently resets to its Rust default on every Settings save. Confirmed this
isn't hypothetical: this repo's real `herd.yaml` has live values in all of those (scored routing
strategy with tuned weights, real `fleet`/`frontier` blocks). `agent.permissions` specifically was
worse — old dashboard hardcoded it to `{deny_file_patterns: [], deny_bash_patterns: [], allow_shell_commands: false}`
unconditionally, not just "missing from the form."

**Fix:** `settings.js` fetches `GET /admin/config` once into `fullConfig` and mutates that
object in place — `applyFormToConfig()` only ever writes the specific leaf fields the form
exposes, and `saveConfig()` PUTs `fullConfig` back whole. Untouched sub-trees round-trip
byte-identical since they were never touched, not because anything preserves them. **Verified
live, not just reasoned about:** edited `server.rate_limit` via the UI, clicked Save & Reload,
then diffed `GET /admin/config` before/after on `routing.scored.weights` (all 22 keys),
`fleet`, `frontier`, `routing_profiles` — byte-identical; `rate_limit` changed 0→5 on disk and
in the reloaded config; toast read "Reloaded: 1 backends, strategy=Scored".

**Second real bug found while building, not porting an old one:** the Routing Strategy
`<select>` only had the old dashboard's 4 options (model_aware/priority/least_busy/
weighted_round_robin) — missing `scored` (`RoutingStrategy::Scored` in `src/config.rs`,
added by the scorer work after PR #G3 shipped, see `herd-scorer-*` memories). This repo's
own `herd.yaml` runs `strategy: scored`, so the dropdown rendered **blank** on first load
(confirmed via screenshot) — saving in that state would have PUT an empty string and either
gotten rejected by `validate()` or, worse, if accepted, corrupted the strategy. Added the
`scored` option; reloaded and confirmed the dropdown now shows "Scored" correctly and a
save round-trips it unchanged.

**Model Routing** (global, cross-backend view — distinct from the per-node tab in Node
Detail, satisfies the same parity-checklist line from Settings so users with many backends
don't have to navigate node-by-node) is a straight port of PR #G3's UX (filter-as-you-type,
All/None, per-backend allowlist checkboxes with "missing" badges for checked-but-uninstalled
models, hot-model multi-select) but re-implemented with `addEventListener` + `data-*`
attributes instead of inline `onclick=""` strings — matches this codebase's established
pattern (fleet.js/node_detail.js/sessions.js all avoid inline handlers), and sidesteps the
attribute-escaping XSS surface PR #G3's commit message flagged as needing `attrEscape()`
in the first place. Added `HerdApp.attrEscape()` to `app.js` (escapeHtml + `'` → `&#39;`) as a
shared utility since Settings is the first screen with several data-* attributes carrying
admin-controllable strings (backend names, override scope/key).

**Verified live in-browser**, full flow: unchecked a model in the `local-5090` card, clicked
"Save allowlist" — toast confirmed, card re-rendered to "6 of 7 allowed", **and** the Config
Overrides table picked up the new `backend:local-5090 / models_enabled` row with the real
JSON array and timestamp (confirms the override-CRUD round trip, not just the allowlist PUT).
Clicked Delete on that row — toast "Removed override...", overrides table fell back to the
"running pure herd.yaml" empty state, **and** the Model Routing card auto-refreshed back to
all-7-checked ("Routing ALL installed models"). Also verified: hot-models multi-select save;
Backends table add-row (focuses the new name field, priority defaults to 50) then remove-row;
401/locked state (cleared the API key in localStorage, reloaded — clean "API key required"
card, no console error); Agent Guide collapse/expand renders real static content. No console
errors from our code (same one stock Chrome-extension messaging exception noted in every
prior slice, unrelated).

Checked off in `tasks/HERD-DASHBOARD-REDESIGN.md`: Phase 1 "Settings" (noted routing_profiles
editor UI as intentionally out of scope — old dashboard never had it either, and it round-trips
unchanged via the whole-object-merge fix above, so nothing is lost by deferring it), and the
parity-checklist lines for Config Overrides, Settings config editor, Agent Guide, and
per-backend routing allowlists/hot-models/missing-badges.

**This was the last unbuilt Phase 1 screen** — all six sidebar views (Fleet, Node detail,
Analytics, Sessions, Settings, plus the Models/Tasks Phase-2 stubs) now have real content.

**Next up:** item 4 from the prior "Next steps" list — go through the parity checklist in
`tasks/HERD-DASHBOARD-REDESIGN.md` item-by-item now that all screens exist (several Fleet/
header-level lines — version footer, update badge, self-enroll flow, app-wide 401 states —
are still unchecked and need a dedicated pass, not just the Settings-scoped ones checked off
this session). After that: item 5 (flag the two non-Ember Phase-2 frames before building them)
and item 6 (version/test-count drift housekeeping).

## Sessions screen added (this session, on top of the Analytics slice below)

Built `src/dashboard2/sessions.js` (already listed in `build.rs`'s `PARTS`,
no build.rs edit needed) — agent session list (`GET /agent/sessions`,
polled 10s, table like Fleet's) + a detail modal.

**Key design call beyond old-dashboard parity:** the old dashboard sends
messages via the blocking `POST /agent/sessions/:id/messages` (no feedback
until the whole tool-call loop finishes). The brief's "Data is polled ...
except agent-session events (WebSocket)" line pointed at a real, unused
capability: `agent/ws.rs`'s `/agent/sessions/:id/ws` route already streams
`AgentEvent`s (`thinking`/`tool_call`/`tool_result`/`permission_denied`/
`message`/`error`, tagged JSON) as the agent works, authenticated via
`?api_key=` query param (WS upgrade can't carry a custom header). So
dashboard2 opens that socket on modal-open and sends over it; only falls
back to the blocking REST endpoint if no admin key is set (no key = no WS
auth = read-only anyway) or the socket isn't open. Verified live: sent a
real message end-to-end through the WS, watched the reply land, confirmed
the message list updates and the socket closes on modal-close.

Status pills reuse the existing `--health-*` glyph vocabulary (active→healthy
green, processing→degraded amber-desaturated, error→offline red, completed→
neutral) rather than inventing a second color language — this is a workflow
status, not GPU health, but the fleet accent-discipline rule ("no brand
amber in a status column") still holds since nothing here uses `--amber`.

`/agent/sessions` 404s/405s when `agent.enabled` is false (the default —
confirmed `herd.yaml` in this repo has no `agent:` section at all, so it's
off by default). Rather than hiding the Sessions nav item (old dashboard's
approach), sessions.js shows an honest empty-state card: "Agent sessions
aren't enabled on this gateway — set `agent.enabled: true`..." — consistent
with how Models/Tasks already handle their not-yet-wired states in this
dashboard, no nav-hiding logic exists anywhere else in dashboard2.

**Verified live in-browser:** rebuilt, relaunched the isolated test instance
(scratch `herd-test.yaml`, port 40199). First confirmed the honest
not-enabled state (agent.enabled defaults false). Then appended `agent:\n
enabled: true` to the scratch config only (not the real `herd.yaml`),
relaunched, created a session via curl, and drove the full UI: list showed
"1 active / 1 total" with correct status glyph; opened the modal; sent "what
is 2+2?" through the live WS input — thinking/message events streamed,
final assistant reply ("Two plus two equals four.") landed in the message
list, background list count/timestamp updated; deleted the session via the
modal button — toast fired, list correctly fell back to the empty/curl-hint
state. No console errors from our code (same one stock Chrome-extension
messaging exception as prior slices, unrelated).

Killed the leftover debug test-instance process at the end of this session
too (confirmed via `Get-Process | Select Path` it was `target/debug/herd.exe`
before stopping it, release gateway PID 48536 untouched throughout).
`cargo build` is clean after.

Checked off in `tasks/HERD-DASHBOARD-REDESIGN.md`: Phase 1 "Sessions" and
the parity-checklist "Sessions" line.

**Next up:** Settings screen (`src/dashboard2/settings.js`, `#settings-root`
stub exists) — port/reuse the model-routing UI already built for the old
dashboard in PR #G3 (07fea6d) rather than re-deriving it; add config editor
w/ secret redaction (`GET`/`PUT /admin/config`), overrides table.

---


## Analytics screen added (this session, on top of the prior handoff below)

Built `src/dashboard2/analytics.js` (already listed in `build.rs`'s `PARTS`,
so no build.rs edit needed) — fleet-wide request volume, latency, and
token/cost metrics, backed entirely by `GET /analytics?hours=N`
(`AnalyticsStats` in `src/analytics.rs`). Follows the same
`HerdApp.registerView` pattern as `fleet.js` (mount/start/stop/pollSeconds),
30s poll per the brief.

**Bug avoided, not introduced:** the *old* `dashboard.html`'s
`updateAnalytics()` reads `data.p50_ms`/`p95_ms`/`p99_ms`, but
`AnalyticsStats` actually serializes those fields as `latency_p50` /
`latency_p95` / `latency_p99` (confirmed via `src/analytics.rs:393-408` and
a live `curl /analytics?hours=24` against the test instance) — the old
dashboard's top-line latency stat has silently always read `undefined` →
`0ms`. dashboard2's `analytics.js` uses the real field names. Also: there is
no `error_count` field on `AnalyticsStats`, so (unlike the old dashboard) no
success-rate stat is shown — that field doesn't exist server-side either.

Added CSS to `design-system.css`: `.chart-grid`, `.chart-card`,
`.chart-card-title`, `.chart-container` (+ `.tall` variant), `.tile-row`
(`.cols-2`/`.cols-3`) — reuses the existing `.tile`/`.card`/`.table-row`
primitives, no new component vocabulary.

Chart.js colors are read at runtime via `getComputedStyle` off the Ember CSS
custom properties (`--amber`, `--neutral-fill`, `--text-2`, etc.) rather than
hardcoded hex, so the palette stays a single source of truth. Timeline chart
uses chrome amber (activity, not a status column — accent discipline holds).
Model/backend/token breakdown charts use a neutral/amber-adjacent palette
that deliberately avoids the `--health-*` hues (green/red/purple stay
reserved for health semantics only).

**Verified live in-browser:** rebuilt, relaunched the isolated test instance
(same recipe as below — scratch `herd-test.yaml`, port 40199, debug binary),
navigated to `/dashboard2` → Analytics. Request volume/by-model/by-backend
charts render real data (2 requests, 1 backend `local-5090`, matches a
direct `curl /analytics?hours=24`); latency tiles show `0ms` correctly (no
successful timed requests logged yet — server was just started); cost/tok-s
tiles show `$0.00` / `0.0 t/s` correctly for empty token data. Switched
hours 24h → 1h via the selector: summary line and all charts/tables
correctly refetched and re-rendered (2 requests → 0, since those 2 log
entries pre-date the 1h window — confirms the hours param round-trips).
Navigated away to Fleet and back to Analytics: view re-rendered cleanly, no
duplicate-canvas Chart.js errors (mount() guards on a `built` flag so DOM/
Chart instances are created once and reused, not torn down on view switch).
No console errors from our code (one stock Chrome-extension messaging
exception, same unrelated noise noted in the prior handoff entries below).

Killed a leftover debug test-instance process (PID 6424, later PID 67984
after relaunch) at both the start and end of this session before/after
`cargo build` — confirmed via `Get-Process | Select Path` that it was
`target/debug/herd.exe`, not the live release gateway (PID 48536), before
stopping it. `cargo build` is clean.

Checked off in `tasks/HERD-DASHBOARD-REDESIGN.md`: the Phase 1 "Analytics"
checklist item and the parity-checklist "Analytics" line.

**Next up (unchanged from before):** Sessions screen (`src/dashboard2/sessions.js`,
`#sessions-root` stub exists) — see "Next steps" section below, now item 1.

---


## Task

Build `/dashboard2` — a new static HTML dashboard behind a flag, alongside
(not replacing) the existing `dashboard.html`, implementing the approved
"Ember" design direction against today's live API only (no backend work this
pass). Full ground rules and design context in
`docs/specs/dashboard-redesign-brief.md` and `tasks/HERD-DASHBOARD-REDESIGN.md`
(read that second file first — it has the endpoint map, decisions log, and
Phase 1 checklist).

Ground rules (from the user, verbatim intent):
- Single static file constraint: authored as separate partials, concatenated
  by `build.rs` at compile time into one `include_str!`'d artifact — no
  runtime build step, no framework, no npm. (User was open to this if I made
  the case for it over hand-authoring one giant file; I did, decision logged
  in `tasks/HERD-DASHBOARD-REDESIGN.md`.)
- Served at `/dashboard2`, existing `/dashboard` untouched until parity is
  verified and we explicitly swap.
- Ember accent discipline: brand amber = chrome only (wordmark, button
  fills, hot-model star). green/amber/red/purple = health only (status dot,
  verdict glyphs). They never cross.
- Parity checklist in `tasks/HERD-DASHBOARD-REDESIGN.md` is the completion
  gate — don't lose functionality the old dashboard has.

## State — what's built and verified

**Architecture (done):**
- `build.rs` — concatenates `src/dashboard2/*` partials into
  `$OUT_DIR/dashboard2.html`, wraps `.css` in `<style>`, `.js` in `<script>`.
  Missing partial files are skipped (expected mid-buildout — analytics.js,
  sessions.js, settings.js don't exist yet); a partial that exists but fails
  to read (bad encoding, permissions) panics loudly instead of silently
  dropping — this distinction matters, see Gotchas.
- `src/server.rs` — added `/dashboard2` route + handler right after the
  existing `/dashboard` route/handler (both untouched, both present).

**Screens built (Fleet + Node Detail — frames 5a/5b):**
- `src/dashboard2/shell_head.html` — `<head>`, fonts, Chart.js CDN with
  verified SRI hash.
- `src/dashboard2/design-system.css` — full Ember token set + component
  library (~350 lines).
- `src/dashboard2/shell_body.html` — sidebar nav + all view `<section>`
  mounts (fleet/models/analytics/sessions/tasks/settings — models/tasks are
  Phase 2 stubs, analytics/sessions/settings are stub mounts only, no JS
  behind them yet).
- `src/dashboard2/app.js` — shell: API key handling, fetch wrapper, toasts,
  modals, view router (`registerView`/`switchView`), formatting helpers
  (`fmtGb`, `fmtPct`, `fmtRelativeTime`, `escapeHtml`, `normalizeUrl`).
- `src/dashboard2/mark.js` — base64 PNG data URI for the Herd wordmark glyph
  + `applyHerdMark()`.
- `src/dashboard2/fleet.js` — Fleet table: merges `/api/nodes` (agent
  registry) with `/status` + `/admin/config/backends` (routing pool) on a
  normalized-URL join key. Handles the healthy/degraded/unreachable
  3-state → 4-state UI mapping (circuit-open glyph reserved, unreachable
  from today's API). Empty-first-run state, 5s poll.
- `src/dashboard2/node_detail.js` — Node detail: identity header, action
  buttons (Enable/Disable/Remove are real API calls; Restart/Install are
  honest Phase-2 toasts), telemetry tiles (real GPU data when a routing-pool
  match exists, honest "not reported" otherwise), Models & Routing tab
  (real allowlist checkboxes + hot-model toggle wired to
  `PUT /admin/config/backends/:name` endpoints), Activity tab (honest stub).
- `src/dashboard2/shell_foot.html` — closing tags.

**Verified live in-browser (this session, just now):** built the binary,
launched an isolated test instance (see Gotchas — do NOT reuse this
approach carelessly), navigated to `/dashboard2` with the claude-in-chrome
tool. Fleet table renders real merged data (2 rows: a pool-only backend and
an agent-enrolled node), health glyphs and Ember accent discipline correct,
nav badge correct, node detail opens and shows real identity/telemetry/tabs
for the agent-enrolled node, and correctly shows an honest "Node not found"
banner for the pool-only row (it has no real node-registry ID to fetch —
known Phase 2 dedup gap, not a bug). No console errors from our own code.

## Third bug found and fixed (after second handoff update, same session)

User reported: "in this version local-5090 clicking results in node not
found...even though its the local one and is running." I'd previously
(wrongly) written this off in the initial handoff as an expected Phase-2 gap
— user pushback was correct, this needed a real fix.

**Root cause** (confirmed via direct `curl` against the test instance, not
guessing): `local-5090` is a **pool-only backend** — configured directly in
`herd.yaml`'s `backends:` list, never agent-enrolled. `/api/nodes` has no
record for it at all (only `warden`, the one real agent-registered node);
it exists only inside `/status`'s `healthy_backends` and
`/admin/config/backends`. `node_detail.js`'s `open()`/`render()`
unconditionally called `GET /api/nodes/:id`, which 404's for any pool-only
backend — the "Node not found" banner was accurate given that assumption,
but the assumption itself was wrong. **This is the common case for existing
Herd installs** (static Ollama/llama-server backends predating agent
enrollment), not a rare edge case — worth remembering for any future
dashboard2 work that touches node/backend identity.

**Fix applied:**
- `fleet.js`: `rowHtml()` now stamps `data-kind="${r.node ? 'node' : 'backend'}"`
  on each row; the row click handler passes `el.dataset.kind` through to
  `HerdNodeDetail.open(id, kind)`.
- `node_detail.js`: `open(nodeId, kind)` now stores `currentKind`; `render()`
  dispatches to the existing node-registry path (renamed `renderNodeDetail`,
  behavior unchanged) or a new `renderBackendDetail()` path when
  `kind === 'backend'`. The backend-only path fetches `/status` +
  `/admin/config/backends`, matches by `name === id` (no `/api/nodes/:id`
  call at all), and reuses the existing `renderTelemetryTiles`,
  `renderModelsRouting`, `wireModelsRouting`, `renderActivity`, `wireTabs`
  helpers unchanged (confirmed none of them actually depend on a real
  node-registry object — `renderModelsRouting`/`wireModelsRouting` don't use
  their `node` param at all; `renderTelemetryTiles` only reads
  `node.gpu_model`/`node.source`, satisfied by a `{source: 'static'}` stub).
  Enable/Disable is wired to the real `PUT /admin/config/backends/:name`
  endpoint (confirmed this endpoint exists and supports `enabled` in
  `src/api/admin.rs`'s `patch_backend`). **No Remove button** — confirmed via
  `server.rs`'s route table that there is no DELETE route for backends, so
  fabricating one would be dishonest UI.

**Verified live in-browser:** rebuilt, relaunched the isolated test instance,
set the admin API key via `localStorage` (avoided the `window.prompt` key
dialog after it froze the tab once — see Gotchas), clicked `local-5090` in
Fleet. Header now reads `local-5090 · ollama · http://127.0.0.1:11434 ·
priority 100` with a real health dot, "Restart backend / Install model /
Disable" actions (no Remove), and an honest "not reported" telemetry banner
(this backend's `/status` entry has no `.gpu` block in this sample — real
absence, not a bug). Models & Routing tab renders all 7 real models with
correct allowlist checkboxes and hot-model ★ stars matching `/status`
(`gemma4:e4b`, `qwen3:1.7b` hot). No console errors from our own code
(only stock Chrome-extension messaging noise, unrelated).

## Second bug found and fixed (after initial handoff write, same session)

User reported "none of the menu tabs are clickable." Two compounding causes:
1. My own earlier automated testing (clicking into node detail) had set
   `localStorage['herd-dashboard2-last-view'] = 'node'` in the browser.
   `node` has no sidebar nav item and renders nothing without an explicit
   `.open(id)` call, so a fresh page load landed on a blank orphaned section
   with no tab highlighted — looked totally broken.
2. Real bug: `switchView()` in `app.js` required `views[name]` (a
   registered JS module) to exist before it would do anything, including
   swap section visibility. This silently blocked navigation to Models and
   Tasks, which already have real static stub content in `shell_body.html`
   but no JS module — clicking them did nothing at all.

**Fix applied** (`app.js`): `switchView` now only requires the target
`#view-<name>` section to exist in the DOM; `views[name]` is optional and
only gates the mount/start/stop lifecycle calls. `init()`'s last-view
restore now only trusts localStorage when it names an actual
`.nav-item[data-view=...]` destination, so a stale/orphaned view (like
`node`) can never be what a fresh load lands on. Rebuilt and verified all
six sidebar tabs (Fleet/Models/Analytics/Sessions/Tasks/Settings) switch
and highlight correctly.

## First bug found and fixed this session (real, not cosmetic)

`app.js` and `fleet.js` (and `node_detail.js`) each attached their own
`document.addEventListener('DOMContentLoaded', ...)`. Listeners fire in
registration order — app.js's runs first and immediately calls
`switchView('fleet')` inside `init()`, but fleet.js's listener (registered
second) is what actually calls `HerdApp.registerView('fleet', ...)`. So the
initial view switch ran before the view existed, `switchView` silently
no-op'd (`if (!views[name]) return;`), nothing ever mounted, no fetch ever
fired, and the page sat on the static "Loading…" text forever — with zero
console errors, since nothing threw.

**Fix applied:** `fleet.js` and `node_detail.js` now call
`HerdApp.registerView(...)` synchronously at module top-level (no
`DOMContentLoaded` wrapper), since the DOM they touch already exists by
script-execution time (shell_body.html is concatenated before them in
`build.rs`'s `PARTS` order). **Apply this same pattern** when writing
`analytics.js`, `sessions.js`, `settings.js` — do not wrap their
`registerView` call in `DOMContentLoaded`.

## Gotchas

- **A pre-existing, unrelated CLI bug**: `main.rs:56-79`'s `serve()` only
  applies `--port`/`--host` CLI overrides in the branch where `--config` is
  *not* passed. If `--config` is given, `--port` is silently ignored and the
  YAML's `server.port` wins. This is why my first attempt to run an isolated
  test instance on `--port 40199` kept binding to 40114 instead and
  colliding with the user's live gateway process. Not fixed (out of scope
  for this pass) — worth a one-line fix if the user wants CLI overrides to
  compose with `--config`.
- **The user's real gateway is `target/release/herd.exe` (PID varies,
  currently 48536), port 40114** — that is live infrastructure, never touch
  it. Any test instance must use `target/debug/herd.exe` with a **scratch
  copy** of `herd.yaml` with the port changed (I used
  `<scratchpad>/herd-test.yaml`, port 40199) — do NOT pass `--port` alongside
  `--config` expecting it to override, per the bug above.
- Debug binary running as a test instance **locks `target/debug/herd.exe`**
  on Windows — `cargo build` will fail with "Access is denied" until that
  test process is stopped. Check `Get-Process -Id <pid> | Select Path` before
  killing anything, to confirm you're killing the debug test instance and
  not the release gateway.
- Windows + git-bash + Python: always write files with
  `encoding='utf-8', newline='\n'` explicitly — the default encoding
  produced an invalid-UTF-8 byte once (an em-dash) that silently dropped an
  entire partial from the build (see `build.rs`'s loud-panic-on-read-failure
  fix, already applied, don't regress it back to silent skip-on-any-error).
- `/tmp` does not resolve to a writable path in this environment — always
  use the scratchpad dir.
- Clicking the "API key" row in the dashboard triggers a native
  `window.prompt()` — this **freezes claude-in-chrome's CDP connection**
  (screenshot/javascript_tool/computer all time out) until the dialog is
  dismissed. A stray `Escape`/`Return` key press (or a fresh `navigate` call)
  clears it, but avoid clicking that row from browser automation — set
  `localStorage.setItem('herd-api-key', ...)` directly instead.

## Next steps (in the user's stated order)

1. ~~Analytics screen~~ — done, see entry above.
2. ~~Sessions screen~~ — done, see entry above.
3. ~~Settings screen~~ — done, see the newest entry at the top of this file.
4. Go through the parity checklist in `tasks/HERD-DASHBOARD-REDESIGN.md`
   item-by-item once all screens exist.
5. Flag the Turn-4-only (non-Ember) Add Node modal / Update Fleet confirm
   frames to the user before building them in Phase 2 (noted in
   `tasks/HERD-DASHBOARD-REDESIGN.md` already).
6. Housekeeping: version/test-count drift noted in `tasks/HERD-DASHBOARD-REDESIGN.md`
   — low priority, fold in opportunistically.

## Verification status

- `cargo build`: passes (debug profile), confirmed again at the end of the
  Settings slice (most recent).
- No automated test suite run (this is pure frontend/static HTML work with
  no Rust logic changes beyond the two route/handler lines in `server.rs`).
- Manual in-browser verification: done for Fleet + Node Detail (prior
  session), Analytics, Sessions, and Settings (this session, see entries
  above). All six Phase 1 screens now have real, browser-verified content.
- No commits made — all changes are still uncommitted in the working tree
  (`git status` shows `src/fit/` and the dashboard redesign task files as
  untracked/new; dashboard2 files are additional new untracked files under
  `src/dashboard2/` plus `build.rs`, plus the `server.rs` route addition).
