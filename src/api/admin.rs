use crate::config::{Backend, BackendType};
use crate::server::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct AddBackendRequest {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub backend: BackendType,
    #[serde(default = "default_priority")]
    pub priority: u32,
    #[serde(default)]
    pub model_filter: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

fn default_priority() -> u32 {
    50
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateBackendRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_filter: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    /// Manual VRAM override in MB. Overrides auto-detected value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vram_override_mb: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct BackendResponse {
    pub name: String,
    pub url: String,
    pub backend: String,
    pub priority: u32,
    pub hot_models: Vec<String>,
    pub model_filter: Option<String>,
    pub tags: Vec<String>,
    pub healthy: bool,
    pub current_model: Option<String>,
    pub model_count: usize,
    pub idle_seconds: u64,
    pub gpu: Option<GpuResponse>,
    pub vram_total_mb: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct GpuResponse {
    pub utilization: f32,
    pub memory_used: u64,
    pub memory_total: u64,
    pub temperature: f32,
}

fn backend_to_response(b: &crate::backend::BackendState) -> BackendResponse {
    BackendResponse {
        name: b.config.name.clone(),
        url: b.config.url.clone(),
        backend: b.config.backend.to_string(),
        priority: b.config.priority,
        hot_models: b.config.hot_models.clone(),
        model_filter: b.config.model_filter.clone(),
        tags: b.config.tags.clone(),
        healthy: b.healthy,
        current_model: b.current_model.clone(),
        model_count: b.models.len(),
        idle_seconds: b.last_request.elapsed().as_secs(),
        gpu: b.gpu_metrics.as_ref().map(|g| GpuResponse {
            utilization: g.utilization,
            memory_used: g.memory_used,
            memory_total: g.memory_total,
            temperature: g.temperature,
        }),
        vram_total_mb: b.vram_total_mb,
    }
}

pub async fn list_backends(State(state): State<AppState>) -> Json<Vec<BackendResponse>> {
    let backends = state.pool.all().await;
    let mut response = Vec::new();

    for name in backends {
        if let Some(b) = state.pool.get(&name).await {
            response.push(backend_to_response(&b));
        }
    }

    Json(response)
}

pub async fn get_backend(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<BackendResponse>, StatusCode> {
    let backend = state.pool.get(&name).await.ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(backend_to_response(&backend)))
}

pub async fn add_backend(
    State(state): State<AppState>,
    Json(req): Json<AddBackendRequest>,
) -> Result<Json<BackendResponse>, (StatusCode, String)> {
    // Check if already exists
    if state.pool.get(&req.name).await.is_some() {
        return Err((
            StatusCode::CONFLICT,
            format!("Backend '{}' already exists", req.name),
        ));
    }

    let backend = Backend {
        name: req.name.clone(),
        url: req.url,
        backend: req.backend,
        priority: req.priority,
        hot_models: Vec::new(),
        gpu_hot_url: None,
        model_filter: req.model_filter,
        health_check_path: None,
        health_check_status: None,
        tags: req.tags,
        max_context_len: None,
        locality: None,
        power_cost: None,
        models_enabled: None,
        enabled: true,
    };

    state.pool.add(backend).await;

    // Brief pause for health check to pick it up
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    let b = state.pool.get(&req.name).await.ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to add backend".to_string(),
        )
    })?;

    tracing::info!("Added backend: {}", req.name);
    Ok(Json(backend_to_response(&b)))
}

pub async fn update_backend(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(req): Json<UpdateBackendRequest>,
) -> Result<Json<BackendResponse>, (StatusCode, String)> {
    let mut backend = state.pool.get(&name).await.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            format!("Backend '{}' not found", name),
        )
    })?;

    if let Some(url) = req.url {
        backend.config.url = url;
    }
    if let Some(priority) = req.priority {
        backend.config.priority = priority;
    }
    if let Some(model_filter) = req.model_filter {
        backend.config.model_filter = Some(model_filter);
    }
    if let Some(tags) = req.tags {
        backend.config.tags = tags;
    }

    state.pool.update(backend.clone()).await;

    if let Some(vram_mb) = req.vram_override_mb {
        state.pool.set_vram(&name, vram_mb).await;
        // Re-fetch so the response reflects the override
        backend = state.pool.get(&name).await.ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                format!("Backend '{}' not found", name),
            )
        })?;
    }

    tracing::info!("Updated backend: {}", name);
    Ok(Json(backend_to_response(&backend)))
}

