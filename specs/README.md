# Sprint: Static Placement Doctrine

Aligns Herd with the current architecture doctrine:

1. **Placement time** (rare, deliberate): operators decide which models live on
   which GPUs, informed by `herd fit` / `pground place`. Agents apply and hold
   the plan.
2. **Route time** (constant): dispatch over what is resident. Herd never
   initiates a model load in the request path. Requests ask for a *capability*
   (model class); resolution goes upward to resident capacity, never downward
   into a load.

## Specs (build order)

| # | Spec | Depends on | Size |
|---|------|-----------|------|
| 1 | [node-origin.md](node-origin.md) | — | S |
| 2 | [residency-signal.md](residency-signal.md) | — | S |
| 3 | [pin-retirement.md](pin-retirement.md) | 2 | M |
| 4 | [residency-routing.md](residency-routing.md) | 2 | M |
| 5 | [legacy-router-residency.md](legacy-router-residency.md) | 4 | L |
| 6 | [model-classes.md](model-classes.md) | 2, 4, 5 | L |

1 and 2 are independent and can land in any order. 2 is a hard prerequisite for
everything downstream — see below. 4 changes routing semantics that 5 extends
and 6 builds on. Each spec passes the spec-first gate independently; each is a
self-contained build task for a coding agent.

### Revised after a repo review (2026-07-26)

The original four-spec plan was written before anyone read the routing and
discovery code against it. Three structural problems came out of that review;
the spec set now reflects them.

**`resident` did not mean resident.** `BackendState.models` is populated from
`/api/tags` for Ollama backends — everything on disk, not what is loaded
(`discovery.rs:206-211`). Every "hard residency filter" in the sprint reads that
field, so as originally written the filter would have cheerfully dispatched to a
node that then loads the model in the request path: the doctrine's first
invariant violated *while every acceptance criterion passed*. residency-signal.md
(new, #2) adds a true `resident_models` field and is a prerequisite for the rest.

**Pinning was two mechanisms, and the spec retired the wrong one.** The warmer
(background, `hot_models`, 240s) was gated off; `inject_keep_alive`
(`server.rs:1225`, called from the proxy at `:1504`) was not even mentioned —
and it stamps a `keep_alive` into **every** proxied Ollama request. Both came
from one 2026-03-13 plan. `warmer-retirement.md` is superseded by
pin-retirement.md (#3), which covers both mechanisms and adds the unpin sweep
that releases what previous runs pinned indefinitely.

**Residency routing was one M-sized spec doing two jobs.** Three of five routers
(`priority.rs:21`, `least_busy.rs:21`, `weighted_round_robin.rs:27`) bind
`_model` and ignore it entirely — they have no fallback to remove, they need
residency built from scratch, plus new pool primitives that do not exist. Split
into residency-routing.md (#4: scored router, `RouteError`, the HTTP contract)
and legacy-router-residency.md (#5: the other four, sized L).

**The retry path reopened the hole the routing fix closes.** A model-endpoint 404
currently excludes the backend and re-routes (`server.rs:1583`), commented *"model
likely evicted by Ollama, another backend may still have it warm"* — load-on-demand
reasoning. Each hop can trigger a real request-path load. Worse, if every attempt
fails the handler returns a generic **502** (`server.rs:1946-1955`), so the 404
contract both routing specs promise was unreachable, and the loop's
`0..=retry_count` bound (default 2) meant neither spec's "until candidates exhaust"
could ever hold. residency-routing.md §3a now gives 404s their own one-re-route
budget that repairs `resident_models` on the way through and terminates honestly.

Smaller corrections folded into the existing specs: `pool.add` has five callers,
not three (node-origin §5); the reconcilers' ownership sweeps are origin logic,
not key construction, and must convert (node-origin AC6); `warm_model_recency`
already defaults to 0.0, so that acceptance criterion was a no-op, while dim 1
`model_resident` — weight 5.0, the highest in the table — becomes dead config
under a hard filter and nobody had noticed (residency-routing §3); and
model-classes' body rewrite has to move inside the proxy retry loop, because
`rewrite_request_model` currently runs *before* routing (model-classes §3).

### Design decisions taken during review

- **404s get a separate, one-hop budget** (residency-routing.md §3a). Placement
  drift is not transport failure and must not be retried like it.
- **Class name collisions are a startup error**, not silent shadowing
  (model-classes.md §5, §8). Class taxonomies are inherently opinionated, so
  collisions are likely rather than exotic; the operator who holds the opinion
  resolves it with a rename.
- **Herd ships zero classes and hardcodes zero class names** (model-classes.md
  AC14). The `coder`/`chat`/`utility` names throughout these specs are
  illustrations, not an endorsed taxonomy.

Specs #1–#5 deliver the doctrine in full. #6 is a feature on top of it and is
safely deferrable — nothing in the first five depends on it.

## Doctrine invariants (apply to every spec)

- **No runtime loads.** No code path reachable from a proxied request may
  trigger a model load, pull, or process (re)start. This includes asserting
  `keep_alive` — manipulating eviction from the request path is placement by
  traffic.
- **Residency is a hard filter**, never a scored preference — and "resident"
  means `resident_models`, never the on-disk catalog.
- **Fail loud.** An unservable request errors with a diagnosable message; it is
  never silently absorbed by an unrelated backend.
- **No `unwrap()` in library code.** (`warmer.rs:21` and `:59` are current
  violations, in a file spec #3 touches — fix them there.)
- Existing behavior for `BackendType::Ollama` users is preserved only where a
  spec explicitly says so; doctrine wins otherwise, with the break documented.

## Breaks requiring CHANGELOG entries

Four distinct user-visible breaks, easy to conflate into one line and shouldn't be:

1. `model_warmer` off by default — Ollama models start cold-unloading (#3).
2. `routing.default_keep_alive` unset by default — herd stops asserting
   residency on every request (#3).
3. `ModelAwareRouter`'s load-implying fallback dies — a non-placed model is now
   a 404 on the **default** strategy (#4).
4. `priority` / `least_busy` / `weighted_round_robin` begin honouring the
   `model` field at all — on these three a wrong model name previously routed
   fine and now 404s (#5). Broadest blast radius of the four.
