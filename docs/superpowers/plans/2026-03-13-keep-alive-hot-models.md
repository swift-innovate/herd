# keep_alive Injection + Hot Models Warmer — Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Centrally inject `keep_alive: "-1"` into proxied Ollama requests and replace `ModelHoming` with a proactive `ModelWarmer` that keeps declared `hot_models` loaded.

**Architecture:** Three isolated changes — config schema update, proxy body mutation, and a new background warmer — wired together in `server.rs`. `model_homing.rs` is deleted; `src/backend/warmer.rs` replaces it.

**Spec:** `docs/superpowers/specs/2026-03-13-keep-alive-hot-models-design.md`

**Tech Stack:** Rust, axum 0.7, reqwest 0.11, serde_json, tokio

---

## File Map

| Action | Path | Responsibility |
|---|---|---|
| Modify | `src/config.rs` | Add `default_keep_alive`, `ModelWarmerConfig`, `hot_models`; remove `default_model`, `idle_timeout_minutes` |
| Create | `src/backend/warmer.rs` | `ModelWarmer` — background task pinging `hot_models` |
| Modify | `src/backend/mod.rs` | Export `ModelWarmer` |
| Modify | `src/server.rs` | Wire `ModelWarmer`; inject `keep_alive` in `proxy_handler`; remove `ModelHoming` |
| Delete | `src/model_homing.rs` | Replaced by `warmer.rs` |

---

## Chunk 1: Config Schema

### Task 1: Update `RoutingConfig` and `Backend`

**Files:**
- Modify: `src/config.rs`

- [ ] **Step 1: Write the failing config tests**

  Add to `src/config.rs` tests block:

  ```rust
  #[test]
  fn routing_default_keep_alive_is_negative_one() {
      let config: Config = serde_yaml::from_str("{}").unwrap();
      assert_eq!(config.routing.default_keep_alive, "-1");
  }

  #[test]
  fn routing_keep_alive_configurable() {
      let yaml = "routing:\n  default_keep_alive: \"1h\"\n";
      let config: Config = serde_yaml::from_str(yaml).unwrap();
      assert_eq!(config.routing.default_keep_alive, "1h");
  }

  #[test]
  fn model_warmer_default_interval() {
      let config: Config = serde_yaml::from_str("{}").unwrap();
      assert_eq!(config.model_warmer.interval_secs, 240);
  }

  #[test]
  fn backend_hot_models_defaults_empty() {
      let yaml = "backends:\n  - name: x\n    url: http://x\n    priority: 50\n";
      let config: Config = serde_yaml::from_str(yaml).unwrap();
      assert!(config.backends[0].hot_models.is_empty());
  }

  #[test]
  fn old_default_model_field_silently_ignored() {
      // Old configs with default_model must not fail to parse
      let yaml = "backends:\n  - name: x\n    url: http://x\n    priority: 50\n    default_model: llama3:8b\n";
      let result: Result<Config, _> = serde_yaml::from_str(yaml);
      assert!(result.is_ok());
  }
  ```

- [ ] **Step 2: Run tests to confirm they fail**

  ```bash
  cargo test config:: -- --nocapture 2>&1 | tail -20
  ```

  Expected: compile error — `default_keep_alive`, `model_warmer`, `hot_models` don't exist yet.

- [ ] **Step 3: Implement config changes**

  In `src/config.rs`:

  **3a. Add `default_keep_alive` to `RoutingConfig` and remove `idle_timeout_minutes`:**

  ```rust
  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct RoutingConfig {
      #[serde(default = "default_strategy")]
      pub strategy: RoutingStrategy,

      #[serde(default = "default_timeout")]
      pub timeout: String,

      #[serde(default = "default_retry_count")]
      pub retry_count: u32,

      #[serde(default = "default_keep_alive_value")]
      pub default_keep_alive: String,
  }

  impl Default for RoutingConfig {
      fn default() -> Self {
          Self {
              strategy: default_strategy(),
              timeout: default_timeout(),
              retry_count: default_retry_count(),
              default_keep_alive: default_keep_alive_value(),
          }
      }
  }

  fn default_keep_alive_value() -> String {
      "-1".to_string()
  }
  ```

  Remove: `fn default_idle_timeout()`.

  **3b. Add `hot_models` to `Backend`, remove `default_model`:**

  ```rust
  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct Backend {
      pub name: String,
      pub url: String,
      pub priority: u32,

      #[serde(default)]
      pub hot_models: Vec<String>,

      #[serde(default)]
      pub gpu_hot_url: Option<String>,

      #[serde(default)]
      pub model_filter: Option<String>,

      #[serde(default)]
      pub health_check_path: Option<String>,

      #[serde(default)]
      pub health_check_status: Option<u16>,

      #[serde(default)]
      pub tags: Vec<String>,
  }

  impl Default for Backend {
      fn default() -> Self {
          Self {
              name: String::new(),
              url: String::new(),
              priority: 50,
              hot_models: Vec::new(),
              gpu_hot_url: None,
              model_filter: None,
              health_check_path: None,
              health_check_status: None,
              tags: Vec::new(),
          }
      }
  }
  ```

  **3c. Add `ModelWarmerConfig` to `Config`:**

  ```rust
  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct ModelWarmerConfig {
      #[serde(default = "default_warmer_interval")]
      pub interval_secs: u64,
  }

  impl Default for ModelWarmerConfig {
      fn default() -> Self {
          Self { interval_secs: default_warmer_interval() }
      }
  }

  fn default_warmer_interval() -> u64 {
      240
  }
  ```

  Add to `Config`:

  ```rust
  #[derive(Debug, Clone, Default, Serialize, Deserialize)]
  pub struct Config {
      #[serde(default)]
      pub server: ServerConfig,
      #[serde(default)]
      pub routing: RoutingConfig,
      #[serde(default)]
      pub backends: Vec<Backend>,
      #[serde(default)]
      pub circuit_breaker: CircuitBreakerConfig,
      #[serde(default)]
      pub observability: ObservabilityConfig,
      #[serde(default)]
      pub model_warmer: ModelWarmerConfig,
  }
  ```

