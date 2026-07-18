# Herd Dashboard Redesign — Design Brief

**Audience:** Claude Design (or any designer/design tool)
**Author:** Tom Swift (Director) + Gage
**Date:** 2026-07-17
**Status:** Ready for design pass

---

## What Herd is

Herd is a single-binary Rust reverse proxy and control plane for self-hosted LLM inference. It sits in front of a fleet of GPU nodes running llama-server (llama.cpp), Ollama, or any OpenAI-compatible backend, and routes requests by model residency, live GPU load, and health. One endpoint, one dashboard, the whole fleet.

The name is the joke: a herd of (o)llamas. The product around the joke is serious infrastructure.

**Users:** homelab operators and small teams running 1–10 GPU nodes. Technically sharp, allergic to enterprise bloat, live in terminals but appreciate a good ops UI (think k9s, Portainer, Proxmox users).

## The assignment

Design a **design system + three key screens** for the Herd dashboard:

1. **Fleet overview** (the landing screen)
2. **Node detail** (drill-in on one GPU node)
3. **Model install flow** (search → per-node fit verdict → install → progress)

The current dashboard grew tab-by-tab over 14 releases and its information architecture leaks internal storage details (a "Backends" tab for the routing pool and a separate "Fleet" tab for the node registry — same GPUs, two views). The redesign should present **nodes as first-class objects** the user drills into, not tabs that mirror the database.

## Brand & identity

**Rule: the llama appears once, at the top, and never speaks.**

- **Provided asset:** the Herd mark — three staggered white llama silhouettes on dark (see `assets/` or attached). Do not redesign it. Design *around* it:
  - Wordmark treatment pairing "Herd" with the mark for the header
  - A reduced small-size variant direction (front llama only, or two simplified silhouettes) for favicon/tray at 16–32px — the full three-llama mark loses legibility below ~48px
  - The mark is monochrome and should stay tintable (the system tray tints it gray/green/amber/red for gateway state)
- **No ranch theme.** No pasture, grazing, wrangling, corrals. Nodes are nodes, models are models, buttons use plain verbs. The identity depth comes from craft: typography, spacing, a confident accent system — not metaphor.
- **Personality budget:** spent in exactly two places — empty states (e.g. "No nodes in the herd yet" above the join command) and error/404 states. Nowhere else.
- **Aesthetic direction:** professional ops tool, dark-first. The current UI is default dark-slate + indigo gradient cards — competent but generic. We want distinctive-but-restrained: a deliberate type choice (not default Inter-on-slate), one memorable accent decision that complements a white-on-dark mark, and density appropriate for an infrastructure tool. Reference lane: Grafana's clarity, Tailscale's admin restraint, Linear's typography discipline. Not: neon cyberpunk, glassmorphism, or SaaS-landing-page gloss.

## Hard constraints

1. **Final implementation is a single static HTML file** — vanilla JS, CSS in `<style>`, Chart.js from CDN, embedded in the Rust binary via `include_str!`. No build step, no framework runtime. Design may be delivered as anything (Figma, HTML prototype, React mock) but every pattern must be implementable in vanilla HTML/CSS/JS. Avoid patterns that require virtual DOM diffing, CSS-in-JS, or component libraries.
2. **Dark-first.** Light mode optional/deferred.
3. **Data is polled, not pushed** (5s node/status, 30s analytics, 10s sessions) except agent-session events (WebSocket). Design states for: loading, gateway-unreachable, 401 (API key needed), and stale-data.
4. **Desktop-primary, must degrade to tablet.** Phone is a nice-to-have.
5. **Admin API key** unlocks mutating actions; the UI must read cleanly in both read-only (no key) and admin modes.

## Screen 1 — Fleet overview (landing)

The at-a-glance answer to "is my fleet healthy and what is it doing right now."

**Header (persistent, all screens):** mark + wordmark, gateway version, routing-strategy badge, connection indicator, update-available badge (links to fleet update), API key entry, auto-refresh countdown.

**Summary stats:** nodes online / total, healthy router backends, requests (24h), aggregate VRAM used/total, current tokens/sec.

**Node cards or rows** — one per physical node, merging today's Backends + Fleet split. Each shows:
- Name, backend type (llama-server / Ollama / openai-compat), health (healthy / degraded / offline / circuit-open)
- GPU: vendor badge, VRAM used/total, utilization, temperature (from gpu-hot or agent telemetry; may be absent — design the degraded state)
- Models resident right now; hot-models count
- Agent version + update-pending indicator (when fleet update is in flight)
- Source indicator (static config vs. agent-registered) — subtle, secondary; users mostly shouldn't care
- Click → Node detail

**Primary actions:**
- **Add Node** → modal with a one-time join token and copy-paste one-liner per OS (PowerShell / bash). This is the money shot for screenshots — make the empty state + join command beautiful.
- **Update fleet** → confirm modal: gateway version vs. per-node versions table, one button, then per-node convergence progress on the cards.

