# Spec: Model Classes — ask for a capability, not a checkpoint

## 1. Problem statement

Callers must name a concrete checkpoint, which couples every client (VALOR
operatives, external tools) to fleet placement and made the old
"load a smaller model for smaller work" behavior seem necessary. A **model
class** is an operator-defined, ordered set of acceptable models for a
capability (`coder`, `chat`, `utility`). A caller puts the class name in the
OpenAI `model` field; Herd expands it to the class's *resident* members and the
router picks among them. Resolution goes upward to resident capacity — a class
listing a 27B serves "8B-tier" work on the 27B — and never triggers a load.

Depends on residency-signal.md (`resident_models`), residency-routing.md (hard
residency filter, `RouteError`, proxy error mapping) and
legacy-router-residency.md (the four legacy routers' filtered candidate sets and
the priority-ordered pool primitive AC3 below requires).

## 2. Non-goals

- Capability inference. Herd never reasons "the 27B can cover the 8B" — class
  membership is the operator's explicit list, nothing else.
- Runtime loads. A class whose members are all non-resident is unservable.
- Per-class strictness knobs. One global `on_missing_model` switch only.
- Glob/regex members. v1 members are exact model names as backends report
  them. (Globs are a plausible v2; the config shape leaves room.)
- Classifier or routing-profile changes. Class names *are* model names, so
  `TierConfig.model` and `RoutingProfile.preferred_model` accept them with
  zero code changes there — verified by AC9, but no edits in those modules.
- Nested classes (a class as a member of another class) — rejected at
  validation, see §7.

## 3. Interfaces / contracts

Config surface (herd.yaml, top level):

```yaml
model_classes:
  classes:
    coder:
      description: "Code gen, review, refactoring"
      members:                # preference order; exact names; non-empty
        - qwen3-coder-next-80b
        - qwen3.6-35b-a3b
    utility:
      members:
        - bonsai-27b
        - qwen3.6-35b-a3b
  resolution:
    on_missing_model: error   # error (default) | fallback
    # fallback_class: chat    # required iff on_missing_model: fallback
```

```rust
// src/resolution.rs (new module)
/// What the caller's `model` field means after class expansion.
#[derive(Debug, Clone, PartialEq)]
pub enum ModelQuery {
    /// No model in the request — legacy priority/tag routing.
    Unspecified,
    /// Concrete model name; must be resident (residency-routing semantics).
    Exact(String),
    /// Class name; candidates are the members, in listed (preference) order.
    Class { name: String, candidates: Vec<String> },
}

/// Pure function: request's model field + config → query. No I/O, no pool
/// access. `requested` comes from the parsed request body (or classifier /
/// profile injection — they run first, unchanged).
pub fn resolve_model(requested: Option<&str>, cfg: &ModelClassesConfig) -> ModelQuery;
```

Router trait — `model: Option<&str>` is replaced by the query across all five
routers (the one real signature change in the sprint):

```rust
#[async_trait]
pub trait Router: Send + Sync {
    async fn route(&self, query: &ModelQuery, tags: Option<&[String]>) -> Result<RoutedBackend>;
    async fn route_excluding(&self, query: &ModelQuery, tags: Option<&[String]>,
        excluded: &HashSet<String>) -> Result<RoutedBackend>;
    async fn route_scored(&self, query: &ModelQuery, tags: Option<&[String]>,
        excluded: &HashSet<String>, ctx: &RouteContext) -> Result<RoutedBackend>;
}

// RoutedBackend carries what was actually chosen so the proxy can rewrite:
pub struct RoutedBackend {
    pub name: String,
    pub url: String,
    /// Concrete model serving the request. Some(..) when the query was a
    /// Class (or fallback) — the proxy MUST rewrite the body's "model" field
    /// to this before forwarding. None for Exact/Unspecified (no rewrite).
    pub resolved_model: Option<String>,
}
```

`RouteError` (from residency-routing.md) gains one variant:

```rust
#[error("no member of class '{class}' is resident on any healthy backend (members: {members:?})")]
ClassUnservable { class: String, members: Vec<String> },
```

### Where the rewrite happens (pipeline change, not a one-liner)

`rewrite_request_model` already exists (`api/openai.rs:41-53`) and already
satisfies two of §7's failure modes for free: it *inserts* `model` when absent
rather than erroring, and returns the body untouched when it won't parse. Reuse
it — do not write a second rewriter.

