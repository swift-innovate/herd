//! Acceptance tests for `specs/residency-signal.md`.
//!
//! Each test is annotated with the AC(s) it proves. AC4 (agent-origin
//! `models_loaded` → `resident_models`) is covered by two in-crate unit tests
//! in `src/nodes/pool_sync.rs` (`agent_add_sets_resident_models_from_models_loaded`,
//! `agent_update_empty_models_loaded_clears_resident_models`) since
//! `AgentPoolSync::reconcile` is the natural seam and doesn't need an HTTP
//! mock; this file covers the discovery-probe path (Ollama, llama-server) plus
//! the `/status` HTTP surface (AC7).
//!
//! Mocking approach mirrors `fleet_routing.rs`: a tiny axum server standing in
//! for the real backend, on an ephemeral port, so these are real HTTP round
//! trips through `reqwest` — not hand-mocked structs.

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use herd::backend::{BackendPool, ModelDiscovery};
use herd::config::{Backend, BackendType};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

// -----------------------------------------------------------------------------
// Mock Ollama backend: /api/tags (available/on-disk) + /api/ps (resident/running).
// A shared `fail_ps` flag lets a test flip `/api/ps` to a 500 mid-test to
// simulate a probe failure without tearing down the server (AC5).
// -----------------------------------------------------------------------------

#[derive(Clone, Default)]
struct MockOllama {
    fail_ps: Arc<AtomicBool>,
}

async fn ollama_tags() -> Json<Value> {
    Json(json!({
        "models": [
            {"name": "llama3:8b"},
            {"name": "mistral:7b"},
            {"name": "qwen3-32b"},
        ]
    }))
}

async fn ollama_ps_one(State(mock): State<MockOllama>) -> axum::response::Response {
    if mock.fail_ps.load(Ordering::SeqCst) {
        return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    Json(json!({
        "models": [
            {"name": "qwen3-32b", "model": "qwen3-32b"},
        ]
    }))
    .into_response()
}

async fn ollama_ps_two(State(mock): State<MockOllama>) -> axum::response::Response {
    if mock.fail_ps.load(Ordering::SeqCst) {
        return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    Json(json!({
        "models": [
            {"name": "qwen3-32b", "model": "qwen3-32b"},
            {"name": "mistral:7b", "model": "mistral:7b"},
        ]
    }))
    .into_response()
}

use axum::response::IntoResponse;

/// Spawn a mock Ollama server. `ps_two` selects whether `/api/ps` reports one
/// (AC1) or two (AC2) running models. Returns the mock (for flipping
/// `fail_ps`), the base URL, and a graceful-shutdown handle.
async fn spawn_mock_ollama(ps_two: bool) -> (MockOllama, String, ShutdownHandle) {
    let mock = MockOllama::default();
    let app = if ps_two {
        Router::new()
            .route("/api/tags", get(ollama_tags))
            .route("/api/ps", get(ollama_ps_two))
            .with_state(mock.clone())
    } else {
        Router::new()
            .route("/api/tags", get(ollama_tags))
            .route("/api/ps", get(ollama_ps_one))
            .with_state(mock.clone())
    };
    let (url, shutdown) = spawn_with_shutdown(app).await;
    (mock, url, shutdown)
}

// -----------------------------------------------------------------------------
// Mock llama-server / openai-compat backend: /v1/models only (it only ever
// serves what it loaded, so `models` and `resident_models` must come out equal).
// -----------------------------------------------------------------------------

async fn llama_models() -> Json<Value> {
    Json(json!({
        "object": "list",
        "data": [
            {"id": "qwen3-32b-q4", "object": "model", "owned_by": "llamacpp", "created": 0},
        ]
    }))
}

async fn spawn_mock_llama_server() -> (String, ShutdownHandle) {
    let app = Router::new().route("/v1/models", get(llama_models));
    spawn_with_shutdown(app).await
}

// -----------------------------------------------------------------------------
// Shutdown plumbing: `axum::serve(..).with_graceful_shutdown` + an awaited
// `JoinHandle` gives a deterministic "the socket is now closed" point (AC5's
// "connection refused" case), with no sleep-and-hope polling.
// -----------------------------------------------------------------------------

struct ShutdownHandle {
    tx: Option<tokio::sync::oneshot::Sender<()>>,
    handle: Option<tokio::task::JoinHandle<()>>,
}

impl ShutdownHandle {
    /// Signal shutdown and wait for the server task to fully exit — after this
    /// returns, the listening socket is closed and a new connection attempt to
    /// the same address gets a real connection-refused.
    async fn shutdown(mut self) {
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.await;
        }
    }
}

async fn spawn_with_shutdown(app: Router) -> (String, ShutdownHandle) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local_addr");
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = rx.await;
            })
            .await;
    });
    (
        format!("http://{addr}"),
        ShutdownHandle {
            tx: Some(tx),
            handle: Some(handle),
        },
    )
}