**Secondary (persistent nav, not necessarily tabs):** Analytics (request volume, latency percentiles p50/p95/p99, top models, timeline chart), Sessions (agent sessions list — niche, keep reachable but demoted), Agent Guide (API reference for AI agents), Settings (config editor with secret redaction, config-override list, routing profiles).

## Screen 2 — Node detail

Everything about one GPU node, with actions.

- **Identity:** name, node ID, backend type + URL, agent version, uptime, source (static/agent), tags
- **Live telemetry:** per-GPU cards (VRAM, util, temp, power), queue depth / slots busy vs. max_concurrent, TTFT p50 when available — every value may be absent depending on backend type; design honest "not reported" states, never fake zeros
- **Models on this node:**
  - Installed list with size, quant, resident-in-VRAM indicator
  - **Routing allowlist** — checkboxes for which installed models this node routes (exists today: filter box, All/None, "route all" vs. explicit allowlist, "missing" badge for allowlisted-but-not-installed)
  - **Hot models** — multi-select of models kept pinned warm
  - Per-model actions: delete, pin hot, install (→ Screen 3 pre-filtered to this node)
- **Node actions:** restart backend, update agent, disable/enable in routing pool, remove node
- **Recent activity:** requests routed here (model, latency, status), errors, circuit-breaker events

## Screen 3 — Model install flow

The differentiator. No competing router does VRAM-aware model provisioning.

1. **Search** HuggingFace GGUF models (endpoint exists). Results: model name, params, family, available quants.
2. **Fit verdict** — for a selected model+quant, a per-node verdict computed from live node telemetry (`herd fit` math):
   - ✓ Fits — max context ~N tokens, full GPU
   - ◐ Fits with offload — K layers to CPU, expect slower
   - ✗ Won't fit — short reason (VRAM, disk)
   This per-node verdict grid — "what does this 20GB download actually get me on each GPU, before I download it" — is the screenshot feature. Give it visual weight.
3. **Install** — pick target node(s) → confirm (download size, disk after) → progress states: queued → downloading (%, resumable) → verifying → loading → live. Progress arrives via polling; design for coarse updates, not smooth streams.
4. **Ollama blob extraction** (exists) — a secondary path: "reuse this model from Ollama's storage on the same node" instead of re-downloading. Surface it as a suggestion when applicable, not a top-level flow.

## Full feature inventory (nothing may be lost)

Existing dashboard behaviors that must have a home in the new IA:

- Tabs today: Backends, Analytics, Sessions, Fleet, Models, Agent Guide, Settings (Fleet is the default tab)
- Header: infrastructure summary (Fleet Nodes Online w/ fallback to Router Backends Online), gateway version footer, update badge, strategy badge, connection-pulse indicator, refresh countdown
- Backend cards: health dot, GPU metrics via gpu-hot (auto-hides when unreachable), model lists
- Per-backend model routing allowlists (`models_enabled`), hot-model selection, "missing" badges
- Config overrides table (scope/key/value/updated/delete) — "No overrides — running pure herd.yaml" empty state
- Settings config editor with secret redaction (GET/PUT `/admin/config`)
- Analytics: request totals, latency p50/p95/p99, model counts, backend counts, timeline chart (Chart.js), hours selector
- Sessions: agent session list, session modal, WebSocket event stream
- Models: HF GGUF search, VRAM-compatibility hints, download modal, download-to-Ollama-node
- Agent Guide: static API reference content for AI agents
- API key: entered in UI, persisted in localStorage, gates admin actions (401 states throughout)
- Self-enroll command generation (becomes the join-token flow)
- Modals close on Escape and overlay-click; toast notifications for action results
- Auto-refresh: 5s status, 30s analytics, 10s sessions, per-tab refresh start/stop, last-tab persistence

New capabilities the design must accommodate (in flight, per control-plane roadmap):

- One-time join tokens + Add Node flow
- Fleet-wide agent update (version authority exists; needs UI)
- Task progress via heartbeat polling (install/restart/update as coarse-grained task states)
- Per-node fit verdicts (`herd fit` engine)

## Deliverables

1. **Design system:** color tokens (dark), type scale + faces, spacing, the accent decision, component set (cards, tables, badges, buttons, modals, toasts, empty states, charts direction), health/state color semantics (healthy/degraded/offline/circuit-open must be distinguishable and colorblind-safe)
2. **Wordmark + small-size mark direction** (using the provided icon)
3. **Three screens** at desktop width, with key states: Fleet overview (populated + empty/first-run + update-in-flight), Node detail (full telemetry + degraded/no-telemetry), Model install (search + fit-verdict grid + install progress)
4. Notes on any pattern that needs care in vanilla JS (e.g., list virtualization for 100+ models — prefer patterns that don't need it)

## Out of scope

- Light mode, mobile layouts, marketing site, the tray app UI (tray only consumes the small-size mark)
- Redesigning the icon
- Copywriting beyond empty/error states