But it is currently called at `server.rs:1499`, **before** routing at
`server.rs:1541`, and both forwarded bodies are built once at
`server.rs:1502-1505` **outside** the retry loop. That ordering cannot express
this spec: `resolved_model` is only known *after* routing, and for a `Class`
query it can differ between retry attempts (attempt 1 picks member A's holder,
attempt 2 picks member B's). Required restructure:

1. Keep the existing pre-routing rewrite — it resolves classifier / profile /
   frontier mutations of `model_name` and is unrelated to classes.
2. Move body finalization **inside** the retry loop, after `route_scored`
   returns, and apply a second rewrite iff `resolved_model.is_some()`.
3. `inject_keep_alive` (if still enabled per pin-retirement.md) runs after that
   rewrite, per attempt, on the same body.

Cost: one extra JSON parse/serialize per retry attempt on class-routed requests
only. Acceptable; the alternative is a body whose `model` names a member the
retry target does not serve.

HTTP contract:

```
POST /v1/chat/completions {"model": "coder", ...}
  → proxied to chosen backend with "model" rewritten to e.g. "qwen3-coder-next-80b"
  → response streamed back unmodified (response "model" field shows the
    concrete name — deliberate: callers see what served them)

POST /v1/chat/completions {"model": "<class with no resident members>"}
  → 404 {"error": {"message": "...", "type": "model_not_found",
                    "code": "class_unservable"}}

GET /v1/models
  → data[] = union of today's entries + one entry per configured class:
    {"id": "coder", "object": "model", "owned_by": "herd/class"}
```

Routing semantics per strategy, for `ModelQuery::Class`:
- **Scored:** candidate set = every `(backend, member)` pair where `member` is
  resident on a healthy, tag-matching, non-excluded backend. Residency filter
  first (hard), then the 23-dim scorer ranks backends; among multiple resident
  members on the *same* backend, listed order picks the member. List position
  is NOT a scored dimension in v1 — it is only the intra-backend member
  tiebreak. (Rationale in §8.)
- **Legacy four (Priority/ModelAware/LeastBusy/WRR):** iterate members in
  listed order; first member with a non-empty candidate set routes under that
  strategy's existing rules. Deterministic, simple, preserves each strategy's
  character.

## 4. Data shapes

```rust
// src/config.rs
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelClassesConfig {
    #[serde(default)]
    pub classes: HashMap<String, ModelClass>,
    #[serde(default)]
    pub resolution: ResolutionConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelClass {
    #[serde(default)]
    pub description: Option<String>,
    /// Preference order. Validated non-empty; entries validated non-blank,
    /// deduplicated preserving first occurrence.
    pub members: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResolutionConfig {
    #[serde(default)]
    pub on_missing_model: OnMissingModel,
    /// Class absorbing unknown concrete names. Required iff Fallback.
    #[serde(default)]
    pub fallback_class: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OnMissingModel {
    #[default]
    Error,
    Fallback,
}
```

`Config` gains `#[serde(default)] pub model_classes: ModelClassesConfig`.
No persisted/DB state — classes are config-only, hot-reloaded with the rest of
herd.yaml if/where config reload exists.

## 5. Invariants

- `resolve_model` is pure and total: every `(requested, cfg)` maps to exactly
  one `ModelQuery` with no I/O.
- Resolution precedence is fixed: class name → `Class` (classes **shadow**
  concrete models); else known-pattern concrete name → `Exact`; else
  `on_missing_model` decides `Exact` (Error mode routes it, residency filter
  yields the 404) vs. `Class` of the fallback class. `None` → `Unspecified`.
  Note: in Error mode resolution does NOT consult residency — "missing" is
  decided by the router against live pool state, keeping `resolve_model` pure.
- A `Class` query only ever proxies to a `(backend, member)` pair where the
  member was resident at candidate-selection time. Members never cause loads.
- The proxied body's `"model"` field always names a concrete model the target
  backend serves: rewrite happens iff `resolved_model.is_some()`, exactly once,
  before forwarding, on both OpenAI and Ollama-compat proxy paths.
- **Herd ships zero classes and hardcodes zero class names.** `classes` defaults
  to empty; a gateway with no `model_classes` block behaves exactly as it did
  before this feature. No class name (`coder`, `chat`, `utility`, or any other)
  carries built-in meaning anywhere in the codebase — they are opaque map keys.
  The names in this spec's examples are illustrations, not a taxonomy Herd
  endorses. Enforced by AC14.
- Config validation (startup + reload) rejects, with an error naming the class:
  empty `members`; a member that names another class (no nesting);
  `fallback_class` unset or unknown while `on_missing_model: fallback`;
  a class name using the reserved `agent:`/`node:` backend prefixes;
  **a class name that collides with a model resident anywhere at validation
  time.** Collision is an error the operator resolves by renaming — not a
  silently-resolved precedence. A model that becomes resident *later* and
  collides cannot be caught at validation: that case logs a runtime WARN and
  resolves as the class (deterministic; see §8).
- `/v1/models` lists every configured class exactly once with
  `"owned_by": "herd/class"`, regardless of member residency (a temporarily
  unservable class is still advertised — it errors at call time, AC6).
- Streaming behavior, headers, and all non-`model` body fields pass through
  the rewrite untouched (byte-for-byte except the one JSON string value).

## 6. Acceptance criteria

- AC1. Given class `utility: [bonsai-27b, qwen3.6-35b-a3b]` with only
  `qwen3.6-35b-a3b` resident (on node B), when `POST /v1/chat/completions`
  with `"model": "utility"`, then the request is proxied to B with the body's
  model field rewritten to `"qwen3.6-35b-a3b"` (mock backend asserts received
  payload).
- AC2. Given both members resident on distinct healthy nodes with equal
  scores, when routing `utility` under the scored strategy, then dispatch is
  driven by the scorer over both candidates; forcing queue_depth high on the
  first-listed member's node routes to the other member (proves candidate set
  is the union, not first-match).
