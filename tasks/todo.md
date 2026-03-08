# Auto-Update Feature — Complete

## Step 1: Add `self_update` dependency
- [x] Add `self_update` crate to Cargo.toml (v0.42, with archive-zip, archive-tar, compression features)
- [x] Validate build (85 new transitive deps)

## Step 2: Create `src/updater.rs` module
- [x] Implement `check_for_update()` — returns UpdateInfo with current/latest/update_available
- [x] Implement `perform_update()` — downloads + replaces binary from GitHub Releases
- [x] Implement `startup_update_check()` — background async notification on server start
- [x] Version comparison logic with `v` prefix handling
- [x] Register module in `lib.rs`
- [x] Tests (version_comparison_newer, version_comparison_same_or_older, version_comparison_handles_v_prefix)

## Step 3: CLI `--update` flag
- [x] Add `--update` flag to Cli struct in `main.rs`
- [x] When `--update` is passed, check + download + exit
- [x] Progress bar shown for CLI downloads

## Step 4: `POST /admin/update` endpoint
- [x] Add endpoint in `server.rs` (behind auth middleware)
- [x] Returns JSON with update status/result
- [x] Notifies that restart is required after update

## Step 5: Startup update check
- [x] Background `spawn_blocking` check on server start
- [x] Logs info message if update is available

## Step 6: Refactor `/update` endpoint
- [x] Replaced manual GitHub API call with `updater::check_for_update()`
- [x] Consistent version checking across CLI, API, and startup

## Step 7: Validation
- [x] Build passes
- [x] All 37 tests pass (34 existing + 3 new)
- [x] README updated with auto-update docs
- [x] Ready to commit

## Review
- Auto-update uses `self_update` crate for GitHub Releases integration
- `herd --update`: CLI-triggered update with progress bar
- `POST /admin/update`: API-triggered update (auth required), returns JSON
- `GET /update`: Check-only (no install), now uses same updater module
- Startup: background check with log notification
- `self_update` handles Windows binary replacement (rename trick) automatically
- Platform matching via target triple (e.g., x86_64-pc-windows-msvc)
- Requires GitHub Releases with platform-specific assets to function
