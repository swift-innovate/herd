# Herd — Working TODO

> Scratchpad for in-flight work. Milestone tracking in `ROADMAP.md`.

**Last updated:** 2026-07-03

---

## ACTIVE — GUI/tray spec: PR #G1 + #G2 (two feat: commits)

Spec: `docs/specs/gui-tray-spec.md`. Fixes CITADEL's 84-model sprawl via a
GUI-managed `models_enabled` allowlist + a SQLite config overlay (YAML stays the
hand-edited base). #G1 = data model + overlay; #G2 = admin API. Do NOT start #G3–#G5.

Non-negotiables (CLAUDE.md): defaults-off / zero change for existing `herd.yaml`;
never `bail!` on config; no `unwrap()` in lib; backend-agnostic routing; `cargo test` +
`cargo clippy` green before EACH commit; existing tests stay green.

### #G1 — models_enabled + config overlay store  (commit 1: feat:)
- [ ] config.rs: `models_enabled: Option<Vec<String>>` (default None) + `enabled: bool`
      (default true) on Backend; Default impl + exhaustive `Backend {}` literals.
- [ ] config.rs: `Config::apply_overrides(&mut self, &[(scope,key,value_json)])` per D3
      (backend:{name} keys models_enabled/hot_models/priority/enabled; `definition` append,
      YAML-wins-on-collision + warn; parse errors warn+skip; pure/testable).
- [ ] discovery.rs: pure `filter_models(...)` per D2 order; discover_models calls it.
- [ ] pool.rs: filter_healthy also requires `b.config.enabled`.
- [ ] db.rs: migration v6 `config_overrides` + CRUD (set/get/delete/list).
- [ ] server.rs: startup opens node_db before pool + apply_overrides; reload_config merges.
- [ ] Tests: filter precedence, Some(vec![]) empties, None byte-identical, merge determinism,
      collision warn, override round-trip.

### #G2 — Admin config API  (commit 2: feat:)
- [ ] D4 endpoints minus detect/pull; api_key guard; PUT writes override + patches AppState +
      nudges discovery; null clears; skills.md + Agent Guide; tests (401, round-trip, null,
      404, reload survives).
