# Spec: Pin Retirement — herd stops asserting model residency

> Supersedes `warmer-retirement.md`. That spec gated the background half of the
> pinning machinery and left the request-path half running; see §1.

Depends on residency-signal.md (the unpin sweep must target genuinely resident
models, never the on-disk list).

## 1. Problem statement

Herd pins models into VRAM through **two** mechanisms, introduced together by
`docs/superpowers/plans/2026-03-13-keep-alive-hot-models.md` — one plan, whose
stated goal was *"centrally inject `keep_alive: "-1"` into proxied Ollama
requests and replace ModelHoming with a proactive ModelWarmer."*

**(a) The warmer** — `src/backend/warmer.rs`. Every 240s it POSTs
`{"model": m, "prompt": "", "keep_alive": -1}` (`warmer.rs:78-86`) to each
Ollama backend's `/api/generate` for each configured `hot_models` entry. Runs by
default, no off switch.

**(b) Request-path injection** — `inject_keep_alive` (`src/server.rs:1225-1247`),
called from the proxy at `server.rs:1504`. Stamps `routing.default_keep_alive`
into **every** proxied Ollama request on `/api/generate` and `/api/chat`.
Default `"5m"` (`config.rs:504`).

(b) is the sharper doctrine violation and the one no prior spec addressed. The
warmer touches only `hot_models` on a timer; injection touches **every model on
every request**, from inside the request path — residency decided by traffic,
which is precisely what the static-placement doctrine forbids. Both
`server.rs:1237` and `warmer.rs:79` carry the same comment about serializing
`-1` as an integer for older Ollama, and `docs/LOCAL_TESTING_GUIDE.md:36` sets
`default_keep_alive: "-1"` — so the indefinite-pin configuration is real and
in use, not hypothetical. Under it, every proxied request permanently pins
whatever model it touched.

Neither mechanism ever *releases* a pin. `keep_alive: -1` is indefinite and
Ollama is a separate long-lived daemon, so pins outlive the herd process that
set them. An operator who upgrades to a warmer-disabled build still has VRAM
held hostage by the previous run, indefinitely, with no code path in herd that
will ever let go — and those stale pins actively block the placement swaps this
doctrine puts operators in charge of.

Gate both mechanisms off by default, and release what previous runs pinned.

## 2. Non-goals

- Deleting `BackendType::Ollama` support. Ollama backends stay first-class
  routing targets; only residency assertion is retired.
- Deleting `warmer.rs` or `inject_keep_alive`. Both stay, gated and documented
  as legacy, until Ollama support itself sunsets.
- Removing dim 23 (`warm_model_recency`) or `last_served` stamping — see
  residency-routing.md. This spec only removes the warmer's dim-23 stamp call
  along with the warm requests when disabled.
- Evicting models. Releasing a pin ≠ unloading (§3, unpin sweep).

## 3. Interfaces / contracts

### (a) Warmer gate

```rust
// src/config.rs
pub struct ModelWarmerConfig {
    /// LEGACY (Ollama only). Periodically pings Ollama backends with
    /// keep_alive: -1 to prevent unloading. Off by default: under the
    /// static-placement doctrine, residency is owned at placement time.
    #[serde(default)]                    // default = false  (BREAKING: was always-on)
    pub enabled: bool,

    #[serde(default = "default_warmer_interval")]
    pub interval_secs: u64,              // unchanged, 240

    #[serde(default = "default_warmer_timeout")]
    pub timeout_secs: u64,               // unchanged, 180
}
```

`ModelWarmer::start` keeps its signature. The existing "interval == 0 disables"
path stays as a second disable route for back-compat; `enabled` is the primary
gate.

### (b) Request-path injection gate

