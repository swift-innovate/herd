# Herd — Working TODO

> Scratchpad for in-flight work. Milestone tracking lives in `ROADMAP.md`.

**Last updated:** 2026-07-02

---

## ACTIVE — Dashboard: unify Backends+Fleet + self-enroll

**Goal (user):** Backends and Fleet aren't really different — unify them. Any node
appears on the main dashboard. Easy "add node" + a surfaced install link. Opening the
dashboard on a computer → easily make that computer a node.

**Findings:**
- 3 node types today, split across 2 tabs: static backends (`herd.yaml` → `/admin/backends`,
  "Backends" card grid) + enrolled nodes (`herd-tune` → `POST /api/nodes/register`) + agent
  nodes (`herd agent` heartbeat) — the latter two in the "Fleet" table (`/api/nodes`).
- Install infra EXISTS: `GET /api/nodes/script?os=windows|linux` bakes the gateway endpoint
  (Host header / `HERD_PUBLIC_URL`) + enrollment key into `herd-tune.ps1/sh`. herd-tune does
  full GPU/VRAM detect + backend setup + register.
- dashboard.html is a single 2829-line file, `include_str!`-compiled into the binary
  (`/dashboard`). Tabs: backends(default)/analytics/sessions/agent-guide/fleet/models/settings.

**Design (locked with user):**
- ONE unified **"Nodes"** view replaces the separate Backends + Fleet tabs. Merge
  `/admin/backends` (+`/status`) and `/api/nodes` client-side into one table. **No source
  column** — treat every node identically (columns: name, GPU, VRAM, status, models, load/latency).
- Prominent **"Add Node"** affordance → panel with an OS-auto-detected **herd-tune one-liner**
  pre-pointed at THIS gateway (`window.location.origin`): e.g. Windows
  `iwr <origin>/api/nodes/script?os=windows -OutFile herd-tune.ps1; ./herd-tune.ps1`.
  Copy button. Both windows + linux shown.

### Plan
- [ ] Read dashboard render paths (backends grid render, fleet table render, switchTab,
      /api/nodes + /admin/backends + /status fetch/merge points).
- [ ] Build unified `renderNodes()` — normalize static-backend and node schemas to one row
      shape; merge + best-effort dedup by name/url (full enrolled+agent dedup deferred to v1.4).
- [ ] Replace Backends + Fleet tabs with one "Nodes" tab (keep analytics/sessions/guide/models/settings).
- [ ] Add "Add Node" panel: herd-tune one-liner (origin-baked, OS-detected), copy button, link.
- [ ] Rebuild + verify live against the running BASTION gateway (local-5090 + citadel + warden
      should all appear in one list; install one-liner shows correct origin).
- [ ] Update CLAUDE.md dashboard-tab rules + skills.md if endpoints/tabs change.

### Notes / open
- Known limitation: enrolled+agent on one host = two entries (dedup deferred v1.4). Dashboard
  merges but won't fully de-dup those; surface both, don't crash.
- dashboard.html is compiled in → changes need `cargo build` + gateway restart to test live.

---

## DONE — recent (collapsed; full detail in git history)
- **Fleet two-box smoke test PASSED** (2026-07-02) — gateway BASTION + agent CITADEL over
  Tailscale; cross-machine routing validated. See memory `herd-fleet-two-box-smoke-test`.
- **v1.3.0 released** — scorer feature-complete (22/23 dims); tag `v1.3.0`, GH release, 4 artifacts.
- **Scorer Phase 4 + dim-22 VRAM tuning** — PRs #29–33; dim 21 deferred to v1.4.
