# Spec: Legacy Router Residency — the other four routers learn what a model is

Depends on residency-signal.md (`resident_models`) and residency-routing.md
(`RouteError`, proxy error mapping, hard-filter semantics).

## 1. Problem statement

residency-routing.md's invariant is *"no router, under any strategy, returns a
backend without the requested model resident."* Three of the five routers cannot
satisfy it today because they **never look at the model at all**:

| Router | Signature | Model handling |
|--------|-----------|----------------|
| `PriorityRouter` | `priority.rs:21` | `_model: Option<&str>` — ignored |
| `LeastBusyRouter` | `least_busy.rs:21` | `_model: Option<&str>` — ignored |
| `WeightedRoundRobinRouter` | `weighted_round_robin.rs:27` | `_model: Option<&str>` — ignored |
| `ModelAwareRouter` | `model_aware.rs:26-45` | filters, then **falls back to priority** (`:47-55`) |
| `ScoredRouter` | `scored.rs:657-681` | hard filter (done in residency-routing.md) |

So this is not "delete a fallback in one router." Three routers need residency
filtering built from scratch, and one needs its fallback removed.

The pool cannot support it as written either. The only residency-filtered
primitive is `get_by_model_tagged_excluding` (`pool.rs:269-281`), and it orders
by `min_by(least_busy_cmp)` — **least-busy, not priority**. There is no
residency-filtered + priority-ordered accessor and no residency-filtered +
weight-ordered one, so Priority and WRR cannot preserve their own character
under a residency filter. (The original spec asserted the pool functions were
"unchanged in shape"; against this tree that is not achievable.)

This also blocks model-classes.md AC3, which requires the Priority strategy to
pick the first-listed member's node *ignoring queue depth* — impossible with
only a least-busy-ordered primitive.

## 2. Non-goals

- The scored router — done in residency-routing.md.
- Class/alias resolution — model-classes.md.
- Changing each strategy's *character*. Priority still prefers highest priority,
  LeastBusy still prefers idlest, WRR still distributes by weight — each now
  does so over a residency-filtered candidate set instead of the healthy set.
- Model-less request behavior. `model: None` keeps today's exact semantics in
  all four routers.
- Deprecating any strategy.

## 3. Interfaces / contracts

Router trait signatures unchanged. New pool primitives — the candidate set
becomes a first-class result rather than a pre-reduced single pick:

```rust
// src/backend/pool.rs
impl BackendPool {
    /// Healthy ∧ ¬excluded ∧ tags⊆ ∧ `model` ∈ resident_models.
    /// The shared residency gate for all four legacy routers; each then applies
    /// its own selection rule to the returned set. Empty vec = nothing resident.
    pub async fn candidates_for_model(
        &self,
        model: &str,
        tags: &[String],
        excluded: &HashSet<String>,
    ) -> Vec<BackendState>;

    /// Residency-filtered + priority-ordered (max priority wins).
    pub async fn get_by_model_priority_excluding(
        &self, model: &str, tags: &[String], excluded: &HashSet<String>,
    ) -> Option<BackendState>;
}
```

`get_by_model_tagged_excluding` keeps its current least-busy ordering and its
name stops being a lie by documentation: it is the *least-busy* model-filtered
accessor, and `ModelAwareRouter` / `LeastBusyRouter` are its callers.

Per-router semantics for `model = Some(name)`:

| Router | Candidate set | Selection within set |
|--------|---------------|----------------------|
| Priority | `candidates_for_model` | max `config.priority` |
| ModelAware | `candidates_for_model` | `least_busy_cmp` (unchanged rule, fallback deleted) |
| LeastBusy | `candidates_for_model` | `least_busy_cmp` |
| WRR | `candidates_for_model` | weighted round-robin over the filtered set's weights |

Empty candidate set → `Err(RouteError::ModelNotResident { .. })` in all four.

WRR detail: the cumulative-weight walk (`weighted_round_robin.rs:45-61`) must
recompute `total_weight` from the **filtered** set, not the healthy set, or the
slot arithmetic overruns and the `.expect("slot must fall within total_weight")`
at `:61` panics. That `expect` is also a library-code panic path and should
become a `RouteError` return regardless.

## 4. Data shapes

No new types and no persisted state. `candidates_for_model` returns
`Vec<BackendState>` — the same clone-on-read shape the existing accessors use.

## 5. Invariants

- For a model-bearing request, all four routers return only backends with the
  model in `resident_models`. An empty set is `ModelNotResident`, never a
  fallback to priority, least-busy, or any unfiltered pick.
- `model: None` behavior is byte-identical to pre-change in all four routers.
- Each router's selection rule over the filtered set is the same rule it applies
  today over the healthy set — filtering changes the *input*, never the
  *ordering*.
