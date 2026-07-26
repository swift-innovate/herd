# Spec: Residency Signal — make "resident" mean resident

## 1. Problem statement

Every other spec in this sprint assumes the pool knows which models are
*actually loaded* on a backend. It does not — for Ollama backends it knows which
models are **on disk**.

`ModelDiscovery::discover_models` (`src/backend/discovery.rs:198-225`) populates
`BackendState.models` from:

| Backend | Endpoint | Meaning |
|---------|----------|---------|
| `LlamaServer` / `OpenAICompat` | `/v1/models` | the model it loaded and serves — **resident** ✅ |
| `Ollama` (static/enrolled) | `/api/tags` | everything on disk — **available, not resident** ❌ |
| any, `Agent` origin | `caps.models_loaded` via `AgentPoolSync` (`nodes/pool_sync.rs:79`) | genuinely loaded — **resident** ✅ |

`BackendState.current_model` *is* live residency (`/api/ps`) but
`discover_running` (`discovery.rs:240`) keeps only `.first()`, discarding the
rest — Ollama holds several models concurrently. So today there is **no accurate
multi-model residency signal for a directly-discovered Ollama backend.**

Consequences, if the rest of the sprint lands on the current field:

- residency-routing.md's hard filter reads `b.models.contains(model)`
  (`scored.rs:663`, `pool.rs:278`). For an Ollama backend that predicate is
  *"the box has it on disk"*, so the router happily dispatches to a node that
  must **load the model in the request path** — the doctrine's first invariant
  violated, while the spec's own acceptance criteria pass. Silent, and exactly
  the failure the sprint exists to prevent.
- pin-retirement.md's unpin sweep, if pointed at `models`, would POST a
  `keep_alive` to every model on disk and **load the entire library**.

Fix the signal before anything consumes it.

## 2. Non-goals

- Changing what `models` means. It stays the advertised/available list — it is
  the right input for `/v1/models`, the dashboard's model catalog, and the D2
  `models_enabled`/`model_filter` allowlist. This spec adds a second field.
- Making Ollama report residency it does not expose. `/api/ps` is the only
  source; this spec reads all of it instead of one entry.
- Changing poll intervals or discovery scheduling.
- Routing behavior. Consumers land in residency-routing.md.

## 3. Interfaces / contracts

```rust
// src/backend/pool.rs
pub struct BackendState {
    /// Advertised / available models. Ollama: /api/tags. llama-server &
    /// openai-compat: /v1/models. Agent: reported models_loaded.
    /// Feeds the model catalog and the D2 allowlist filter. UNCHANGED.
    pub models: Vec<String>,

    /// NEW. Models actually loaded and serving RIGHT NOW. The ONLY field any
    /// router may filter on. Empty = nothing resident (not "unknown").
    pub resident_models: Vec<String>,

    /// Retained for the API surface only (`/status`, `/admin/backends`);
    /// now derived as `resident_models.first()`. Never read by routing.
    pub current_model: Option<String>,
}

impl BackendPool {
    /// Replaces `update_current_model`. Sets `resident_models` and re-derives
    /// `current_model`. Callers must pass the FULL resident list.
    pub async fn update_resident_models(&self, name: &str, models: Vec<String>);

    /// True residency predicate. Routers call this, never `models.contains`.
    pub async fn is_resident(&self, name: &str, model: &str) -> bool;
}
```

Population contract, exhaustive by backend type and origin:

```
Ollama (Static | Enrolled)  → GET /api/ps  → ALL entries (not .first())
LlamaServer  | OpenAICompat → GET /v1/models → all entries (it serves what it loaded)
any Agent origin            → caps.models_loaded  (already true residency)
```

HTTP — additive, no version bump:

```
GET /api/status → each backend entry gains "resident_models": ["qwen3-32b", ...]
                  existing "models" and "current_model" unchanged in shape
```

## 4. Data shapes

Covered by §3. No new types, no persisted state — `resident_models` is
in-memory pool state refreshed on the existing discovery tick, exactly like
`models`.

Serialized example (`GET /api/status`, one Ollama backend):

