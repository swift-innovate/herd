# v0.3.0 — Complete

## Step 1: Backend tagging + tag-based routing
- [x] Add `BackendPool::get_healthy_with_tags()` and tagged variants
- [x] Extend `Router` trait to accept `tags: Option<&[String]>`
- [x] Update all 4 router strategies
- [x] Extract `X-Herd-Tags` header in proxy handler and chat_completions
- [x] Tests (get_healthy_with_tags_filters, routes_with_tag_filter)
- [x] Validate build

## Step 2: Health check endpoint customization
- [x] Add `health_check_path` and `health_check_status` to Backend config
- [x] Modify HealthChecker to use per-backend configurable path/status
- [x] Tests (default_health_check_path, custom_health_check_config_deserializes)
- [x] Validate build

## Step 3: Hot-reload configuration
- [x] Refactor AppState: router in RwLock, timeout/retry in atomics
- [x] Add `AppState::reload_config()` — syncs backends, swaps router, updates settings
- [x] Add `POST /admin/reload` endpoint (auth required)
- [x] Add file-polling reload (30s mtime check)
- [x] Pass config_path from main.rs through to server
- [x] Validate build

## Step 4: Validation & version bump
- [x] All 27 tests pass
- [x] ROADMAP.md updated — v0.3.0 fully complete
- [x] Version bumped to 0.3.0
- [x] Ready to commit

## Review
- v0.3.0 is fully complete with all 10 roadmap items done
- Test coverage: 27 tests (up from 6 at session start)
- Key features: tag routing (X-Herd-Tags), health check config, hot-reload (file + API)
- Hot-reload reloads: backends, routing strategy, timeout, retry count
- Non-reloadable (by design): server host/port, circuit breaker thresholds, rate limit
