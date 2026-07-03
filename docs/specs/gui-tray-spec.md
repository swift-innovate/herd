# Herd GUI & Tray — v1.4 Spec

**Milestone:** v1.4 "Approachable Herd" (v1.3 remains distributed-inference: RPC sharding, agent/enrolled dedup)
**Status:** Draft — locked decisions below, PR breakdown at bottom
**Author:** Tom Swift / Gage
**Date:** 2026-07-03

---

## Problem

Herd's adoption ceiling is "people who enjoy editing YAML." Three concrete pains:

1. **No GUI config surface.** Every routing decision — which models a backend exposes,
   hot_models, priorities — requires hand-editing `herd.yaml` and knowing the schema.
2. **Model sprawl poisons routing.** A long-lived Ollama install (e.g. CITADEL: 84 models
   accumulated over years of experiments) exposes *everything* via `/api/tags`, so the
   residency gate treats stale experiments as routing candidates. `model_filter` (regex)
   exists but is hostile as a user-facing control.
3. **Zero onboarding.** A new user with no `herd.yaml` and no backend gets a config error,
   not a path forward. No Ollama detection, no starter guidance.

## Goals

- Tray icon: at-a-glance gateway health + one-click access to dashboard/config.
- GUI model selection: per-backend checkbox allowlist of which installed models Herd routes to.
- Ollama auto-detection with zero config.
- First-run onboarding: detect → (optionally) link installs → suggest starter model families by VRAM tier.
- Surface future config options through the same GUI channel without another config-file schema.

## Non-Goals (v1.4)

- No new GUI framework. **No Tauri app, no egui, no webview bundling.** The existing web
  dashboard (`dashboard.html`) is the single config surface; the tray is a native
  status/launcher only.
- No macOS/Linux tray in the first cut (code stays portable; Windows ships first).
- No YAML rewriting. GUI never serializes `herd.yaml` (comments are documentation).
- No model *deletion* (`ollama rm`) from the GUI — allowlist only. Deleting user data
  from a router GUI is a footgun; revisit post-v1.4.
- No auth/user system beyond the existing `server.api_key`.

---

## Locked Decisions

### D1 — Tray is a launcher, dashboard is the GUI
`herd-tray` is a new small workspace binary using `tray-icon` + `muda` + `tao` (no webview).
All configuration UI lives in the existing dashboard as a new **Settings** tab. The tray's
"Models…" item opens `http://{host}:{port}/#settings` in the default browser (`open` crate).
One UI, one truth; remote users get the identical config surface.

### D2 — `models_enabled` allowlist (GUI-managed), `model_filter` (hand-managed) both survive
New optional field on `Backend`:

```rust
/// GUI-managed allowlist. When Some, discovery retains ONLY these models
/// (exact-name match on the backend's reported model list).
/// Some(vec![]) is valid and means "expose nothing" (explicit off-switch).
/// Takes precedence over model_filter when both are set.
#[serde(default)]
pub models_enabled: Option<Vec<String>>,
```

Filter application order in `ModelDiscovery::discover_models`:
`raw list → models_enabled (if Some) → model_filter regex (only if models_enabled is None)`.
`None` = current behavior, zero change for existing configs (code-quality rule: defaults off).

### D3 — SQLite settings overlay; YAML stays the hand-edited base
GUI-managed values persist to a new `config_overrides` table in the existing `herd.db`
(NodeDb connection), NOT to `herd.yaml`.

```sql
CREATE TABLE IF NOT EXISTS config_overrides (
  scope      TEXT NOT NULL,   -- e.g. 'backend:local-5090'
  key        TEXT NOT NULL,   -- e.g. 'models_enabled'
  value_json TEXT NOT NULL,   -- JSON-encoded value
  updated_at TEXT NOT NULL,
  PRIMARY KEY (scope, key)
);
```

Merge semantics: config is loaded from YAML, then overrides are applied on top
(**DB wins on conflict**). Applied at: (a) startup after YAML parse, (b) config
hot-reload, (c) immediately on PUT (write DB → patch in-memory `AppState.config`
→ nudge discovery). A `GET /admin/config/overrides` + `DELETE /admin/config/overrides/:scope/:key`
pair makes the overlay inspectable and reversible — no invisible state.

v1.4 supports overrides for: `backend:{name}` scope, keys `models_enabled`,
`hot_models`, `priority`, `enabled`. The table is generic so future GUI settings
need no migration.

