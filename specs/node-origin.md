# Spec: Node Origin — explicit provenance for pool backends

## 1. Problem statement

Backend provenance (static config vs. enrolled registration vs. live agent) is
encoded only in a name-prefix convention (`agent:*`, `node:*`, bare) spread
across `pool_sync.rs`, `nodes/health.rs`, and config validation. Nothing
downstream can ask "is this node managed?" without string matching, and the
API/dashboard cannot show users which of their nodes are managed vs. plain
endpoints — which is the entire progressive-enhancement story Herd sells.
Promote origin to an explicit, type-checked field and surface it over the API.

## 2. Non-goals

- Deprecating or folding the `node:` enrolled tier. That is a separate product
  decision (see §8); this spec surfaces `Enrolled` as-is.
- Dashboard UI changes. The dashboard redesign track consumes the API field;
  this spec ends at the JSON boundary.
- Changing pool key naming. The `agent:`/`node:` prefixes remain as stable keys
  and reserved namespaces; they just stop being load-bearing for logic.
- Any routing behavior change. Origin is informational in this spec.

## 3. Interfaces / contracts

```rust
// src/backend/pool.rs (or a shared types location) — new public enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeOrigin {
    /// Declared in herd.yaml `backends:` or `discovery.static_nodes`.
    Static,
    /// Registered via POST /api/nodes (herd-tune), polled by NodeHealthPoller.
    Enrolled,
    /// Live `herd agent` heartbeating into NodeRegistry, synced by AgentPoolSync.
    Agent,
}
```

```rust
// BackendPool constructor takes origin at insert time; existing signatures grow
// one parameter (no Option — every caller knows its origin):
impl BackendPool {
    pub fn new(backends: Vec<Backend>, failure_threshold: u32, recovery_time: Duration) -> Self;
    // ^ unchanged; config-loaded backends are stamped Static internally.

    pub async fn add(&self, backend: Backend, origin: NodeOrigin);      // was add(&self, Backend)
    pub async fn origin_of(&self, name: &str) -> Option<NodeOrigin>;    // new
}
```

HTTP — existing endpoints gain a field (additive, no version bump):

```
GET /api/status          → each backend entry gains  "origin": "static" | "enrolled" | "agent"
GET /api/nodes           → each node entry gains     "origin": ...
GET /api/nodes/{id}      → response gains            "origin": ...
```

CLI:

```
herd status   → node table gains an ORIGIN column (values: static | enrolled | agent)
```

## 4. Data shapes

```rust
// src/backend/pool.rs
pub struct BackendState {
    pub config: Backend,
    pub origin: NodeOrigin,          // NEW — set at insert, never mutated
    pub healthy: bool,
    // ... existing fields unchanged (gpu_metrics, queue_depth, vram_free_mb, ...)
}
```

Serialized example (`GET /api/status`, one backend):

```json
{
  "name": "agent:bastion-01",
  "url": "http://100.64.0.2:8080",
  "backend": "llama-server",
  "origin": "agent",
  "healthy": true,
  "queue_depth": 0,
  "vram_free_mb": 4096
}
```

## 5. Invariants

- Every `BackendState` has exactly one `NodeOrigin`, assigned at insertion and
  immutable for the entry's lifetime. A node that "upgrades" (static entry later
  covered by an agent) exists as two pool entries with distinct keys and
  origins — same as today.
- Insert-site mapping is total and fixed. `pool.add` has **five** callers, not
  three — the original spec's "no other insertion path exists" was wrong:

  | Call site | Origin |
  |-----------|--------|
  | `BackendPool::new` (config load) | `Static` |
  | `api/admin.rs:142` (runtime add-backend API) | `Static` |
  | `api/admin.rs:528` (config-reload reconcile) | `Static` |
  | `nodes/health.rs:144` (`NodeHealthPoller::sync_to_pool`) | `Enrolled` |
  | `nodes/pool_sync.rs:95` (`AgentPoolSync::reconcile`) | `Agent` |

  Adding a sixth without an origin is a compile error (no `Option`, no default).
