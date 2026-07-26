use crate::backend::{BackendPool, GpuMetrics};
use crate::config::Backend;
use anyhow::Result;
use serde::Deserialize;
use std::collections::BTreeSet;
use std::sync::Mutex;
use std::time::Duration;
use tokio::time::interval;
use tracing::info;

#[derive(Debug, Deserialize)]
struct OllamaModels {
    models: Vec<OllamaModel>,
}

#[derive(Debug, Deserialize)]
struct OllamaModel {
    name: String,
}

#[derive(Debug, Deserialize)]
struct OllamaRunning {
    models: Vec<OllamaRunningModel>,
}

#[derive(Debug, Deserialize)]
struct OllamaRunningModel {
    name: String,
    #[serde(default)]
    model: String,
}

/// OpenAI-compatible /v1/models response (used by llama-server)
#[derive(Debug, Deserialize)]
struct OpenAIModelsResponse {
    #[serde(default)]
    data: Vec<OpenAIModel>,
}

#[derive(Debug, Deserialize)]
struct OpenAIModel {
    id: String,
}

#[derive(Debug, Deserialize)]
struct GpuHotData {
    gpus: Vec<GpuInfo>,
}

#[derive(Debug, Deserialize)]
struct GpuInfo {
    #[serde(rename = "index")]
    _index: u32,
    #[allow(dead_code)]
    name: String,
    utilization: f32,
    memory_used: u64,
    memory_total: u64,
    temperature: f32,
}

/// Apply the D2 model-filter order to a backend's raw model list:
/// `raw → models_enabled (exact-name retain, when Some) → model_filter regex
/// (only when models_enabled is None)`.
///
/// - `models_enabled = Some(list)` → keep only exact-name matches; `Some(&[])`
///   yields an empty list ("expose nothing").
/// - `models_enabled = None` → current behavior: apply the `model_filter` regex
///   if set (an invalid regex warns and is skipped — never a hard error).
pub(crate) fn filter_models(
    mut names: Vec<String>,
    models_enabled: Option<&[String]>,
    model_filter: Option<&str>,
    backend_name: &str,
) -> Vec<String> {
    if let Some(allow) = models_enabled {
        let before = names.len();
        let set: std::collections::HashSet<&str> = allow.iter().map(|s| s.as_str()).collect();
        names.retain(|n| set.contains(n.as_str()));
        tracing::debug!(
            "models_enabled on {}: kept {}/{} models",
            backend_name,
            names.len(),
            before
        );
        return names;
    }

    if let Some(filter) = model_filter {
        match regex::Regex::new(filter) {
            Ok(re) => {
                let before = names.len();
                names.retain(|name| re.is_match(name));
                if names.len() < before {
                    tracing::debug!(
                        "model_filter '{}' on {}: kept {}/{} models",
                        filter,
                        backend_name,
                        names.len(),
                        before
                    );
                }
            }
            Err(e) => {
                tracing::warn!(
                    "Invalid model_filter '{}' on {}: {}",
                    filter,
                    backend_name,
                    e
                );
            }
        }
    }

    names
}

pub struct ModelDiscovery {
    client: reqwest::Client,
    interval: Duration,
    /// Backend names whose resident-model probe (`/api/ps` / `/v1/models`) is
    /// CURRENTLY in a failed state. Bookkeeping only — never read by routing —
    /// so `discover_running`'s stale-keep WARN logs once per backend per
    /// healthy→failed transition instead of once per discovery tick.
    resident_probe_failed: Mutex<BTreeSet<String>>,
}