**Backends created by the GUI** (e.g. auto-detected Ollama) use scope `backend:{name}`,
key `definition` (full Backend JSON). Overlay-defined backends are appended to the pool
after YAML backends; name collision → YAML wins + warn (never bail — code-quality rule).

### D4 — Admin API endpoints (all guarded by existing `server.api_key` middleware)

| Method | Path | Purpose |
|---|---|---|
| GET | `/admin/config/backends` | Effective (merged) backend configs + per-backend `models_available` (live from pool) + `models_enabled` |
| PUT | `/admin/config/backends/:name/models` | Body `{"models_enabled": ["a","b"] \| null}` — null clears the override |
| PUT | `/admin/config/backends/:name` | Patch `priority` / `enabled` / `hot_models` |
| POST | `/admin/config/backends` | Create overlay-defined backend (used by detect flow) |
| GET | `/admin/config/overrides` | Dump the overlay |
| DELETE | `/admin/config/overrides/:scope/:key` | Remove one override |
| GET | `/admin/detect/ollama` | Probe `OLLAMA_HOST` env, then `http://127.0.0.1:11434/api/version`; returns `{found, url, version, model_count}` |
| POST | `/admin/models/pull` | Proxy Ollama `/api/pull` for a named backend; streams NDJSON progress through |

New endpoints go into `skills.md` and the dashboard Agent Guide tab (repo rule).

### D5 — Settings tab (dashboard.html)
- **Backends section:** card per backend. Shows name, URL, type, health. Model list
  rendered as checkboxes: checked = enabled. Source of truth for the list is
  `models_available` from the pool (i.e. `/api/tags` for Ollama, `/v1/models` for
  llama-server). "Select all / none", filter-as-you-type box (84 models needs search).
  Save → PUT. A model that is checked but no longer installed renders greyed with a
  "missing" badge (kept in the allowlist — it may be mid-`ollama pull`).
- **Hot models:** per-backend multi-select drawn from the enabled set → writes
  `hot_models` override (warmer picks it up next tick).
- **Overrides panel:** raw view of the overlay with per-row delete (D3 inspectability).

### D6 — First-run onboarding wizard (dashboard)
Shown when the effective config has **zero backends** (also reachable manually via
Settings → "Add backend"). Steps:

1. **Detect.** Call `/admin/detect/ollama`. Found → offer "Use this Ollama" →
   POST creates the overlay backend, jump to step 3.
2. **Install links.** Not found → two cards:
   - **Ollama** (recommended for new users): `https://ollama.com/download`
   - **llama.cpp / llama-server** (max performance, advanced):
     `https://github.com/ggml-org/llama.cpp/releases` + pointer to `docs/LLAMA_CPP_BACKEND.md`
   "Detect again" button re-probes.
3. **Starter models.** Read VRAM (pool `gpu_metrics`/`vram_total_mb` if present, else ask
   the user to pick a tier). Render the starter catalog (D7) for that tier with one-click
   pull buttons (`/admin/models/pull`, NDJSON progress bar). Pulled models are
   auto-added to `models_enabled`.
4. **Auto mode prefill.** Offer to write the `auto.model_map` tiers from the pulled set
   (stored as overlay scope `routing`, key `model_map` — read-only in v1.4 UI beyond
   this prefill; full model_map editor is post-v1.4).

### D7 — Starter model catalog (static JSON, versioned in-repo)
`src/onboarding/catalog.json`, compiled in via `include_str!`. Three-to-four entries per
tier; tiers keyed by usable VRAM. Initial content (update as families evolve — this file
is the single place to touch):

| VRAM tier | General | Code | Reasoning | Always |
|---|---|---|---|---|
| `<=8gb` | `qwen3:4b` | `qwen2.5-coder:7b` | `deepseek-r1:7b` | `qwen3:1.7b` (auto-classifier), `nomic-embed-text` |
| `16gb` | `gemma4:e4b` | `qwen2.5-coder:14b` | `deepseek-r1:14b` | same |
| `24gb+` | `gemma4:26b` | `qwen3-coder:30b` | `deepseek-r1:32b` | same |

Catalog schema: `{tier, role, model, approx_gb, note}`. Keep it small — a wall of
options is the CITADEL failure mode this spec exists to prevent.