- [ ] **Step 4: Fix compile errors from removed `default_model` and `idle_timeout_minutes`**

  ```bash
  cargo build 2>&1 | grep "error\[" | head -20
  ```

  Fix each location:

  **`src/api/admin.rs` — three places:**

  a. `AddBackendRequest`: remove `default_model` field and its `#[serde(default)]` line.

  b. `UpdateBackendRequest`: remove `default_model` field and its `#[serde(skip_serializing_if)]` line.
  Also remove the handler block that mutates it (in `update_backend`):
  ```rust
  // Remove this block:
  if let Some(default_model) = req.default_model {
      backend.config.default_model = Some(default_model);
  }
  ```

  c. `BackendResponse`: remove `default_model` field. In `backend_to_response()`, replace:
  ```rust
  // Remove:
  default_model: b.config.default_model.clone(),
  // Add:
  hot_models: b.config.hot_models.clone(),
  ```
  Add `hot_models: Vec<String>` to the `BackendResponse` struct.

  **`src/server.rs`:**

  Remove the `ModelHoming` import and spawn call (leave a TODO comment — Task 3 replaces it):
  ```rust
  // TODO Task 3: replace with ModelWarmer::spawn
  // let homing = ModelHoming::new(...);
  // homing.spawn(pool.clone()).await;
  ```

  Remove from `status_handler`:
  ```rust
  // Remove: "idle_timeout_minutes": config.routing.idle_timeout_minutes,
  ```

  Note: `Backend.default_model` had `#[serde(default)]` — its absence from the struct means old YAML with `default_model:` will parse fine because serde ignores unknown fields by default (no `#[serde(deny_unknown_fields)]` on `Backend`).

- [ ] **Step 5: Run all tests**

  ```bash
  cargo test 2>&1 | tail -20
  ```

  Expected: all existing tests pass + 5 new config tests pass.

- [ ] **Step 6: Commit**

  ```bash
  git add src/config.rs src/server.rs src/api/admin.rs
  git commit -m "feat: add default_keep_alive, hot_models, ModelWarmerConfig; remove default_model, idle_timeout_minutes"
  ```

---

## Chunk 2: keep_alive Proxy Injection

### Task 2: Inject `keep_alive` in `proxy_handler`

**Files:**
- Modify: `src/server.rs`