- After this change, zero logic branches on `name.starts_with("agent:")` /
  `starts_with("node:")` outside of (a) config validation of reserved prefixes
  (`config.rs:1442`) and (b) **key construction** in the two reconcilers
  (`health.rs:107,120`; `pool_sync.rs:49,64`).

  Specifically **not** exempt: the reconcilers' *ownership sweeps*
  (`health.rs:112`, `pool_sync.rs:55`), which use the prefix to decide "is this
  stale entry mine to remove?" That is origin logic wearing a string-match
  costume, and it is exactly what `origin_of()` exists to replace. Those two
  sites must convert.
- Telemetry field semantics are unchanged: `queue_depth`/`vram_free_mb`/`ttft_p50_ms`
  are `Some` only for `Agent` entries — but consumers check the `Option`, not
  the origin.
- `origin` serializes lowercase and round-trips through serde.

## 6. Acceptance criteria

- AC1. Given a herd.yaml with one `backends:` entry, when the gateway starts,
  then `GET /api/status` shows that backend with `"origin": "static"`.
- AC2. Given a node registered via `POST /api/nodes`, when `NodeHealthPoller`
  syncs it, then its pool entry and `GET /api/nodes` row report
  `"origin": "enrolled"`.
- AC3. Given a fresh agent heartbeat in `NodeRegistry`, when `AgentPoolSync::reconcile`
  runs, then the `agent:{node_id}` pool entry reports `"origin": "agent"`.
- AC4. Given any pool entry, when queried repeatedly across health flaps and
  reconcile ticks, then its `origin` value never changes.
- AC5. `herd status` output contains an ORIGIN column with the correct value
  for each of the three origins (integration test against a mock gateway).
- AC6. A search of `src/` for prefix-based logic branches
  (`starts_with("agent:")` / `starts_with("node:")`) finds **only**
  `config.rs:1442` (reserved-prefix validation) and the four key-construction
  sites (`health.rs:107,120`, `pool_sync.rs:49,64`) — i.e. exactly five hits,
  and neither ownership sweep among them. Enforced by an automated check (unit
  test via `include_str!` or a CI grep step — builder's choice).
- AC8. Given a pool holding one `Enrolled` and one `Agent` entry, when
  `AgentPoolSync::reconcile` runs a sweep with no fresh agents, then the `Agent`
  entry is removed and the `Enrolled` entry survives — the sweep selects by
  `origin_of()`, and a hypothetical enrolled node whose hostname begins with
  `agent` is no longer collateral damage.
- AC9. Given a backend added at runtime via the add-backend API
  (`admin.rs:142`), then `GET /api/status` reports `"origin": "static"`.
- AC7. serde round-trip test: `NodeOrigin::Agent` → `"agent"` → `NodeOrigin::Agent`.

## 7. Failure modes

- Unknown `origin` string arrives in a deserialization context (e.g. future
  version skew on a persisted status snapshot) → serde error propagates; the
  entry is rejected and logged at WARN with the offending value. Never a
  silent default to `Static`.
- A code path attempts to insert a backend without an origin → does not
  compile (parameter is non-optional). This is the required behavior.
- API consumer predates the field → additive JSON field is ignored by old
  consumers; no compatibility shim needed. Verified by AC1–AC3 asserting the
  rest of the payload is unchanged.

## 8. Open questions / assumptions

BLOCKING: (none)

NON-BLOCKING:
- Assumption: the `Enrolled` tier survives this sprint unchanged. The Director
  has an open product question about folding herd-tune enrollment into either
  static-with-metadata or the agent path; if he rules to deprecate it, that is
  a follow-up spec — this enum makes that deprecation easier (delete a
  variant, compiler finds every consumer).
- Assumption: origin lives on `BackendState`, not on `Backend` (config struct),
  because config-declared backends are definitionally `Static` and putting it
  in the serde config surface would let users lie about it.
- Assumption: no dashboard work here; the redesign brief picks up the field.

## 9. Gate (run last)

- [x] Every section 1-8 filled (N/A entries justified)?
- [x] Every interface in §3 a concrete signature, not prose?
- [x] Every data shape in §4 a concrete type/schema, not a description?
- [x] Every acceptance criterion in §6 testable (maps to an assertion)?
- [x] Every failure mode in §7 paired with a required behavior?
- [x] §8 BLOCKING list empty?

GATE RESULT: PASS