impl ModelDiscovery {
    pub fn new(interval_secs: u64) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap(),
            interval: Duration::from_secs(interval_secs),
            resident_probe_failed: Mutex::new(BTreeSet::new()),
        }
    }

    /// Records the outcome of a resident-model probe for `name`. Returns
    /// `true` exactly on a healthy→failed transition (caller should log a
    /// WARN); returns `false` on a repeat failure (already logged) or any
    /// success. Never panics on a poisoned lock — recovers the inner set,
    /// since a stale bookkeeping set is a logging-cadence nuisance, not a
    /// correctness issue.
    fn note_probe_result(&self, name: &str, ok: bool) -> bool {
        let mut failed = self
            .resident_probe_failed
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if ok {
            failed.remove(name);
            false
        } else {
            failed.insert(name.to_string())
        }
    }

    pub async fn spawn(self, pool: BackendPool) {
        tokio::spawn(async move {
            let mut ticker = interval(self.interval);
            loop {
                ticker.tick().await;
                self.discover_all(&pool).await;
            }
        });
    }

    async fn discover_all(&self, pool: &BackendPool) {
        let backends = pool.all().await;
        for name in backends {
            if let Some(state) = pool.get(&name).await {
                // Discover available models
                if let Err(e) = self.discover_models(pool, &state.config).await {
                    tracing::warn!("Failed to discover models for {}: {}", name, e);
                }

                // Discover currently loaded model
                if let Err(e) = self.discover_running(pool, &state.config).await {
                    tracing::trace!("No running model on {}: {}", name, e);
                }

                // Discover GPU metrics via explicit gpu_hot_url, or auto-derive from backend host on port 1312
                let gpu_url = if let Some(ref configured) = state.config.gpu_hot_url {
                    Some(configured.clone())
                } else {
                    let host = state
                        .config
                        .url
                        .trim_start_matches("http://")
                        .trim_start_matches("https://")
                        .split(':')
                        .next()
                        .unwrap_or("");
                    if !host.is_empty() {
                        Some(format!("http://{}:1312", host))
                    } else {
                        None
                    }
                };
                if let Some(ref gpu_url) = gpu_url {
                    if let Err(e) = self.discover_gpu_metrics(pool, &name, gpu_url).await {
                        tracing::trace!("No gpu-hot on {}: {}", name, e);
                    }
                }

                // Populate total VRAM once from passive GPU telemetry when available.
                if !state.vram_populated {
                    if let Some(updated) = pool.get(&name).await {
                        if let Some(gpu) = updated.gpu_metrics {
                            if gpu.memory_total > 0 {
                                info!("Backend {} VRAM: {} MB", name, gpu.memory_total);
                                pool.set_vram(&name, gpu.memory_total).await;
                            }
                        }
                    }
                }
            }
        }
        info!("Model discovery complete");
    }

    /// Re-probe a single backend and update the pool's model list (applies the
    /// D2 filter). Public so the config API can nudge a refresh after a PUT.
    pub async fn discover_models(&self, pool: &BackendPool, backend: &Backend) -> Result<()> {
        let model_names: Vec<String> = match backend.backend {
            crate::config::BackendType::LlamaServer | crate::config::BackendType::OpenAICompat => {
                let url = format!("{}/v1/models", backend.url);
                let resp = self.client.get(&url).send().await?;
                let models: OpenAIModelsResponse = resp.json().await?;
                models.data.into_iter().map(|m| m.id).collect()
            }
            crate::config::BackendType::Ollama => {
                let url = format!("{}/api/tags", backend.url);
                let resp = self.client.get(&url).send().await?;
                let models: OllamaModels = resp.json().await?;
                models.models.into_iter().map(|m| m.name).collect()
            }
        };

        // Apply the D2 filter order: models_enabled allowlist wins over model_filter.
        let model_names = filter_models(
            model_names,
            backend.models_enabled.as_deref(),
            backend.model_filter.as_deref(),
            &backend.name,
        );

        pool.update_models(&backend.name, model_names).await;

        Ok(())
    }

    /// Re-probe a single backend's LIVE resident-model set (models actually
    /// loaded and serving right now, not merely on disk) and write the FULL
    /// list into the pool via `update_resident_models` — Ollama can hold
    /// several models concurrently, so this reads every `/api/ps` entry
    /// rather than truncating to `.first()`. Public so tests and the config
    /// API can drive a single pass deterministically.
    ///
    /// Failure policy (residency-signal.md §7): a failed or malformed probe —
    /// connection refused, a non-2xx status (including a pre-`/api/ps`
    /// Ollama's 404), or unparseable JSON — leaves `resident_models`
    /// UNCHANGED. It is never cleared and never falls back to `models`; doing
    /// either would misreport a transient blip or an old Ollama as "nothing
    /// resident" or "everything on disk is resident". A WARN is logged once
    /// per backend per healthy→failed transition (not once per tick); the
    /// error is still returned so `discover_all`'s existing trace-level log
    /// and the health checker's independent probe cycle are unaffected.
    pub async fn discover_running(&self, pool: &BackendPool, backend: &Backend) -> Result<()> {
        let probe: Result<Vec<String>> = async {
            match backend.backend {
                crate::config::BackendType::LlamaServer
                | crate::config::BackendType::OpenAICompat => {
                    // llama-server/OpenAI-compat only ever serves what it
                    // loaded, so /v1/models IS the resident set.
                    let url = format!("{}/v1/models", backend.url);
                    let resp = self.client.get(&url).send().await?.error_for_status()?;
                    let models: OpenAIModelsResponse = resp.json().await?;
                    Ok(models.data.into_iter().map(|m| m.id).collect())
                }
                crate::config::BackendType::Ollama => {
                    let url = format!("{}/api/ps", backend.url);
                    let resp = self.client.get(&url).send().await?.error_for_status()?;
                    let running: OllamaRunning = resp.json().await?;
                    Ok(running
                        .models
                        .into_iter()
                        .map(|m| if m.model.is_empty() { m.name } else { m.model })
                        .collect())
                }
            }
        }
        .await;

        match probe {
            Ok(models) => {
                pool.update_resident_models(&backend.name, models).await;
                self.note_probe_result(&backend.name, true);
                Ok(())
            }
            Err(e) => {
                if self.note_probe_result(&backend.name, false) {
                    tracing::warn!(
                        "Resident-model probe failed for {}: {} — keeping previous resident_models (stale-keep)",
                        backend.name,
                        e
                    );
                }
                Err(e)
            }
        }
    }

    async fn discover_gpu_metrics(&self, pool: &BackendPool, name: &str, url: &str) -> Result<()> {
        let url = format!("{}/api/gpu-data", url);
        let resp = self.client.get(&url).send().await?;
        let data: GpuHotData = resp.json().await?;

        // Use first GPU for now (could aggregate multi-GPU later)
        if let Some(gpu) = data.gpus.first() {
            let metrics = GpuMetrics {
                utilization: gpu.utilization,
                memory_used: gpu.memory_used,
                memory_total: gpu.memory_total,
                temperature: gpu.temperature,
            };
            pool.update_gpu_metrics(name, metrics).await;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    // ── D2 filter_models: models_enabled allowlist vs model_filter regex ─────

    #[test]
    fn models_enabled_takes_precedence_over_filter() {
        // Allowlist wins; the regex (which would keep only b:2) is ignored.
        let out = filter_models(
            v(&["a:1", "b:2", "c:3"]),
            Some(&v(&["a:1", "c:3"])),
            Some("^b"),
            "be",
        );
        assert_eq!(out, v(&["a:1", "c:3"]));
    }

    #[test]
    fn some_empty_enabled_exposes_nothing() {
        let out = filter_models(v(&["a", "b"]), Some(&[]), None, "be");
        assert!(out.is_empty(), "Some(vec![]) => expose nothing");
    }

    #[test]
    fn none_enabled_applies_filter_current_behavior() {
        let out = filter_models(
            v(&["keep-1", "drop-2", "keep-3"]),
            None,
            Some("^keep"),
            "be",
        );
        assert_eq!(out, v(&["keep-1", "keep-3"]));
    }

    #[test]
    fn none_enabled_no_filter_is_identity() {
        let raw = v(&["x", "y", "z"]);
        assert_eq!(filter_models(raw.clone(), None, None, "be"), raw);
    }

    #[test]
    fn enabled_is_exact_name_not_substring() {
        let out = filter_models(
            v(&["llama3:8b", "llama3:70b"]),
            Some(&v(&["llama3:8b"])),
            None,
            "be",
        );
        assert_eq!(out, v(&["llama3:8b"]));
    }

    #[test]
    fn invalid_filter_regex_is_passthrough_not_error() {
        let raw = v(&["a", "b"]);
        // Invalid regex warns and keeps everything — never a hard error.
        assert_eq!(
            filter_models(raw.clone(), None, Some("(unclosed"), "be"),
            raw
        );
    }

    #[test]
    fn parse_openai_models_response() {
        let json = r#"{"object":"list","data":[
            {"id":"gemma-4-26B","object":"model","owned_by":"llamacpp","created":1234}
        ]}"#;
        let resp: OpenAIModelsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.data.len(), 1);
        assert_eq!(resp.data[0].id, "gemma-4-26B");
    }
}