pub async fn remove_backend(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<StatusCode, StatusCode> {
    if state.pool.remove(&name).await {
        tracing::info!("Removed backend: {}", name);
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

// ---------------------------------------------------------------------------
// Model management endpoints (proxy to Ollama API on the backend)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct PullModelRequest {
    pub name: String,
}

/// POST /admin/backends/:name/pull — Pull a model on a specific backend.
/// Streams Ollama pull progress as SSE.
pub async fn pull_model(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(req): Json<PullModelRequest>,
) -> Result<axum::response::Response, (StatusCode, String)> {
    let backend = state.pool.get(&name).await.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            format!("Backend '{}' not found", name),
        )
    })?;

    let url = format!("{}/api/pull", backend.config.url.trim_end_matches('/'));
    tracing::info!("Pulling model '{}' on backend '{}'", req.name, name);

    // Stream the pull response from Ollama (uses mgmt_client with 1-hour timeout)
    let resp = state
        .mgmt_client
        .post(&url)
        .json(&serde_json::json!({"name": req.name, "stream": true}))
        .send()
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                format!("Failed to reach backend '{}': {}", name, e),
            )
        })?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err((
            StatusCode::BAD_GATEWAY,
            format!("Ollama pull failed ({}): {}", status, body),
        ));
    }

    // Stream Ollama's NDJSON progress through as SSE
    let stream = resp.bytes_stream();
    let body = axum::body::Body::from_stream(stream);

    axum::response::Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/x-ndjson")
        .header("cache-control", "no-cache")
        .body(body)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to build response: {}", e),
            )
        })
}

/// DELETE /admin/backends/:name/models/:model — Delete a model from a specific backend.
pub async fn delete_model(
    State(state): State<AppState>,
    Path((name, model)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let backend = state.pool.get(&name).await.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            format!("Backend '{}' not found", name),
        )
    })?;

    let url = format!("{}/api/delete", backend.config.url.trim_end_matches('/'));
    tracing::info!("Deleting model '{}' from backend '{}'", model, name);

    let resp = state
        .client
        .delete(&url)
        .json(&serde_json::json!({"name": model}))
        .send()
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                format!("Failed to reach backend '{}': {}", name, e),
            )
        })?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err((
            StatusCode::BAD_GATEWAY,
            format!("Ollama delete failed ({}): {}", status, body),
        ));
    }

    tracing::info!("Deleted model '{}' from backend '{}'", model, name);
    Ok(Json(serde_json::json!({
        "status": "deleted",
        "model": model,
        "backend": name,
    })))
}

/// GET /admin/backends/:name/models — List all models on a specific backend.
pub async fn list_backend_models(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let backend = state.pool.get(&name).await.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            format!("Backend '{}' not found", name),
        )
    })?;

    // Fetch fresh model list from Ollama
    let url = format!("{}/api/tags", backend.config.url.trim_end_matches('/'));
    let resp = state
        .client
        .get(&url)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                format!("Failed to reach backend '{}': {}", name, e),
            )
        })?;

    let data: serde_json::Value = resp.json().await.map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            format!("Invalid response from '{}': {}", name, e),
        )
    })?;

    Ok(Json(serde_json::json!({
        "backend": name,
        "vram_total_mb": backend.vram_total_mb,
        "models": data.get("models").cloned().unwrap_or(serde_json::json!([])),
    })))
}

// ---------------------------------------------------------------------------
// Config editor endpoints
// ---------------------------------------------------------------------------

/// GET /admin/config — Return current config as JSON (api_key redacted).
pub async fn get_config(State(state): State<AppState>) -> Json<serde_json::Value> {
    let config = state.config_snapshot().await;
    let mut json = serde_json::to_value(&config).unwrap_or_default();
    // Redact secrets — show presence but not value
    if let Some(server) = json.get_mut("server") {
        for field in &["api_key", "enrollment_key"] {
            if let Some(key) = server.get(*field) {
                if key.is_string() {
                    server[*field] = serde_json::json!("********");
                }
            }
        }
    }
    Json(json)
}