```json
{
  "name": "node:citadel",
  "backend": "ollama",
  "models": ["qwen3-32b", "llama3:8b", "mistral:7b"],
  "resident_models": ["qwen3-32b"],
  "current_model": "qwen3-32b"
}
```

## 5. Invariants

- `resident_models ⊆ models` for llama-server/openai-compat/agent backends. For
  Ollama it is `⊆ models` in practice (you cannot run what isn't pulled) but the
  code must not *assume* it — a model pulled and run between two discovery ticks
  can appear in `/api/ps` before `/api/tags` is re-read. Never intersect the two.
- `current_model == resident_models.first().cloned()` at all times.
- An unreachable backend leaves `resident_models` **unchanged** (stale), and the
  health checker marks it unhealthy — routing excludes it on health, not on an
  emptied residency list. A failed probe must never be read as "nothing resident."
- No code path in this spec issues a request that could cause a load: `/api/ps`,
  `/api/tags`, and `/v1/models` are all read-only.
- After this spec, `grep -rn 'models.contains' src/router/ src/backend/pool.rs`
  returns zero routing-path hits; all residency predicates go through
  `resident_models` / `is_resident`.

## 6. Acceptance criteria

- AC1. Given a mock Ollama backend whose `/api/tags` lists 3 models and whose
  `/api/ps` lists 1, when discovery runs, then `models.len() == 3` and
  `resident_models == ["<the ps one>"]`.
- AC2. Given a mock Ollama `/api/ps` listing **two** running models, when
  discovery runs, then both appear in `resident_models` (regression test for the
  `.first()` truncation at `discovery.rs:240`).
- AC3. Given a llama-server backend, when discovery runs, then `models` and
  `resident_models` are equal.
- AC4. Given an agent heartbeat carrying `models_loaded: ["a","b"]`, when
  `AgentPoolSync::reconcile` runs, then `resident_models == ["a","b"]`.
- AC5. Given a backend with `resident_models: ["x"]`, when its next discovery
  probe fails (connection refused), then `resident_models` is still `["x"]` and
  the backend is marked unhealthy.
- AC6. `current_model` equals `resident_models.first()` after each of AC1–AC4.
- AC7. `GET /api/status` includes `resident_models` and leaves `models` /
  `current_model` byte-identical to pre-change for the same fixture.

## 7. Failure modes

- `/api/ps` unreachable or malformed while `/api/tags` succeeds → keep the
  previous `resident_models`, log WARN once per backend per transition. Never
  clear the list, never fall back to `models` (that is the availability/residency
  conflation this spec exists to kill).
- Ollama version predating `/api/ps` → probe 404s; treat as above (stale-keep +
  WARN). Required behavior: the backend stays routable only for models already
  known resident; it does not silently become "everything on disk is resident."
- A model appears in `/api/ps` but not `/api/tags` (pulled + run between ticks)
  → both fields are written independently from their own sources; no
  reconciliation, no error. Explicitly allowed by §5.
- Agent reports an empty `models_loaded` → `resident_models` becomes empty and
  the node is correctly unroutable for model-bearing requests. This is a true
  signal, not a failure — distinct from the unreachable case above.

## 8. Open questions / assumptions

BLOCKING: (none)

NON-BLOCKING:
- Assumption: a second field beats redefining `models`. `models` has three live
  consumers (catalog, dashboard, D2 allowlist) that genuinely want availability;
  redefining it would break them silently. Two fields, two meanings, both honest.
- Assumption: stale-keep on probe failure is right, versus clearing. Clearing
  converts a transient network blip into a fleet-wide 404 storm the moment
  residency becomes a hard filter. Health is the liveness signal; residency
  is a placement signal. Keep them separate.
- Assumption: no new poll. `discover_running` already probes `/api/ps` on the
  discovery tick; this spec reads its full response instead of one entry, so
  the cost is unchanged.

## 9. Gate (run last)

- [x] Every section 1-8 filled (N/A entries justified)?
- [x] Every interface in §3 a concrete signature, not prose?
- [x] Every data shape in §4 a concrete type/schema, not a description?
- [x] Every acceptance criterion in §6 testable (maps to an assertion)?
- [x] Every failure mode in §7 paired with a required behavior?
- [x] §8 BLOCKING list empty?

GATE RESULT: PASS