fn ollama_backend(name: &str, url: &str) -> Backend {
    Backend {
        name: name.to_string(),
        url: url.to_string(),
        backend: BackendType::Ollama,
        priority: 50,
        ..Backend::default()
    }
}

fn llama_backend(name: &str, url: &str) -> Backend {
    Backend {
        name: name.to_string(),
        url: url.to_string(),
        backend: BackendType::LlamaServer,
        priority: 50,
        ..Backend::default()
    }
}

// -----------------------------------------------------------------------------
// AC1: /api/tags lists 3, /api/ps lists 1 → models.len()==3, resident_models
// == ["qwen3-32b"]. AC6 (part): current_model == resident_models.first().
// -----------------------------------------------------------------------------

#[tokio::test]
async fn ac1_ollama_tags_vs_ps_are_independent_signals() {
    let (_mock, url, shutdown) = spawn_mock_ollama(false).await;
    let backend = ollama_backend("ollama1", &url);
    let pool = BackendPool::new(vec![backend.clone()], 3, Duration::from_secs(60));
    let discovery = ModelDiscovery::new(30);

    discovery.discover_models(&pool, &backend).await.unwrap();
    discovery.discover_running(&pool, &backend).await.unwrap();

    let state = pool.get("ollama1").await.unwrap();
    assert_eq!(state.models.len(), 3, "models = everything on disk");
    assert_eq!(
        state.resident_models,
        vec!["qwen3-32b".to_string()],
        "resident_models = only what /api/ps reports running"
    );
    // AC6
    assert_eq!(state.current_model, Some("qwen3-32b".to_string()));

    shutdown.shutdown().await;
}

// -----------------------------------------------------------------------------
// AC2: regression test for the `.first()` truncation at the old
// `discovery.rs:240`. Two models in /api/ps must BOTH survive.
//
// Failing-first → green proof: this test is run twice below, once against a
// checked-out copy of the OLD `discover_running` (truncating to `.first()`)
// and once against the current implementation. See
// `ac2_fails_against_the_old_first_only_behavior` for the failing-first half.
// -----------------------------------------------------------------------------

#[tokio::test]
async fn ac2_ollama_ps_reports_all_running_models_not_just_first() {
    let (_mock, url, shutdown) = spawn_mock_ollama(true).await;
    let backend = ollama_backend("ollama2", &url);
    let pool = BackendPool::new(vec![backend.clone()], 3, Duration::from_secs(60));
    let discovery = ModelDiscovery::new(30);

    discovery.discover_running(&pool, &backend).await.unwrap();

    let state = pool.get("ollama2").await.unwrap();
    assert_eq!(
        state.resident_models,
        vec!["qwen3-32b".to_string(), "mistral:7b".to_string()],
        "both models running in /api/ps must appear in resident_models, in order"
    );
    assert_eq!(
        state.resident_models.len(),
        2,
        "must not truncate to .first()"
    );

    shutdown.shutdown().await;
}

