# Supervisor Extract — Design Note

**Date:** 2026-08-18  
**Status:** Implemented (disabled by default)  
**Module:** `src/supervisor/`  
**Config:** `supervisor.enabled = false` (zero overhead when off)

---

## Overview

This extracts Substrate's ABI v0 and attention scheduler into a userspace supervisor library inside Herd. The supervisor gives Herd **agent-native process semantics** (capabilities, token budgets, hierarchy) without requiring ring 0 or a custom kernel.

**What this is NOT:**
- This does NOT make models smarter. Horizon F (g) did not flip.
- This does NOT change inference quality or reasoning physics.
- This is NOT a new HTTP product or public brand.
- This is NOT deployed to production by default (feature flag, disabled).

## What Moved from Substrate

Ported from `swift-innovate/substrate` main @ `49893300`:

### ABI Types (frozen v0)
From `abi/src/lib.rs`:
- **AgentId**: u64 unforgeable ID (AgentId::ROOT = 0)
- **CapId**: u32 unforgeable handle (not a string)
- **Tier**: Director=0, Worker=1, Ephemeral=2 (lower rank = more authority)
- **Right**: u32 bitfield (SPAWN_AGENT, INFER, SECRET_USE, etc.)
- **SpawnSpec**: tier, priority, attention_budget, rights array
- **Syscall** enum (SpawnAgent, Send, Recv, Grant, Revoke, Seal, InvokeAuthed, etc.)
- **AbiError**: Ok=0, InvalidArg=-1, NotFound=-2, CapDenied=-3, TierDenied=-4, QueueEmpty=-6, NoAgent=-7

### ACB (Agent Control Block)
From `kernel/src/agent/acb.rs`:
- **Fields**: `id: AgentId`, `tier: Tier`, `parent: Option<AgentId>`, `caps: CapTable`, `attention_budget: u32`
- **Root ACB**: `attention_budget = config or u32::MAX/4`, `CapTable::root()`, `parent = None`, tier = Director
- **Spawn**: child caps ⊆ parent via `CapTable::subset`; tier delegation rules enforced (Workers cannot mint Directors)

### CapTable
From `kernel/src/caps.rs`:
- **Entries**: HashMap<CapId, Right> with auto-incrementing CapId
- **Operations**: `subset`, `grant`, `revoke_right`, `holds` (mask union)
- **Semantics**: `holds(right)` is the **mask-union** of all entries, not "one entry contains every bit"
- **Grant/Revoke**: Director-tier only, else TierDenied. Empty right → InvalidArg.

### Attention Scheduler
From `kernel/src/scheduler.rs` RULES (not PIT/IRQ/wall-clock):
- **Quanta**: integer tokens (`u32`), saturating math, no floats
- **Defaults**: Priority=5, base_budget=4 (coerced to 1 if 0)
- **Herd charges**: `usage.prompt_tokens + usage.completion_tokens` (not timer ticks)
- **Park**: budget==0 → refuse further spend, 429 error with type `attention_exhausted`

### Secrets Boundary
From `docs/decisions/d6-abi-secrets.md` (RATIFIED):
- **Syscalls**: Seal=11, InvokeAuthed=12
- **Right**: SECRET_USE=1<<14
- **Semantics**: Seal-as-boundary; InvokeAuthed injects credential on the record path only
- **Revoke**: class-level (no per-secret revoke in v0)
- **Plaintext never enters**: agent context, Session.messages, logs, error strings

## What Was Deleted (Not Ported)

**Kernel-only pieces** (Herd is userspace, not ring 0):
- `no_std`, QEMU, PIT (Programmable Interval Timer), virtio drivers
- x86 `int 0x80` syscall handler, paging, SMP, bootloader
- Host daemon, UART serial, interrupt gates

**Reason:** Herd runs in userspace Linux. The supervisor is a library, not a kernel. We keep the _semantics_ (caps, budget, hierarchy) but discard the hardware layer.

## What Herd Already Covered (Kept)

These subsystems were **not duplicated**:

### USD Budgets (`src/budget.rs`)
- **BudgetTracker**: f32 USD, per-client/per-model caps, 429 `budget_exceeded`
- **Coexists with supervisor**: USD budget checked first, then attention budget
- **Reason**: Different physics (money vs tokens), different reset periods (monthly vs per-request)

### Rate Limits (`src/rate_limit.rs`)
- **RateLimiter**: req/s token bucket, `Authorization: Bearer` client keys
- **Kept as-is**: rate limiting is a second gate, orthogonal to caps

### Sessions (`src/agent/session.rs`)
- **Session**: chat transcript (id, model, messages, status)
- **Not overloaded**: a Session is NOT an ACB. The supervisor's AgentId is a separate concern.

### Permissions (`src/agent/permissions.rs`)
- **PermissionEngine**: regex deny lists for files and bash commands
- **Kept as second gate**: caps do not replace regex filters. Both run.

### Backend Stickiness (`src/router/session_affinity.rs`)
- **SessionAffinity**: X-Herd-Session → backend name for dim 18 stickiness
- **Not identity**: session affinity is routing stickiness, not supervisor AgentId