- [ ] **Step 1: Write the failing injection tests**

  Add to the `#[cfg(test)]` block in `src/server.rs` (near `reload_config_updates_live_state`):

  ```rust
  #[test]
  fn keep_alive_injected_into_api_generate() {
      let body = serde_json::json!({"model": "llama3", "prompt": "hi"});
      let bytes = serde_json::to_vec(&body).unwrap();
      let result = inject_keep_alive(&bytes, "/api/generate", "-1");
      let parsed: serde_json::Value = serde_json::from_slice(&result).unwrap();
      assert_eq!(parsed["keep_alive"], "-1");
      assert_eq!(parsed["model"], "llama3"); // other fields preserved
  }

  #[test]
  fn keep_alive_injected_into_api_chat() {
      let body = serde_json::json!({"model": "llama3", "messages": []});
      let bytes = serde_json::to_vec(&body).unwrap();
      let result = inject_keep_alive(&bytes, "/api/chat", "-1");
      let parsed: serde_json::Value = serde_json::from_slice(&result).unwrap();
      assert_eq!(parsed["keep_alive"], "-1");
  }

  #[test]
  fn keep_alive_not_injected_on_v1_path() {
      let body = serde_json::json!({"model": "llama3", "messages": []});
      let bytes = serde_json::to_vec(&body).unwrap();
      let result = inject_keep_alive(&bytes, "/v1/chat/completions", "-1");
      let parsed: serde_json::Value = serde_json::from_slice(&result).unwrap();
      assert!(!parsed.as_object().unwrap().contains_key("keep_alive"));
  }

  #[test]
  fn keep_alive_overwrites_existing_client_value() {
      // Primary use case: client (e.g. Open WebUI) sends keep_alive: "5m"; Herd overrides it
      let body = serde_json::json!({"model": "llama3", "prompt": "hi", "keep_alive": "5m"});
      let bytes = serde_json::to_vec(&body).unwrap();
      let result = inject_keep_alive(&bytes, "/api/generate", "-1");
      let parsed: serde_json::Value = serde_json::from_slice(&result).unwrap();
      assert_eq!(parsed["keep_alive"], "-1");
  }

  #[test]
  fn keep_alive_passthrough_on_invalid_json() {
      let bad = b"not json at all";
      let result = inject_keep_alive(bad, "/api/generate", "-1");
      assert_eq!(result.as_ref(), bad.as_ref()); // bytes unchanged
  }
  ```

- [ ] **Step 2: Run tests to confirm they fail**

  ```bash
  cargo test keep_alive -- --nocapture 2>&1
  ```

  Expected: compile error — `inject_keep_alive` not defined.

- [ ] **Step 3: Implement `inject_keep_alive` helper and wire it in**

  Add this free function above `proxy_handler` in `src/server.rs`:

  ```rust
  /// Injects `keep_alive` into an Ollama-native request body.
  /// Only applies to /api/generate and /api/chat; all other paths and
  /// invalid JSON bodies are returned unchanged.
  fn inject_keep_alive(body: &[u8], path: &str, keep_alive: &str) -> bytes::Bytes {
      let is_ollama_endpoint =
          path.contains("/api/generate") || path.contains("/api/chat");
      if !is_ollama_endpoint {
          return bytes::Bytes::copy_from_slice(body);
      }
      let Ok(mut payload) = serde_json::from_slice::<serde_json::Value>(body) else {
          return bytes::Bytes::copy_from_slice(body);
      };
      if let Some(obj) = payload.as_object_mut() {
          obj.insert("keep_alive".to_string(), serde_json::Value::String(keep_alive.to_string()));
      }
      match serde_json::to_vec(&payload) {
          Ok(modified) => bytes::Bytes::from(modified),
          Err(_) => bytes::Bytes::copy_from_slice(body),
      }
  }
  ```

  Then in `proxy_handler`, after the existing model extraction block (after line ~540), add:

  ```rust
  // Inject keep_alive for Ollama-native endpoints
  let keep_alive_value = state.config.read().await.routing.default_keep_alive.clone();
  let forward_bytes = inject_keep_alive(&body_bytes, &path, &keep_alive_value);
  ```

  In the retry loop, replace every `body_bytes.clone()` with `forward_bytes.clone()`:

  ```rust
  // was: .body(body_bytes.clone())
  .body(forward_bytes.clone())
  ```

- [ ] **Step 4: Run all tests**

  ```bash
  cargo test 2>&1 | tail -20
  ```

  Expected: all tests pass including 5 new injection tests.

- [ ] **Step 5: Commit**

  ```bash
  git add src/server.rs
  git commit -m "feat: inject keep_alive into Ollama proxied requests"
  ```

---

## Chunk 3: ModelWarmer + Wiring + Cleanup

### Task 3: Create `ModelWarmer`, wire it, delete `ModelHoming`

**Files:**
- Create: `src/backend/warmer.rs`
- Modify: `src/backend/mod.rs`
- Modify: `src/server.rs`
- Delete: `src/model_homing.rs`