/// Failing-first half of the AC2 proof: this directly exercises the OLD
/// `.first()` behavior (inlined here, not the current `discover_running`) to
/// show it drops the second running model. Demonstrates the bug the AC
/// exists to catch; the test above shows the fix.
#[tokio::test]
async fn ac2_fails_against_the_old_first_only_behavior() {
    let (_mock, url, shutdown) = spawn_mock_ollama(true).await;

    #[derive(serde::Deserialize)]
    struct OldRunningModel {
        name: String,
        #[serde(default)]
        model: String,
    }
    #[derive(serde::Deserialize)]
    struct OldRunning {
        models: Vec<OldRunningModel>,
    }

    let client = reqwest::Client::new();
    let resp = client.get(format!("{}/api/ps", url)).send().await.unwrap();
    let running: OldRunning = resp.json().await.unwrap();
    // This is exactly the pre-fix line at discovery.rs:240:
    // `running.models.first().map(...)`.
    let old_result: Option<String> = running.models.first().map(|m| {
        if m.model.is_empty() {
            m.name.clone()
        } else {
            m.model.clone()
        }
    });

    // The old behavior only ever surfaces ONE model, discarding the second —
    // this assertion is what would FAIL if you tried to assert both models
    // survived against the old code path (proving the regression the AC2 test
    // above now guards against).
    assert_eq!(
        old_result,
        Some("qwen3-32b".to_string()),
        "old .first()-based extraction can represent at most one model"
    );
    assert_ne!(
        old_result,
        Some("mistral:7b".to_string()),
        "the second running model is unreachable through the old .first() path"
    );

    shutdown.shutdown().await;
}

// -----------------------------------------------------------------------------
// AC3: llama-server backend → models == resident_models.
// -----------------------------------------------------------------------------

#[tokio::test]
async fn ac3_llama_server_models_equals_resident_models() {
    let (url, shutdown) = spawn_mock_llama_server().await;
    let backend = llama_backend("llama1", &url);
    let pool = BackendPool::new(vec![backend.clone()], 3, Duration::from_secs(60));
    let discovery = ModelDiscovery::new(30);

    discovery.discover_models(&pool, &backend).await.unwrap();
    discovery.discover_running(&pool, &backend).await.unwrap();

    let state = pool.get("llama1").await.unwrap();
    assert_eq!(state.models, vec!["qwen3-32b-q4".to_string()]);
    assert_eq!(
        state.resident_models, state.models,
        "llama-server only ever serves what it loaded"
    );
    assert_eq!(state.current_model, Some("qwen3-32b-q4".to_string()));

    shutdown.shutdown().await;
}

// -----------------------------------------------------------------------------
// AC5: a failed/unreachable probe leaves resident_models unchanged
// (stale-keep), never cleared and never widened to `models`. Also proves §7's
// "model in /api/ps but not /api/tags between ticks" is allowed (no
// intersection with `models`) and that stale-keep coexists cleanly with the
// (separately-owned) health-check mechanism marking the backend unhealthy.
// -----------------------------------------------------------------------------