- AC3. Given both members resident and the first-listed member's node deep-
  queued, when routing under **Priority** strategy, then the first-listed
  member's node is still chosen (proves legacy first-match-in-listed-order
  semantics, distinct from AC2). Requires
  `get_by_model_priority_excluding` from legacy-router-residency.md — with only
  today's least-busy-ordered model accessor (`pool.rs:269-281`) this AC cannot
  pass, because the deep-queued node would lose on queue depth.
- AC4. Given a class with zero resident members, when called, then HTTP 404
  with `"code": "class_unservable"` and the member list in the message.
- AC5. Given `on_missing_model: error` (default), when `"model": "no-such-model"`
  (not a class, not resident), then 404 `model_not_resident`
  (residency-routing.md AC3 unchanged by this feature).
- AC6. Given `on_missing_model: fallback` + `fallback_class: chat`, when
  `"model": "gpt-4"` arrives, then it is served by a resident member of
  `chat` with the body rewritten; and `GET /v1/models` still lists `chat`.
- AC7. Given a config where class `llama3` collides with a model `llama3`
  resident at validation time, then **startup fails** with an error naming the
  collision. Given instead that `llama3` becomes resident only *after* a clean
  startup, then a runtime WARN names the collision and `"model": "llama3"`
  resolves as the class (deterministic).
- AC14. Given a build of Herd, then `grep -rn '"coder"\|"chat"\|"utility"' src/`
  finds no class-name special-casing outside tests and doc comments, and a
  gateway started with **no** `model_classes` block routes identically to one
  built before this feature (behavioral snapshot). Neutrality is testable, not
  aspirational.
- AC8. Config validation: empty members / nested class member / fallback mode
  without `fallback_class` each fail startup with an error naming the class
  (three unit tests).
- AC9. Given `task_classifier` tier `light` with `model: utility` and a
  request matching that tier's keywords, then the request routes per AC1
  semantics with **zero diffs** in `classifier.rs`/`profiles.rs` (integration
  test through the middleware stack).
- AC10. `GET /v1/models` includes `{"id": "coder", "owned_by": "herd/class"}`
  alongside concrete entries; concrete entries unchanged in shape.
- AC11. Streaming completion through a class rewrite: SSE chunks arrive
  unmodified and the terminal usage/model fields show the concrete model.
- AC13. Given a class `[a, b]` with `a` on node A and `b` on node B, when A
  returns 502 on the first attempt, then the retry reaches B with the body's
  model rewritten to **`b`** — not still `a` (regression guard for the
  rewrite-outside-the-retry-loop bug described in §3).