```rust
// src/config.rs — RoutingConfig
/// LEGACY (Ollama only). When Some, herd stamps this keep_alive into every
/// proxied Ollama /api/generate and /api/chat request. None (the default)
/// leaves the field alone and lets the backend apply its own policy.
/// A negative value is an INDEFINITE pin asserted from the request path —
/// doctrine-violating; permitted, but warned about loudly at startup.
#[serde(default)]                        // default = None  (BREAKING: was "5m")
pub default_keep_alive: Option<String>,
```

`inject_keep_alive` keeps its signature but returns the body untouched when the
configured value is `None`.

### (c) Unpin sweep

```rust
// src/backend/warmer.rs (alongside the warmer it retires)
/// One-shot, startup-only. For each Ollama backend, replaces any indefinite
/// pin with a finite TTL so previously pinned models age out naturally.
/// Never runs from the request path. Never issues a load.
pub async fn release_pins(pool: &BackendPool, client: &reqwest::Client);
```

Sweep contract:
- Targets `BackendType::Ollama` backends only.
- Targets **`resident_models`** (residency-signal.md) — the live `/api/ps` list.
  **Never `models`**: that is the on-disk catalog, and POSTing to it would load
  the entire library.
- Sends `keep_alive: "5m"` (a finite TTL), **not** `keep_alive: 0`. Releasing a
  pin must not evict: `0` unloads immediately and would blow away models an
  operator loaded deliberately outside herd.
- Runs once at startup, only when at least one Ollama backend reports a
  non-empty `resident_models`. Logs one INFO naming what it released.

Config surface (herd.yaml) after this spec:

```yaml
routing:
  # default_keep_alive: "5m"   # omitted → herd does not assert residency
model_warmer:
  enabled: true                # opt-in; omitted or false → warmer never spawns
  interval_secs: 240
  timeout_secs: 180
```

## 4. Data shapes

Covered by §3: `enabled: bool` on `ModelWarmerConfig`, and
`default_keep_alive: String → Option<String>` on `RoutingConfig`. No new types,
no persisted state.

## 5. Invariants

- With `model_warmer.enabled: false` (the default), the warmer task is never
  spawned: zero warm requests, zero `last_served` stamps from the warmer, for
  any backend type.
- With `routing.default_keep_alive` unset (the default), no proxied request body
  is modified by `inject_keep_alive` — the `keep_alive` key is absent from the
  forwarded body unless the **caller** put it there. A caller-supplied
  `keep_alive` passes through untouched; herd never overrides it.
- With `model_warmer.enabled: true`, behavior is byte-identical to today's
  warmer for Ollama backends; llama-server / openai-compat are still skipped.
- Startup logs exactly one WARN per enabled legacy mechanism, naming the
  doctrine. A `default_keep_alive` that parses to a negative number logs an
  additional WARN naming it an indefinite request-path pin.
- The unpin sweep issues at most one request per `(Ollama backend, resident
  model)` pair, exactly once per process lifetime, never from the request path.
- The sweep never causes a load: every model it touches was already in
  `resident_models`.
- No code path outside `warmer.rs` calls `warm_url` / `warm_payload`.

## 6. Acceptance criteria

- AC1. Given a default config (no `model_warmer` block), when the gateway runs
  for > `interval_secs`, then zero warm requests to any backend's
  `/api/generate` are observed (mock-backend integration test).
- AC2. Given `model_warmer.enabled: true` and one Ollama backend, when one
  interval elapses, then exactly one warm request with `keep_alive: -1` is
  observed — current behavior preserved (existing tests adapted, not deleted).
- AC3. Given `model_warmer.enabled: true` and one llama-server backend, when one
  interval elapses, then that backend receives zero warm requests.
- AC4. Given `enabled: true`, when the gateway starts, then the deprecation WARN
  appears exactly once in logs.
- AC5. Given `enabled: false` and `interval_secs: 240`, the "Model warmer
  disabled" info line is logged and no task is spawned.
- AC6. Config round-trip: a herd.yaml omitting `model_warmer` deserializes to
  `enabled == false` with unchanged interval/timeout defaults.