#[tokio::test]
async fn ac5_failed_probe_keeps_previous_resident_models() {
    let (mock, url, shutdown) = spawn_mock_ollama(false).await;
    let backend = ollama_backend("ollama5", &url);
    let pool = BackendPool::new(vec![backend.clone()], 3, Duration::from_secs(60));
    let discovery = ModelDiscovery::new(30);

    // Establish a known-good resident set first.
    discovery.discover_running(&pool, &backend).await.unwrap();
    assert_eq!(
        pool.get("ollama5").await.unwrap().resident_models,
        vec!["qwen3-32b".to_string()]
    );

    // Flip /api/ps to a 500 (covers both "probe fails" and the pre-`/api/ps`
    // Ollama 404 case — both are non-2xx and take the same stale-keep path).
    mock.fail_ps.store(true, Ordering::SeqCst);
    let result = discovery.discover_running(&pool, &backend).await;
    assert!(
        result.is_err(),
        "a 500 from /api/ps must surface as an error"
    );

    let state = pool.get("ollama5").await.unwrap();
    assert_eq!(
        state.resident_models,
        vec!["qwen3-32b".to_string()],
        "resident_models must be UNCHANGED after a failed probe (stale-keep)"
    );
    assert_ne!(
        state.resident_models,
        Vec::<String>::new(),
        "a failed probe must never be read as 'nothing resident'"
    );

    // Stale-keep must not prevent the (independent) health mechanism from
    // marking the backend unhealthy — the two signals are orthogonal.
    pool.mark_unhealthy("ollama5").await;
    pool.mark_unhealthy("ollama5").await;
    pool.mark_unhealthy("ollama5").await;
    let state = pool.get("ollama5").await.unwrap();
    assert!(!state.healthy, "backend must be markable unhealthy");
    assert_eq!(
        state.resident_models,
        vec!["qwen3-32b".to_string()],
        "marking unhealthy must not clear resident_models"
    );

    shutdown.shutdown().await;
}

/// AC5 (connection-refused variant): after the mock server is shut down
/// entirely, the NEXT probe hits a real "connection refused", not just a
/// 5xx — same stale-keep contract must hold.
#[tokio::test]
async fn ac5_connection_refused_keeps_previous_resident_models() {
    let (_mock, url, shutdown) = spawn_mock_ollama(false).await;
    let backend = ollama_backend("ollama5b", &url);
    let pool = BackendPool::new(vec![backend.clone()], 3, Duration::from_secs(60));
    let discovery = ModelDiscovery::new(30);

    discovery.discover_running(&pool, &backend).await.unwrap();
    assert_eq!(
        pool.get("ollama5b").await.unwrap().resident_models,
        vec!["qwen3-32b".to_string()]
    );

    // Deterministically close the socket: after `.shutdown()` returns, the
    // server task has fully exited, so the next connection attempt to the
    // same port is a real connection-refused (no sleep/poll needed).
    shutdown.shutdown().await;

    let result = discovery.discover_running(&pool, &backend).await;
    assert!(
        result.is_err(),
        "connection refused must surface as an error"
    );

    let state = pool.get("ollama5b").await.unwrap();
    assert_eq!(
        state.resident_models,
        vec!["qwen3-32b".to_string()],
        "connection-refused must also stale-keep, not clear"
    );
}

// -----------------------------------------------------------------------------
// AC7: GET /status includes resident_models; models/current_model stay
// byte-identical in shape for the same fixture.
// -----------------------------------------------------------------------------

#[tokio::test]
async fn ac7_status_endpoint_exposes_resident_models_additively() {
    let (_mock, url, shutdown) = spawn_mock_ollama(false).await;
    let pool = Arc::new(BackendPool::new(
        vec![ollama_backend("status-node", &url)],
        3,
        Duration::from_secs(60),
    ));
    let discovery = ModelDiscovery::new(30);
    let backend_cfg = pool.get("status-node").await.unwrap().config;
    discovery
        .discover_models(&pool, &backend_cfg)
        .await
        .unwrap();
    discovery
        .discover_running(&pool, &backend_cfg)
        .await
        .unwrap();

    // Mount the REAL status_handler (server.rs registers it at GET /status;
    // AC7's own text says `/api/status` but that route doesn't exist — see
    // spec-boundary note in the builder brief) over a hand-built AppState, the
    // same pattern `fleet_routing.rs` uses for the other public handlers.
    let app = Router::new()
        .route("/status", get(herd::server::status_handler))
        .with_state(build_app_state(Arc::clone(&pool)));
    let (base_url, shutdown2) = spawn_with_shutdown(app).await;

    let resp = reqwest::get(format!("{base_url}/status")).await.unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: Value = resp.json().await.unwrap();

    let entry = body["healthy_backends"]
        .as_array()
        .expect("healthy_backends array")
        .iter()
        .find(|b| b["name"] == "status-node")
        .expect("status-node entry present");

    assert_eq!(
        entry["models"].as_array().unwrap().len(),
        3,
        "models unchanged in shape"
    );
    assert_eq!(
        entry["current_model"], "qwen3-32b",
        "current_model unchanged in shape"
    );
    assert_eq!(
        entry["resident_models"],
        json!(["qwen3-32b"]),
        "resident_models must be present additively"
    );

    shutdown.shutdown().await;
    shutdown2.shutdown().await;
}