/// PUT /admin/config — Validate, write to disk, and hot-reload.
pub async fn update_config(
    State(state): State<AppState>,
    Json(mut new_config): Json<crate::config::Config>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    // If secrets are the redacted sentinel, preserve existing values
    let current = state.config_snapshot().await;
    if new_config.server.api_key.as_deref() == Some("********") {
        new_config.server.api_key = current.server.api_key.clone();
    }
    if new_config.server.enrollment_key.as_deref() == Some("********") {
        new_config.server.enrollment_key = current.server.enrollment_key.clone();
    }
    drop(current);

    // Validate before writing
    new_config.validate().map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e.to_string()})),
        )
    })?;

    // Write to disk (atomic: temp file + rename)
    let path = state.config_path.as_ref().ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "No config file path — server was started without a config file"})),
        )
    })?;

    let yaml = new_config.to_yaml().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Failed to serialize config: {}", e)})),
        )
    })?;

    let temp_path = path.with_extension("yaml.tmp");
    tokio::fs::write(&temp_path, &yaml).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Failed to write config: {}", e)})),
        )
    })?;
    tokio::fs::rename(&temp_path, path).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Failed to save config: {}", e)})),
        )
    })?;

    // Trigger hot-reload
    let msg = state.reload_config().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Config saved but reload failed: {}", e)})),
        )
    })?;

    tracing::info!("Config updated via dashboard: {}", msg);
    Ok(Json(serde_json::json!({"status": "ok", "message": msg})))
}

// ── GUI config overlay endpoints (gui-tray-spec #G2) ─────────────────────────

/// Effective (merged) view of a backend plus its live pool state.
#[derive(Debug, Serialize)]
pub struct EffectiveBackend {
    pub name: String,
    pub url: String,
    pub backend: String,
    pub priority: u32,
    pub enabled: bool,
    pub hot_models: Vec<String>,
    pub model_filter: Option<String>,
    pub tags: Vec<String>,
    /// GUI allowlist: `None` = all installed, `Some([])` = none.
    pub models_enabled: Option<Vec<String>>,
    /// Models the pool currently reports for this backend (post-filter).
    pub models_available: Vec<String>,
    pub healthy: bool,
}

/// True iff the effective config currently contains a backend named `name`.
async fn backend_exists(state: &AppState, name: &str) -> bool {
    state
        .config
        .read()
        .await
        .backends
        .iter()
        .any(|b| b.name == name)
}

/// Persist one override, JSON-encoding `value`. Never bails — encode/DB errors warn.
fn persist_override<T: Serialize>(state: &AppState, scope: &str, key: &str, value: &T) {
    match serde_json::to_string(value) {
        Ok(json) => {
            if let Err(e) = state.node_db.set_override(scope, key, &json) {
                tracing::warn!("config overlay: failed to persist {}/{}: {}", scope, key, e);
            }
        }
        Err(e) => tracing::warn!("config overlay: failed to encode {}/{}: {}", scope, key, e),
    }
}

/// Recompute the effective config from the YAML base + current overrides, push it
/// to the in-memory config and the live pool, and nudge discovery so the routable
/// model list updates within a tick — no restart. Correctly restores YAML values
/// when an override is cleared. No config file (CLI-args start) → the override is
/// still persisted; it takes effect on next start. Never bails.
async fn remerge_and_apply(state: &AppState) {
    let Some(path) = state.config_path.clone() else {
        tracing::warn!(
            "config overlay: override saved but no config file to re-merge live — restart to apply"
        );
        return;
    };
    let mut cfg = match crate::config::Config::from_file(&path) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("config overlay: re-merge failed to read config: {}", e);
            return;
        }
    };
    let rows: Vec<(String, String, String)> = match state.node_db.list_overrides() {
        Ok(l) => l
            .into_iter()
            .map(|o| (o.scope, o.key, o.value_json))
            .collect(),
        Err(e) => {
            tracing::warn!("config overlay: re-merge failed to list overrides: {}", e);
            return;
        }
    };
    cfg.apply_overrides(&rows);

    // Reconcile the pool: update existing backends' live config, add new ones.
    for b in &cfg.backends {
        if state.pool.get(&b.name).await.is_some() {
            state.pool.update_backend_config(&b.name, b.clone()).await;
        } else {
            state.pool.add(b.clone()).await;
        }
    }

    // Best-effort immediate re-discovery so model lists reflect new filters now
    // (the scheduled discovery tick would also catch up).
    let pool = (*state.pool).clone();
    let backends = cfg.backends.clone();
    tokio::spawn(async move {
        let disc = crate::backend::ModelDiscovery::new(0);
        for b in &backends {
            if let Err(e) = disc.discover_models(&pool, b).await {
                tracing::debug!(
                    "config overlay: discovery nudge for {} failed: {}",
                    b.name,
                    e
                );
            }
        }
    });

    *state.config.write().await = cfg;
}