- AC7. Given no `routing.default_keep_alive`, when a request is proxied to an
  Ollama backend's `/api/chat`, then the forwarded body has **no** `keep_alive`
  key (mock backend asserts the received payload).
- AC8. Given a caller body that already contains `"keep_alive": "10m"` and no
  configured default, when proxied, then the forwarded body still reads
  `"keep_alive": "10m"`.
- AC9. Given `routing.default_keep_alive: "5m"`, when proxied to Ollama
  `/api/chat`, then `keep_alive: "5m"` is injected — legacy behavior preserved
  on opt-in (existing injection tests adapted).
- AC10. Given `routing.default_keep_alive: "-1"`, when the gateway starts, then
  an indefinite-pin WARN naming the setting appears exactly once.
- AC11. Given an Ollama backend with `resident_models: ["a","b"]` and 40 models
  in `models`, when the gateway starts, then exactly 2 unpin requests are
  observed, both carrying a finite `keep_alive`, and none carrying `0`
  (regression guard against both the load-the-library and the evict-everything
  failure modes).
- AC12. Given an Ollama backend with empty `resident_models`, when the gateway
  starts, then zero unpin requests are issued.
- AC13. Given the gateway runs for an hour after startup, then no further unpin
  requests are observed beyond AC11's (one-shot, not periodic).

## 7. Failure modes

- Existing Ollama user upgrades and their models start cold-unloading →
  expected, documented break. Required behavior: CHANGELOG entry + README
  migration note (`model_warmer.enabled: true` and/or
  `routing.default_keep_alive: "5m"` to restore). The deprecation WARNs make a
  restored config self-documenting.
- `enabled: true` with zero Ollama backends → warmer ticks, finds nothing, does
  nothing. Log at DEBUG only; no warning spam.
- Warm or unpin request times out / errors → logged, non-fatal. The unpin sweep
  does **not** retry (it is best-effort; the pin expires when Ollama restarts
  anyway) and never marks the backend unhealthy — health is the health-checker's
  job.
- Unpin sweep runs before the first discovery tick populates `resident_models`
  → it observes an empty list and does nothing. Required behavior: schedule the
  sweep after the first successful discovery pass, not at bare process start.
  A missed sweep is recoverable (next restart); a sweep against a stale-empty
  list that silently no-ops while the operator believes pins were released is
  not — so log at INFO when the sweep finds nothing, naming why.
- `hot_models` was changed at runtime via `PUT /admin/config/backends/:name`
  (`admin.rs:622-639`) while the warmer was enabled → models pinned under the
  *previous* `hot_models` are not in the current config. Sweeping
  `resident_models` rather than `hot_models` covers them by construction; this
  is the reason for that choice.

## 8. Open questions / assumptions

BLOCKING: (none)

NON-BLOCKING:
- Assumption: gate-off rather than delete, because `BackendType::Ollama` is
  still supported and external users exist (26 stars, 3 forks — some run Ollama
  fleets). Deleting is a follow-up whenever Ollama support sunsets.
- Assumption: default-off is the right polarity for both mechanisms even though
  it breaks running Ollama configs. Doctrine wins; each break is one config line
  to undo, and the WARNs document the restored state.
- Assumption: release-to-finite-TTL beats release-to-zero. Evicting on startup
  would be a nastier surprise than the pin it fixes, and a finite TTL restores
  exactly the pre-herd baseline.
- Assumption: `"5m"` is the right finite TTL for the sweep (it matches both
  Ollama's own default and herd's outgoing default), so a swept fleet behaves as
  if herd had never pinned it.

## 9. Gate (run last)

- [x] Every section 1-8 filled (N/A entries justified)?
- [x] Every interface in §3 a concrete signature, not prose?
- [x] Every data shape in §4 a concrete type/schema, not a description?
- [x] Every acceptance criterion in §6 testable (maps to an assertion)?
- [x] Every failure mode in §7 paired with a required behavior?
- [x] §8 BLOCKING list empty?

GATE RESULT: PASS
