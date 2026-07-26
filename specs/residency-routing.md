# Spec: Residency Routing — hard filter and error contract (scored router + proxy)

> Narrowed from the original single spec. The four legacy routers need residency
> built from scratch, not a fallback removed — that is a materially larger job
> and now lives in legacy-router-residency.md. This spec lands the semantics,
> the error type, and the HTTP contract on the scored path first, so
> model-classes.md has a stable error surface to build on.

Depends on residency-signal.md — without it, `resident_models` does not exist and
the "hard filter" would filter on Ollama's on-disk catalog (see §1).

## 1. Problem statement

Under static placement a model is either serving on a node or it is not. Two
things stand in the way, on the scored path:

- **The gate defaults to soft.** `ModelGate::Relaxed` is the default
  (`config.rs:224-233`) and drops the residency predicate entirely when no
  candidate holds the model (`scored.rs:669-676`), routing to an arbitrary
  healthy backend. That only made sense when the backend would load on demand.
  `ModelGate::Strict` already exists and already implements the hard filter —
  it is simply not the default.
- **The error is thrown away.** `server.rs:1548` does
  `.map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?`, collapsing every routing
  failure into a bodyless 503. A caller cannot tell "your model isn't placed
  anywhere" from "the fleet is down."

Note what is **already true** and needs no work: `warm_model_recency` (dim 23)
already defaults to `0.0` (`config.rs:322`, `scored.rs:927`) — the original
spec's claim that it "was non-zero" was wrong about this tree. And dim 1
`model_resident` already self-neutralizes within the scored set (`scored.rs:336-343`).

## 2. Non-goals

- The four legacy routers (Priority / ModelAware / LeastBusy / WRR) — see
  legacy-router-residency.md.
- Class/alias resolution — model-classes.md. Here a requested model name is
  matched literally against `resident_models`.
- Removing dim 23's code, the `last_served` map, or the `ScoredWeights` field.
  The machinery stays for operators who override (multi-model llama-server
  `--model-alias` setups still benefit).
- Changing behavior for requests with **no** model specified. Priority/tag
  fallback for model-less requests is correct and unchanged.
- Placement tooling (`herd fit`, agent-applied plans).

## 3. Interfaces / contracts

Router trait signatures unchanged. New semantic contract for the scored router:

```rust
//   model = Some(name) → candidate set is EXACTLY the healthy, non-excluded,
//     tag-matching backends with `name` in `resident_models`. Empty set → Err —
//     never fall through to a backend without the model.
//   model = None → existing priority/tag behavior, unchanged.
```

Default flip:

```rust
// src/config.rs — ModelGate
#[derive(..., Default)]
pub enum ModelGate {
    #[serde(rename = "relaxed")]
    Relaxed,                 // was #[default]
    #[default]               // NOW the default (BREAKING)
    #[serde(rename = "strict")]
    Strict,
}
```

New error type, so callers can map to a real status instead of a generic 502/503:

```rust
// src/router/mod.rs
#[derive(Debug, thiserror::Error)]
pub enum RouteError {
    #[error("model '{model}' is not resident on any healthy backend{tags_note}")]
    ModelNotResident { model: String, tags_note: String },
    #[error("no healthy backends available")]
    NoBackends,
}
```

Routers return `Err(RouteError::…​.into())`; the proxy downcasts and maps.

Proxy error mapping — **this is a return-type change, not a one-line edit.**
Both call sites currently discard the error into a bare `StatusCode`
(`server.rs:1548`, `api/openai.rs:410`) and the handlers' error type is
`StatusCode`, which cannot carry a JSON body. The handlers must return a type
implementing `IntoResponse`:

```
POST /v1/chat/completions {"model": "<not resident anywhere>"}
  → 404 {"error": {"message": "model '<name>' is not resident on any healthy backend",
                   "type": "model_not_found", "code": "model_not_resident"}}

(pool empty / nothing healthy)
  → 503 {"error": {"message": "no healthy backends available",
                   "type": "server_error", "code": "no_backends"}}
```

### 3a. Backend-404 handling — bounded, self-healing, honestly terminated

Three defects in the current retry loop (`server.rs:1536-1626`), all of which
this spec must fix or its own §7 promises are false:

**(i) The terminal error is a lie.** If every attempt fails, `response` stays
`None` and the handler returns **502 Bad Gateway** (`server.rs:1946-1955`) — not
`model_not_resident`. So a pure placement problem surfaces to the caller as a
generic gateway error today, and simply adding `RouteError` does not change that:
the 502 is emitted *after* the loop, on a path that never sees the routing error.