- WRR's weight arithmetic is derived solely from the filtered set; no panic path
  survives in library code.
- No `route*` path issues, schedules, or implies a load. Reads pool state only.

## 6. Acceptance criteria

- AC1. Given A (llama3 resident, priority 50) and B (llama3 **not** resident,
  priority 100), when routing `model=Some("llama3")` under **each** of Priority,
  ModelAware, LeastBusy, WRR, then A is returned — never B. Four assertions.
- AC2. Given no backend with llama3 resident and healthy B available, when
  routing `model=Some("llama3")` under each of the four, then the result is
  `Err(RouteError::ModelNotResident)`. This **inverts** the existing
  `falls_back_to_priority` test (`model_aware.rs:97-110`) and the fallback leg
  of `mixed_fleet_routes_to_correct_backend` (`model_aware.rs:194-196`) — both
  are rewritten, not deleted.
- AC3. Given A (resident, priority 100, deep queue) and B (resident,
  priority 50, idle), when routing under **Priority**, then A wins; under
  **LeastBusy**, then B wins. Proves each strategy kept its character over a
  filtered set, and that the new priority-ordered primitive is actually wired.
- AC4. Given three resident backends with weights 50/30/20 and one non-resident
  weight-1000 backend, when routing WRR 100 times, then dispatch distribution
  matches 50/30/20 over the three and the weight-1000 backend is never selected
  (proves `total_weight` came from the filtered set).
- AC5. Given `model: None`, when routing under each of the four, then existing
  model-less tests pass **unmodified** (`priority.rs:57-126`,
  `least_busy.rs:59-105`, WRR's own).
- AC6. Given a filtered set that empties mid-retry as `excluded` grows, when the
  proxy loop exhausts it, then the terminal result is 404 `model_not_resident`,
  not 503 — and the loop is bounded by pool size.
- AC7. Given an Ollama backend with llama3 in `models` but not in
  `resident_models`, when routing under each of the four, then
  `ModelNotResident` (same regression guard as residency-routing.md AC10,
  applied to the legacy path).
- AC8. `grep -rn 'expect(' src/router/` returns no hits on a routing path
  (WRR's slot `expect` removed).

## 7. Failure modes

- Model resident nowhere → `ModelNotResident` → 404, per residency-routing.md's
  contract. Identical error surface across all five strategies; a caller cannot
  tell which strategy the gateway runs from the error, and should not be able to.
- WRR filtered set non-empty but total weight 0 (all filtered backends have
  `priority: 0`) → existing "All healthy backends have zero weight" error
  (`weighted_round_robin.rs:47`), now scoped to the filtered set. Keep it
  distinct from `ModelNotResident`: the models *are* placed, the weights are
  misconfigured, and the operator needs to know which.
- A strategy is switched at runtime via config reload while requests are in
  flight → unchanged from today; the router is swapped behind `RwLock` and the
  next request uses the new one. No new mechanism.
- Existing Ollama users on `priority` / `least_busy` / `weighted_round_robin`
  discover that the `model` field now matters → this is a **second, broader
  break class** than the ModelAware fallback removal: for these three strategies
  the model field previously had no effect at all, so a wrong or unplaced model
  name that used to route fine is now a 404. `ModelAware` is the default
  strategy (`config.rs:495`) so most users hit the documented break, but this
  one needs its own CHANGELOG paragraph naming the three strategies.

## 8. Open questions / assumptions

BLOCKING: (none)

NON-BLOCKING:
- Assumption: a shared `candidates_for_model` primitive beats four bespoke
  filters. One residency predicate, one place to keep honest as
  residency-signal.md evolves; each router keeps only its selection rule.
- Assumption: returning `Vec<BackendState>` (cloned) is acceptable at routing
  hot-path cost, matching the existing accessors' clone-on-read pattern. If
  profiling says otherwise, the scored router's borrow-under-one-read-guard
  pattern (`scored.rs:642-651`) is the template to copy — but do not
  pre-optimize on speculation.
- Assumption: WRR's counter stays global rather than per-model. Per-model
  round-robin state would be more "correct" for a multi-model fleet but adds
  keyed state for a strategy that is already the least-used; revisit only if
  asked.

## 9. Gate (run last)

- [x] Every section 1-8 filled (N/A entries justified)?
- [x] Every interface in §3 a concrete signature, not prose?
- [x] Every data shape in §4 a concrete type/schema, not a description?
- [x] Every acceptance criterion in §6 testable (maps to an assertion)?
- [x] Every failure mode in §7 paired with a required behavior?
- [x] §8 BLOCKING list empty?

GATE RESULT: PASS