- [ ] **Step 1: Write the failing warmer tests**

  At the bottom of the soon-to-be-created `src/backend/warmer.rs`, add:

  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;

      #[test]
      fn warm_url_constructed_correctly() {
          let url = warm_url("http://citadel:11434");
          assert_eq!(url, "http://citadel:11434/api/generate");
      }

      #[test]
      fn warm_payload_contains_keep_alive() {
          let payload = warm_payload("llama3:8b");
          assert_eq!(payload["model"], "llama3:8b");
          assert_eq!(payload["keep_alive"], "-1");
          assert_eq!(payload["prompt"], "");
      }
  }
  ```

  These test two pure helper functions we'll extract to make the logic easily testable without needing a live HTTP client.

- [ ] **Step 2: Run tests to confirm they fail**

  ```bash
  cargo test warmer -- --nocapture 2>&1
  ```

  Expected: compile error — `warmer` module not found.

- [ ] **Step 3: Create `src/backend/warmer.rs`**

  ```rust
  use crate::backend::BackendPool;
  use std::time::Duration;
  use tokio::time::interval;

  pub struct ModelWarmer {
      interval: Duration,
      client: reqwest::Client,
  }

  impl ModelWarmer {
      pub fn new(interval_secs: u64) -> Self {
          Self {
              interval: Duration::from_secs(interval_secs),
              client: reqwest::Client::builder()
                  .timeout(Duration::from_secs(30))
                  .build()
                  .unwrap(),
          }
      }

      pub async fn spawn(self, pool: BackendPool) {
          tokio::spawn(async move {
              let mut ticker = interval(self.interval);
              loop {
                  ticker.tick().await;
                  self.warm_all(&pool).await;
              }
          });
      }

      async fn warm_all(&self, pool: &BackendPool) {
          let backends = pool.all().await;
          for name in backends {
              if let Some(state) = pool.get(&name).await {
                  for model in &state.config.hot_models {
                      let url = warm_url(&state.config.url);
                      let payload = warm_payload(model);
                      let client = self.client.clone();
                      let model = model.clone();
                      let name = name.clone();
                      tokio::spawn(async move {
                          if let Err(e) = client.post(&url).json(&payload).send().await {
                              tracing::warn!("Warmer failed for {} on {}: {}", model, name, e);
                          } else {
                              tracing::debug!("Warmed {} on {}", model, name);
                          }
                      });
                  }
              }
          }
      }
  }

  pub fn warm_url(base_url: &str) -> String {
      format!("{}/api/generate", base_url.trim_end_matches('/'))
  }

  pub fn warm_payload(model: &str) -> serde_json::Value {
      serde_json::json!({
          "model": model,
          "prompt": "",
          "keep_alive": "-1"
      })
  }

  #[cfg(test)]
  mod tests {
      use super::*;

      #[test]
      fn warm_url_constructed_correctly() {
          let url = warm_url("http://citadel:11434");
          assert_eq!(url, "http://citadel:11434/api/generate");
      }

      #[test]
      fn warm_payload_contains_keep_alive() {
          let payload = warm_payload("llama3:8b");
          assert_eq!(payload["model"], "llama3:8b");
          assert_eq!(payload["keep_alive"], "-1");
          assert_eq!(payload["prompt"], "");
      }
  }
  ```

- [ ] **Step 4: Export from `src/backend/mod.rs`**

  Add to `src/backend/mod.rs`:

  ```rust
  pub mod warmer;
  pub use warmer::ModelWarmer;
  ```

- [ ] **Step 5: Run warmer tests**

  ```bash
  cargo test warmer -- --nocapture 2>&1
  ```

  Expected: 2 warmer tests pass.

- [ ] **Step 6: Wire `ModelWarmer` into `server.rs`**

  In `src/server.rs`:

  **6a.** Change the import:
  ```rust
  // Remove:
  // use crate::model_homing::ModelHoming;
  // Add:
  use crate::backend::{BackendPool, HealthChecker, ModelDiscovery, ModelWarmer};
  ```

  **6b.** Replace the commented-out homing block:
  ```rust
  // Remove TODO comment and add:
  let warmer = ModelWarmer::new(self.config.model_warmer.interval_secs);
  warmer.spawn(pool.clone()).await;
  ```

- [ ] **Step 7: Delete `src/model_homing.rs`**

  ```bash
  rm src/model_homing.rs
  ```

- [ ] **Step 8: Build and run all tests**

  ```bash
  cargo build 2>&1 | grep "^error" | head -20
  cargo test 2>&1 | tail -25
  ```

  Expected: clean build, all tests pass (40+ tests).

- [ ] **Step 9: Commit**

  ```bash
  git add src/backend/warmer.rs src/backend/mod.rs src/server.rs
  git rm src/model_homing.rs
  git commit -m "feat: add ModelWarmer (hot_models); remove ModelHoming"
  ```

---

## Final: Tag and push

- [ ] **Update version in `Cargo.toml` to `0.4.3`**

  ```bash
  # Edit Cargo.toml: version = "0.4.2" → "0.4.3"
  cargo build  # verify
  ```

- [ ] **Update `tasks/todo.md`** — mark all v0.4.3 items as done

- [ ] **Commit and tag**

  ```bash
  git add Cargo.toml tasks/todo.md
  git commit -m "chore: bump version to 0.4.3"
  git tag v0.4.3
  git push && git push --tags
  ```