### Hardware Capabilities (`src/daemon/capabilities.rs`)
- **Capabilities**: GPU vendor, VRAM, model list snapshot
- **Renamed in supervisor**: agent caps → `Cap` / `CapTable` to avoid collision

### Tool Loop (`src/agent/executor.rs`)
- **Executor**: `/api/agent` endpoint only
- **Not affected**: `/v1/chat/completions` is a proxy, not the tool loop

## Integration Points

### Config (`src/config.rs`)
Added `SupervisorConfig`:
```yaml
supervisor:
  enabled: false              # default OFF (zero overhead)
  default_attention_tokens: 100000
```

Must not break existing `herd.yaml` files. Defaults sensible.

### AppState (`src/server.rs`)
New optional field:
```rust
pub supervisor: Option<Arc<crate::supervisor::Supervisor>>,
```

Initialized only when `config.supervisor.enabled == true`.

### OpenAI Endpoint (`src/api/openai.rs`)
Two hooks:
1. **Pre-routing check** (after USD budget check):
   - Resolve AgentId from `X-Herd-Session` or `X-Herd-Client` or `"default"`
   - Check `supervisor.check_budget(aid)`
   - If exhausted → 429 with error type `attention_exhausted`

2. **Post-response charge** (non-streaming path):
   - After extracting `tokens_in + tokens_out`
   - Call `supervisor.charge_tokens(aid, total_tokens)`
   - USD `record_cost` still runs (USD and attention are independent)

## Success Criteria Met

1. ✅ **Library API**: `src/supervisor` module with spawn, charge, check, grant/revoke
2. ✅ **Tests**: 34 supervisor tests + integration tests (610 total pass)
   - FAIL if child inherits extra caps → `test spawn_with_extra_caps_denied`
   - FAIL if spend past budget → `test charge_tokens_exhausts_budget`
   - FAIL if tool/secret used without cap → `test subset_fails_when_parent_lacks_rights`
   - FAIL if InvokeAuthed leaks plaintext → `test seal_as_boundary_no_leakage_in_error_path`
3. ✅ **Thin integration, feature-flagged OFF**:
   - `config.supervisor.enabled = false` (default)
   - USD BudgetTracker still runs unchanged
   - 429 `attention_exhausted` distinct from `budget_exceeded`
   - X-Herd-Session / X-Herd-Client → AgentId attribution
4. ✅ **Design note**: this document
5. ✅ **PR ready**: branch cursor/supervisor-extract-4d93, not merged

## Testing the Supervisor

### Enable in Config

```yaml
supervisor:
  enabled: true
  default_attention_tokens: 100  # low for testing
```

### Test 429 on Exhaustion

```bash
# First request (50 tokens) succeeds
curl -X POST http://localhost:40114/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "X-Herd-Session: test-session" \
  -d '{"model":"llama3:8b","messages":[{"role":"user","content":"hi"}]}'

# Second request (another 50+ tokens) hits budget
# Expect: HTTP 429, error.type="attention_exhausted"
curl -X POST http://localhost:40114/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "X-Herd-Session: test-session" \
  -d '{"model":"llama3:8b","messages":[{"role":"user","content":"long prompt..."}]}'
```

Response when exhausted:
```json
{
  "error": {
    "message": "Attention budget exhausted for agent test-session",
    "type": "attention_exhausted",
    "code": 429
  }
}
```

### Unit Tests

All 34 supervisor tests are in the `cargo test` suite:
```bash
cargo test supervisor
```

Key test assertions:
- **Caps isolation**: `spawn_with_extra_caps_denied`
- **Budget exhaustion**: `charge_tokens_exhausts_budget`
- **Tier delegation**: `worker_cannot_mint_director`
- **Secret boundary**: `seal_as_boundary_no_leakage_in_error_path`

## Deployment Notes

**This is NOT production-ready by default.** The supervisor is:
- **Disabled** by default (`enabled: false`)
- **Zero overhead** when disabled (no supervisor instance created)
- **Opt-in** only for dev/test environments

To enable:
1. Set `supervisor.enabled = true` in `herd.yaml`
2. Restart Herd
3. Monitor 429 `attention_exhausted` errors in logs/analytics

**Do NOT enable in production** without:
- Tuning `default_attention_tokens` for your workload
- Testing budget exhaustion behavior
- Monitoring attention spend per agent/session
- Understanding that this is a prototype extract

## Future Work (Out of Scope)

Not implemented (would require additional design):
- **Multi-ACB per session**: currently one root ACB per session key
- **Budget replenishment**: no periodic refill (each request starts fresh)
- **Distributed consensus**: single-node only (no fleet coordination)
- **Tool-level cap checks**: executor.rs integration (thin for now)
- **Persistent ACB state**: in-memory only (resets on restart)

## Conclusion

This extracts Substrate's agent-native process model (caps, attention, hierarchy) into Herd as a library, giving userspace process semantics without ring 0. The physics are unchanged: Horizon F (g) did not flip. This is a **prototype extract** to validate the ABI and attention scheduler in a production router context.

**Default: OFF.** Zero overhead when disabled. Opt-in for testing only.