### D8 — `herd-tray` binary (Windows-first)
New workspace member `herd-tray/` (convert repo root to a cargo workspace:
`[workspace] members = [".", "herd-tray"]` — the main crate's build output is unchanged).

- **Crates:** `tray-icon`, `muda`, `tao` (event loop), `open`, `reqwest` (blocking or
  minimal tokio), `serde_json`. Windows autostart via `HKCU\...\Run` registry key
  (`winreg` crate), behind `#[cfg(windows)]`.
- **Behavior:**
  - On launch: probe `GET {gateway}/api/status`. Reachable → attach (status mode).
    Unreachable → spawn `herd serve` as child process (looks for `herd.exe` beside
    itself, then PATH), supervise: child exit → icon red + "Start gateway" menu item.
  - Poll `/api/status` every 5s → icon tint: green (healthy backends > 0),
    amber (gateway up, zero healthy backends), red (gateway unreachable).
  - Menu: **Open Dashboard** · **Models…** (→ `/#settings`) · **Start/Stop gateway**
    (only when supervising) · **Start at login** (checkbox, registry) · **Quit**
    (stops supervised child, leaves attached gateways alone).
  - Single instance: named mutex (`Global\herd-tray`); second launch focuses nothing,
    just exits 0.
- **Config:** `--gateway <url>` (default `http://127.0.0.1:40114`), `HERD_TRAY_GATEWAY` env.
- Tray never writes config; it is a viewer/launcher (D1).

### D9 — Routing interaction note (context from 2026-07-03 analysis)
`models_enabled` filters the **pool model list**, which is what the scored router's
residency GATE reads. For static Ollama backends that list is `/api/tags` (installed);
for enrolled/agent nodes it is currently `/api/ps` (warm-only). The installed-vs-warm
gate split is a **separate v1.3/v1.4 work item** (see tasks/todo: "split installed from
warm for Ollama backends") — this spec neither depends on nor blocks it. The Settings
tab reads whatever the pool reports; when the gate fix lands, enrolled/agent nodes get
checkbox lists for free.

---

## PR Breakdown

| PR | Title | Scope | Depends on |
|----|-------|-------|-----------|
| #G1 | `models_enabled` + config overlay store | `Backend.models_enabled`; discovery filter order (D2); `config_overrides` migration + `NodeDb` CRUD; overlay merge at load/reload; unit tests (filter precedence, `Some(vec![])`, merge determinism, YAML-wins-on-name-collision) | — |
| #G2 | Admin config API | D4 endpoints minus detect/pull; api_key guard; patch-in-memory + discovery nudge on PUT; `skills.md` + Agent Guide entries; endpoint tests (auth, null-clears, unknown backend 404) | #G1 |
| #G3 | Settings tab | Dashboard Settings tab per D5 (checkbox list w/ search, hot_models select, overrides panel); no new build tooling — stays vanilla JS in `dashboard.html` like existing tabs | #G2 |
| #G4 | Ollama detect + onboarding + catalog + pull proxy | `/admin/detect/ollama`, `/admin/models/pull` (NDJSON stream-through), `catalog.json`, wizard UI (D6), model_map prefill override | #G2 (#G3 for UI shell) |
| #G5 | `herd-tray` | Workspace conversion; tray bin per D8; icon assets (green/amber/red .ico); autostart; single-instance; supervised-child lifecycle tests where feasible (status-poll + menu-state logic unit-tested behind traits) | gateway API only |

#G1 and #G2 are the value core and independently shippable — they fix CITADEL's 84-model
problem via `curl` even before the Settings tab exists.

## Acceptance (v1.4)

- [ ] Existing `herd.yaml` files load byte-for-byte identically with no overrides present
- [ ] Checking/unchecking models in Settings changes the routable set within one discovery tick, no restart
- [ ] `Some(vec![])` allowlist yields an empty pool model list (backend healthy, gates everything)
- [ ] Override survives gateway restart; `DELETE` restores YAML behavior
- [ ] Fresh machine, no `herd.yaml`, Ollama running → wizard detects, creates backend, user pulls a starter model, first chat completion routes — zero file edits
- [ ] Tray icon reflects gateway state transitions (green/amber/red) and survives gateway restart
- [ ] `cargo test` green; new endpoints in `skills.md`

## References

- Routing/gate analysis motivating D9: conversation 2026-07-03 (installed-vs-warm split)
- `docs/LLAMA_CPP_BACKEND.md` — backend strategy, linked from onboarding
- `CLAUDE.md` — code-quality rules (defaults-off, never-bail, endpoint registry)