**(ii) The attempt budget does not scale with the candidate space.** The loop is
`for _ in 0..=retry_count()` with `retry_count` defaulting to 2
(`config.rs:501`) — three attempts, total, regardless of how many backends hold
the model. It is not infinite, but it is also not "until candidates exhaust,"
which is what both this spec and model-classes.md assume when they promise a
terminal `ModelNotResident` / `ClassUnservable`.

**(iii) Retrying a 404 is load-on-demand reasoning, and it amplifies loads.**
`server.rs:1583-1591` excludes the backend and retries, commented *"model likely
evicted by Ollama, another backend may still have it warm."* Under static
placement each such hop dispatches the request to another backend that — for
Ollama — may then **load the model in the request path**. One client request
becomes several loads. The routing fix alone does not close this; the retry path
reopens it.

Required behavior:

```rust
// Separate budget: general failures (5xx, connection errors) keep retry_count.
// A model-endpoint 404 is placement drift, not a transport failure, and gets
// exactly ONE re-route per request, tracked independently.
const MAX_RESIDENCY_REROUTES: u32 = 1;
```

On a 404 from a model endpoint, in order:

1. **Repair the pool.** Remove the requested model from that backend's
   `resident_models` (new `BackendPool::drop_resident_model(&name, &model)`).
   The pool was wrong; now it is not. Do **not** mark the backend unhealthy —
   it answered.
2. **Re-route at most once.** Because step 1 corrected residency, the re-route
   cannot re-select that backend for this model, and it returns
   `ModelNotResident` naturally if nobody else holds it.
3. **Terminate honestly.** When the re-route budget is spent or routing returns
   `ModelNotResident`, the response is the 404 contract below — never the
   generic 502. The post-loop `None` arm must distinguish "no backend accepted"
   (502, genuine upstream failure) from "routing had nothing left" (404).

This makes §7's terminal-state promise true, bounds 404 handling at one extra
hop regardless of `retry_count` or class size, and converts each drift event
into a pool correction rather than a fleet-wide shopping trip.

Dim 1 weight, now that residency is enforced purely by elimination:

```rust
// src/config.rs
fn w_model_resident() -> f64 { 0.0 }   // was 5.0
```

It is dead config under a hard filter — the highest weight in the table
(`config.rs:328`) attached to a dimension that provably cannot rank
(`scored.rs:336-343`). Zero the default; keep the field and catalog entry so
existing herd.yaml files still parse and `sanitize_weights` still accepts an
operator override.

## 4. Data shapes

No new persisted state. `RouteError` is the only new type. `ScoredWeights` shape
unchanged — only the `Default` of `model_resident` changes (`warm_model_recency`
is already 0.0). `BackendState` gains nothing here (residency-signal.md added
`resident_models`).

## 5. Invariants

- For any request carrying a model name, every backend the scored router can
  return has that model in **`resident_models`** at candidate-selection time.
- No code path reachable from `route*` issues, schedules, or implies a model
  load, pull, or backend process operation. Routing reads pool state only.
- An empty candidate set is an error, never a silent fallback. The error names
  the model and, when tags constrained the search, says so.
- `ScoredWeights::default().model_resident == 0.0` and
  `.warm_model_recency == 0.0`; `sanitize_weights` still accepts overrides > 0
  for both. The "all Phase-1 weights zero → restore defaults" guard
  (`scored.rs:930-943`) must be re-checked: with `model_resident` now defaulting
  to 0.0 it is one fewer non-zero term, so confirm the guard cannot misfire on a
  legitimate config.
- Model-less requests route exactly as before (existing tests pass unmodified).
- Retry/exclusion loops terminate with `ModelNotResident` when candidates
  exhaust — never with the generic 502. A 502 means an upstream genuinely failed
  to answer; a 404 means placement is wrong. The two must stay distinguishable
  to the caller.
- Model-endpoint 404s cost at most one extra upstream request per client
  request, independent of `routing.retry_count` and of how many backends hold
  the model. Every 404 leaves the pool's residency view strictly more accurate
  than it found it.

## 6. Acceptance criteria

- AC1. Given backends A (llama3 in `resident_models`) and B (higher priority,
  llama3 not resident), when routing `model=Some("llama3")` under the scored
  strategy, then A is returned — never B.
- AC2. Given no backend with llama3 resident and healthy B available, when
  routing `model=Some("llama3")`, then the result is
  `Err(RouteError::ModelNotResident)` — B is not returned.
- AC3. Given AC2's state, when `POST /v1/chat/completions` with
  `"model": "llama3"`, then HTTP 404, body `"code": "model_not_resident"`, and
  the model name present in the message.