/// GET /admin/config/backends — effective merged configs + live `models_available`.
pub async fn get_effective_backends(State(state): State<AppState>) -> Json<serde_json::Value> {
    let config = state.config.read().await;
    let mut backends = Vec::with_capacity(config.backends.len());
    for b in &config.backends {
        let (models_available, healthy) = match state.pool.get(&b.name).await {
            Some(st) => (st.models, st.healthy),
            None => (Vec::new(), false),
        };
        backends.push(EffectiveBackend {
            name: b.name.clone(),
            url: b.url.clone(),
            backend: b.backend.to_string(),
            priority: b.priority,
            enabled: b.enabled,
            hot_models: b.hot_models.clone(),
            model_filter: b.model_filter.clone(),
            tags: b.tags.clone(),
            models_enabled: b.models_enabled.clone(),
            models_available,
            healthy,
        });
    }
    Json(serde_json::json!({ "backends": backends }))
}

#[derive(Debug, Deserialize)]
pub struct SetModelsRequest {
    /// An array sets the allowlist; JSON `null` (or absent) clears the override.
    #[serde(default)]
    pub models_enabled: Option<Vec<String>>,
}

/// PUT /admin/config/backends/:name/models — set or clear the models_enabled allowlist.
pub async fn put_backend_models(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(req): Json<SetModelsRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !backend_exists(&state, &name).await {
        return Err(StatusCode::NOT_FOUND);
    }
    let scope = format!("backend:{name}");
    match &req.models_enabled {
        Some(list) => persist_override(&state, &scope, "models_enabled", list),
        None => {
            // JSON null / absent → delete the override (restore YAML behavior).
            if let Err(e) = state.node_db.delete_override(&scope, "models_enabled") {
                tracing::warn!(
                    "config overlay: failed to clear models_enabled for {}: {}",
                    name,
                    e
                );
            }
        }
    }
    remerge_and_apply(&state).await;
    Ok(Json(serde_json::json!({ "ok": true, "backend": name })))
}

#[derive(Debug, Deserialize)]
pub struct PatchBackendRequest {
    #[serde(default)]
    pub priority: Option<u32>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub hot_models: Option<Vec<String>>,
}

/// PUT /admin/config/backends/:name — patch priority / enabled / hot_models.
pub async fn patch_backend(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(req): Json<PatchBackendRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !backend_exists(&state, &name).await {
        return Err(StatusCode::NOT_FOUND);
    }
    let scope = format!("backend:{name}");
    if let Some(p) = req.priority {
        persist_override(&state, &scope, "priority", &p);
    }
    if let Some(e) = req.enabled {
        persist_override(&state, &scope, "enabled", &e);
    }
    if let Some(ref h) = req.hot_models {
        persist_override(&state, &scope, "hot_models", h);
    }
    remerge_and_apply(&state).await;
    Ok(Json(serde_json::json!({ "ok": true, "backend": name })))
}

#[derive(Debug, Deserialize)]
pub struct CreateOverlayBackendRequest {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub backend: BackendType,
    #[serde(default = "default_priority")]
    pub priority: u32,
}

/// POST /admin/config/backends — create an overlay-defined backend (detect flow).
pub async fn create_overlay_backend(
    State(state): State<AppState>,
    Json(req): Json<CreateOverlayBackendRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if backend_exists(&state, &req.name).await {
        return Err(StatusCode::CONFLICT);
    }
    let backend = Backend {
        name: req.name.clone(),
        url: req.url,
        backend: req.backend,
        priority: req.priority,
        ..Default::default()
    };
    persist_override(
        &state,
        &format!("backend:{}", req.name),
        "definition",
        &backend,
    );
    remerge_and_apply(&state).await;
    Ok(Json(serde_json::json!({ "ok": true, "backend": req.name })))
}

/// GET /admin/config/overrides — dump the raw overlay (D3 inspectability).
pub async fn get_overrides(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    match state.node_db.list_overrides() {
        Ok(list) => Ok(Json(serde_json::json!({ "overrides": list }))),
        Err(e) => {
            tracing::warn!("config overlay: failed to list overrides: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// DELETE /admin/config/overrides/:scope/:key — remove one override, restoring YAML.
pub async fn delete_override_handler(
    State(state): State<AppState>,
    Path((scope, key)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    match state.node_db.delete_override(&scope, &key) {
        Ok(true) => {
            remerge_and_apply(&state).await;
            Ok(Json(
                serde_json::json!({ "ok": true, "removed": format!("{scope}/{key}") }),
            ))
        }
        Ok(false) => Err(StatusCode::NOT_FOUND),
        Err(e) => {
            tracing::warn!("config overlay: failed to delete {}/{}: {}", scope, key, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}