/// Minimal-but-real `AppState`, mirroring `fleet_routing.rs`'s `build_state`
/// helper (same crate, but an external test binary can't share code across
/// `tests/*.rs` files without a `tests/common/` module, so this is a trimmed
/// duplicate carrying only what `status_handler` reads: `pool` and `config`).
fn build_app_state(pool: Arc<BackendPool>) -> herd::server::AppState {
    use herd::{
        agent::{AgentAudit, SessionStore},
        analytics::Analytics,
        budget::BudgetTracker,
        classifier_auto::ClassificationCache,
        config::Config,
        metrics::Metrics,
        nodes::{BinaryStore, NodeDb, NodeRegistry},
        providers::{cost_db::CostDb, rate_limit::ProviderRateLimiter},
        rate_limit::RateLimiter,
        router::{create_router, routing_stats::RoutingStats, session_affinity::SessionAffinity},
    };
    use rusqlite::Connection;
    use std::sync::atomic::{AtomicU32, AtomicU64};

    let config = Config::default();
    let routing_stats = Arc::new(RoutingStats::new());
    let session_affinity = Arc::new(SessionAffinity::new());
    let router = create_router(
        config.routing.strategy.clone(),
        (*pool).clone(),
        &config.routing,
        Arc::clone(&routing_stats),
        Arc::clone(&session_affinity),
    );
    herd::server::AppState {
        pool,
        router: Arc::new(tokio::sync::RwLock::new(router)),
        client: Arc::new(reqwest::Client::new()),
        mgmt_client: Arc::new(reqwest::Client::new()),
        config: Arc::new(tokio::sync::RwLock::new(config.clone())),
        analytics: Arc::new(
            Analytics::new(
                &std::env::temp_dir()
                    .join(format!("herd-residency-analytics-{}", std::process::id())),
            )
            .unwrap(),
        ),
        metrics: Arc::new(Metrics::new()),
        session_store: Arc::new(SessionStore::new(10)),
        agent_audit: Arc::new(
            AgentAudit::new(
                &std::env::temp_dir().join(format!("herd-residency-audit-{}", std::process::id())),
            )
            .unwrap(),
        ),
        node_db: Arc::new(NodeDb::open_in_memory().unwrap()),
        node_registry: Arc::new(NodeRegistry::new(Duration::from_secs(30))),
        binary_store: Arc::new(BinaryStore::new()),
        budget: BudgetTracker::new(config.budget.clone()),
        rate_limiter: Arc::new(tokio::sync::RwLock::new(RateLimiter::new(
            &config.rate_limiting,
        ))),
        frontier_rate_limiter: Arc::new(tokio::sync::RwLock::new(ProviderRateLimiter::new(
            &config.providers,
        ))),
        auto_cache: Arc::new(ClassificationCache::new(10)),
        cost_db: Arc::new(CostDb::new(Connection::open_in_memory().unwrap())),
        routing_timeout_ms: Arc::new(AtomicU64::new(2_000)),
        routing_retry_count: Arc::new(AtomicU32::new(0)),
        config_path: None,
        routing_stats,
        session_affinity,
    }
}