- AC4. Given an empty/all-unhealthy pool, when any request is proxied, then HTTP
  503 with `"code": "no_backends"` — distinguishable from AC3 by both status and
  code.
- AC5. Given `model: None`, when routing, then behavior is identical to
  pre-change (existing model-less tests pass unmodified).
- AC6. `ScoredWeights::default().model_resident == 0.0` and
  `.warm_model_recency == 0.0` (unit assert); a config overriding
  `warm_model_recency` to 0.3 sees dim 23 participate (existing dim-23 formula
  tests pass with explicit weight injection).
- AC7. Given two residency-valid candidates where one has lower `queue_depth`,
  when routing scored with default weights, then the lower-queue candidate wins
  (proves scoring still differentiates *within* the filtered set).
- AC8. Given a resident-model backend that is unhealthy, when routing with that
  model, then it is excluded and, if it was the only holder, `ModelNotResident`
  is returned (residency does not override health).
- AC9. Given a herd.yaml with `routing.scored.model_gate: relaxed`, when routing
  a non-resident model, then the old fallback behavior still applies — the
  escape hatch survives the default flip.
- AC10. Given an Ollama backend with llama3 in `models` (on disk) but **not** in
  `resident_models`, when routing `model=Some("llama3")`, then
  `ModelNotResident` — not a dispatch to that backend. This is the regression
  guard for the conflation residency-signal.md fixes; without it the hard filter
  silently permits a request-path load.
- AC11. Given backends A and B both reporting llama3 resident, and A 404s the
  request, when proxied, then: A's `resident_models` no longer contains llama3,
  exactly **one** re-route occurs, and B serves the request. Counting assertion —
  the mock fleet must observe exactly two upstream requests, not three.
- AC12. Given AC11's setup but with B also 404ing, when proxied, then the client
  receives **404 `model_not_resident`** — not 502 — and both A and B have had
  llama3 dropped from `resident_models`. Regression guard for defect (i).
- AC13. Given `routing.retry_count: 10` and six backends all reporting llama3
  resident and all 404ing, when proxied, then exactly **two** upstream requests
  are observed (initial + one re-route), not seven. Proves the 404 budget is
  independent of `retry_count` — defect (ii).
- AC14. Given a backend that returns **502** (not 404), when proxied with
  `retry_count: 2`, then the general retry budget still applies (up to three
  attempts) and `resident_models` is left untouched — the two budgets are
  genuinely separate, and a transport failure is not misread as placement drift.

## 7. Failure modes

- Requested model resident nowhere → `ModelNotResident` → 404
  `model_not_resident`. Fail loud with the model name; never proxy to a backend
  that will 400/500 in its own dialect.
- All holders unhealthy or excluded mid-retry → same 404 after the exclusion
  loop exhausts; the error is the terminal state of retries, not a per-attempt
  502.
- Pool empty entirely → `NoBackends` → 503.
- Backend's residency list is stale (model swapped at placement time, pool not
  yet refreshed) → the backend 404s. **DECIDED (2026-07-26): a 404 gets its own
  budget of exactly one re-route, and it repairs the pool on the way through.**
  See §3a below for the contract and the three problems it fixes.
- Operator sets `warm_model_recency` > 0 with static placement → harmless (all
  candidates hold the model; recency differentiates multi-model nodes).

## 8. Open questions / assumptions

BLOCKING: (none)

NON-BLOCKING:
- Assumption: 404 (not 400/503) for non-resident models, matching OpenAI's
  unknown-model status so client SDKs surface it as model-not-found.
- Assumption: flipping `ModelGate`'s default beats deleting `Relaxed`. The
  escape hatch costs nothing, is already implemented and tested, and gives an
  Ollama shop a one-line rollback while they fix placement.
- Assumption: `thiserror` is acceptable (or hand-rolled `Display`/`Error` impls
  to keep the tree lean — builder's choice; the contract is the variant shape).
- Assumption: zeroing dim 1's default is safe because it provably cannot rank
  inside a hard-filtered candidate set. If `Relaxed` is in use the dim *can*
  rank (resident 1.0 vs relaxed-in 0.0) — so a `Relaxed` operator who wants the
  old tie-break must set the weight explicitly. Called out in CHANGELOG.

## 9. Gate (run last)

- [x] Every section 1-8 filled (N/A entries justified)?
- [x] Every interface in §3 a concrete signature, not prose?
- [x] Every data shape in §4 a concrete type/schema, not a description?
- [x] Every acceptance criterion in §6 testable (maps to an assertion)?
- [x] Every failure mode in §7 paired with a required behavior?
- [x] §8 BLOCKING list empty?

GATE RESULT: PASS
