# v0.4.0 — Complete

## Step 1: Prometheus-native metrics export
- [x] Create `src/metrics.rs` with `Metrics` struct (request counters by status/backend)
- [x] Implement `LatencyHistogram` with cumulative buckets (10–10000ms)
- [x] Wire `record_request()` into proxy_handler and chat_completions
- [x] Add `/metrics` endpoint rendering Prometheus exposition format
- [x] Tests (histogram_buckets_cumulative, records_and_renders_metrics)
- [x] Validate build

## Step 2: Request tracing with correlation IDs
- [x] Add `uuid` dependency for UUID v4 generation
- [x] Extract or generate `X-Request-Id` in proxy_handler and chat_completions
- [x] Forward `X-Request-Id` to upstream backends
- [x] Include `X-Request-Id` in response headers
- [x] Add `request_id` field to `RequestLog` (backward-compatible with serde defaults)
- [x] Tests (request_log serialization/deserialization with and without request_id)
- [x] Validate build

## Step 3: Log rotation and retention policies
- [x] Add `log_retention_days`, `log_max_size_mb`, `log_max_files` to `ObservabilityConfig`
- [x] Implement `rotate_if_needed(max_size_mb, max_files)` in Analytics
- [x] Wire rotation into cleanup task with configurable retention
- [x] Tests (config_defaults, config_deserializes_log_settings)
- [x] Validate build

## Step 4: Validation & version bump
- [x] All 34 tests pass
- [x] ROADMAP.md updated — v0.4.0 fully complete
- [x] Version bumped to 0.4.0
- [x] Ready to commit

## Review
- v0.4.0 is fully complete with all 3 roadmap items done
- Test coverage: 34 tests (up from 27 in v0.3.0)
- Key features: Prometheus metrics (counters + histogram), correlation IDs (X-Request-Id), log rotation
- Zero new external dependencies for metrics (in-memory atomics)
- Backward-compatible RequestLog (request_id uses serde default)