- AC12. `resolve_model` unit matrix: `None → Unspecified`; class name →
  `Class` with members in config order (dupes removed); non-class name →
  `Exact` in Error mode; non-class name → fallback `Class` in Fallback mode.

## 7. Failure modes

- Class unservable (no resident members) → 404 `class_unservable`, message
  lists members so the operator sees exactly what placement is missing. Never
  falls through to a non-member backend.
- All candidate backends fail mid-retry (exclusion loop) → terminal 404
  `class_unservable`. **Note the budget:** the loop does *not* walk every
  `(backend, member)` pair. Per residency-routing.md §3a, a model-endpoint 404
  buys exactly one re-route for the whole request (and repairs
  `resident_models` on the way), while general failures keep `retry_count`
  (default 2). A 5-member class spread over 8 backends therefore terminates
  after a couple of upstream requests, not 40 — deliberate: each extra hop is a
  potential request-path load on an Ollama backend, and class size must not
  multiply that. The earlier draft of this spec promised pair-exhaustion; that
  was never achievable against `for _ in 0..=retry_count()` and would have been
  the wrong behavior anyway.
- Body rewrite target field absent (Ollama-dialect request with no `model`,
  routed via classifier-injected class) → rewrite inserts the field rather
  than erroring; assert in AC9 path.
- Malformed/oversized body that can't be parsed for the model field → resolution
  yields `Unspecified` and existing no-model behavior applies; the rewrite step
  is skipped (`resolved_model` is `None` for `Unspecified` by construction).
- Config reload introduces an invalid classes block → reload is rejected,
  previous config stays live, error logged naming the class (same posture as
  existing config validation).
- Two members resident on one backend → listed order picks the member;
  deterministic, tested in scored-strategy unit tests.
- Class name equals a provider-routed model (frontier/provider config) →
  classes shadow providers too; the WARN from §5 collision detection covers
  it (providers are "resident" for collision-warn purposes).

## 8. Open questions / assumptions

BLOCKING: (none)

NON-BLOCKING:
- **DECIDED (2026-07-26): collisions are a startup error, not silent shadowing.**
  Class taxonomies are exactly the kind of thing every operator has a different
  opinion about, so name collisions are likely rather than exotic — and both
  silent resolutions are bad. Classes-win silently hides a real model; models-win
  makes resolution depend on what happens to be loaded right now, which is
  non-deterministic. Erroring at validation hands the decision to the operator
  who holds the opinion, costs one rename, and leaves runtime resolution
  deterministic (classes win) for the collision validation cannot see. The
  earlier draft's WARN-and-shadow is withdrawn.
- Corollary: a `class/` prefix namespace was considered and rejected. It makes
  collision structurally impossible but costs the ergonomics that justify the
  feature — callers would type `class/coder` instead of `coder`, and the whole
  point is that a caller names a capability as naturally as a model.
- Assumption: list position is only the intra-backend member tiebreak in the
  scored strategy, not a scored dimension. Rationale: with residency filtered
  and queue/context scored, a position dimension mostly re-encodes "prefer
  smaller" — the instinct this doctrine retires. If fleet experience shows we
  want it, adding dim 24 `class_position` is a contained follow-up.
- Assumption: response `model` fields show the concrete model, not the class.
  Callers learn what served them; observability beats symmetry. Veto → rewrite
  responses too (streaming makes that materially more invasive).
- Assumption: trait signature change (`Option<&str>` → `&ModelQuery`) over a
  compat shim. Five routers, one crate, no external trait consumers — churn is
  contained and the compiler drives the migration.
- Assumption: hot-reload of classes rides the existing config-reload path with
  no extra machinery; if herd.yaml reload doesn't cover the new block, restart
  is acceptable for v1.

## 9. Gate (run last)

- [x] Every section 1-8 filled (N/A entries justified)?
- [x] Every interface in §3 a concrete signature, not prose?
- [x] Every data shape in §4 a concrete type/schema, not a description?
- [x] Every acceptance criterion in §6 testable (maps to an assertion)?
- [x] Every failure mode in §7 paired with a required behavior?
- [x] §8 BLOCKING list empty?

GATE RESULT: PASS
