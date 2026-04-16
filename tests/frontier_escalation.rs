//! Sprint 3 integration tests for auto-mode -> frontier escalation.
//!
//! Exercises `herd::providers::frontier_route_if_applicable` decision logic
//! and the full proxy path using a minimal in-process HTTP listener as the
//! mock frontier provider.

use axum::http::HeaderMap;
use herd::classifier_auto::Classification;
use herd::config::{FrontierConfig, ProviderConfig};
use herd::providers::{cost_db::CostDb, frontier_route_if_applicable};
use rusqlite::Connection;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

// -----------------------------------------------------------------------------
// Mock provider: accepts one request, returns a canned 200 JSON response.
// -----------------------------------------------------------------------------

async fn spawn_mock_provider(response_body: &'static str) -> (String, Arc<tokio::sync::Notify>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let notify = Arc::new(tokio::sync::Notify::new());
    let notify_clone = notify.clone();

    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let note = notify_clone.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let _ = socket.read(&mut buf).await;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                    response_body.len(),
                    response_body
                );
                let _ = socket.write_all(resp.as_bytes()).await;
                let _ = socket.shutdown().await;
                note.notify_one();
            });
        }
    });

    (format!("http://{}", addr), notify)
}

fn provider(name: &str, url: &str, model: &str) -> ProviderConfig {
    ProviderConfig {
        name: name.to_string(),
        api_url: url.to_string(),
        api_key_env: format!("TEST_{}_KEY", name.to_uppercase()),
        models: vec![model.to_string()],
        priority: 100,
        monthly_budget: 100.0,
        ..Default::default()
    }
}

fn in_memory_cost_db() -> CostDb {
    let conn = Connection::open_in_memory().unwrap();
    CostDb::new(conn)
}

fn frontier_tier() -> Classification {
    Classification {
        tier: "frontier".to_string(),
        capability: "reasoning".to_string(),
        needs_large_context: false,
        language: "en".to_string(),
    }
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[tokio::test]
async fn returns_none_when_frontier_disabled() {
    let client = reqwest::Client::new();
    let providers = vec![provider(
        "anthropic",
        "https://api.anthropic.com",
        "claude-sonnet-4",
    )];
    let cfg = FrontierConfig {
        enabled: false,
        ..Default::default()
    };
    let cost_db = in_memory_cost_db();
    let headers = HeaderMap::new();

    let result = frontier_route_if_applicable(
        &client,
        &cfg,
        &providers,
        &cost_db,
        Some("claude-sonnet-4"),
        &headers,
        Some(&frontier_tier()),
        b"{}",
        "req-1",
    )
    .await;

    assert!(
        result.is_none(),
        "helper must return None when frontier disabled"
    );
}

#[tokio::test]
async fn returns_none_for_non_frontier_model() {
    let client = reqwest::Client::new();
    let providers = vec![provider(
        "anthropic",
        "https://api.anthropic.com",
        "claude-sonnet-4",
    )];
    let cfg = FrontierConfig {
        enabled: true,
        ..Default::default()
    };
    let cost_db = in_memory_cost_db();
    let headers = HeaderMap::new();

    let result = frontier_route_if_applicable(
        &client,
        &cfg,
        &providers,
        &cost_db,
        Some("qwen3:8b"),
        &headers,
        None,
        b"{}",
        "req-1",
    )
    .await;

    assert!(
        result.is_none(),
        "helper must return None for local model names"
    );
}

#[tokio::test]
async fn blocks_auto_escalation_when_flag_disabled() {
    // Classifier said frontier, but allow_auto_escalation=false.
    // Helper must return None so the caller can fall back to fallback_model,
    // preventing an unintended cloud request.
    let client = reqwest::Client::new();
    let providers = vec![provider(
        "anthropic",
        "https://api.anthropic.com",
        "claude-sonnet-4",
    )];
    let cfg = FrontierConfig {
        enabled: true,
        allow_auto_escalation: false,
        require_header: false,
        ..Default::default()
    };
    let cost_db = in_memory_cost_db();
    let headers = HeaderMap::new();

    let result = frontier_route_if_applicable(
        &client,
        &cfg,
        &providers,
        &cost_db,
        Some("claude-sonnet-4"),
        &headers,
        Some(&frontier_tier()),
        b"{}",
        "req-1",
    )
    .await;

    assert!(
        result.is_none(),
        "auto-classified frontier tier with allow_auto_escalation=false must return None"
    );
}

#[tokio::test]
async fn rejects_with_403_when_header_required_and_missing() {
    let client = reqwest::Client::new();
    let providers = vec![provider(
        "anthropic",
        "https://api.anthropic.com",
        "claude-sonnet-4",
    )];
    let cfg = FrontierConfig {
        enabled: true,
        require_header: true,
        allow_auto_escalation: false,
        ..Default::default()
    };
    let cost_db = in_memory_cost_db();
    let headers = HeaderMap::new();

    // Explicit model (no auto classification), require_header=true, no header.
    let result = frontier_route_if_applicable(
        &client,
        &cfg,
        &providers,
        &cost_db,
        Some("claude-sonnet-4"),
        &headers,
        None,
        b"{}",
        "req-1",
    )
    .await;

    let response = result.expect("helper must return a 403 response");
    assert_eq!(
        response.status(),
        axum::http::StatusCode::FORBIDDEN,
        "missing X-Herd-Frontier header must yield 403"
    );
}

#[tokio::test]
async fn auto_escalation_bypasses_header_requirement() {
    // Classifier returned frontier + allow_auto_escalation=true + require_header=true.
    // The escalation should bypass the header check and hit the mock provider.
    let mock_response = r#"{"id":"chatcmpl-x","choices":[{"message":{"role":"assistant","content":"ok"}}],"usage":{"prompt_tokens":5,"completion_tokens":2,"total_tokens":7}}"#;
    let (mock_url, _notify) = spawn_mock_provider(mock_response).await;

    std::env::set_var("TEST_MOCK_KEY", "sk-test");

    let client = reqwest::Client::new();
    let providers = vec![ProviderConfig {
        name: "mock".to_string(),
        api_url: mock_url,
        api_key_env: "TEST_MOCK_KEY".to_string(),
        models: vec!["mock-frontier-model".to_string()],
        priority: 100,
        monthly_budget: 100.0,
        ..Default::default()
    }];
    let cfg = FrontierConfig {
        enabled: true,
        require_header: true,
        allow_auto_escalation: true,
        ..Default::default()
    };
    let cost_db = in_memory_cost_db();
    let headers = HeaderMap::new();

    let result = frontier_route_if_applicable(
        &client,
        &cfg,
        &providers,
        &cost_db,
        Some("mock-frontier-model"),
        &headers,
        Some(&frontier_tier()),
        br#"{"messages":[{"role":"user","content":"hi"}]}"#,
        "req-1",
    )
    .await;

    let response = result.expect("helper must return a response");
    assert_eq!(
        response.status(),
        axum::http::StatusCode::OK,
        "auto-escalation should bypass require_header and reach the provider"
    );

    let provider_header = response
        .headers()
        .get("x-herd-provider")
        .expect("response must include X-Herd-Provider");
    assert_eq!(provider_header.to_str().unwrap(), "mock");

    let tier_header = response
        .headers()
        .get("x-herd-auto-tier")
        .expect("auto-escalation must emit X-Herd-Auto-Tier");
    assert_eq!(tier_header.to_str().unwrap(), "frontier");
}

#[tokio::test]
async fn explicit_header_allows_direct_frontier_call() {
    // User sent X-Herd-Frontier: true header with an explicit frontier model.
    // No auto classification, allow_auto_escalation irrelevant.
    let mock_response = r#"{"id":"chatcmpl-y","choices":[],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}"#;
    let (mock_url, _notify) = spawn_mock_provider(mock_response).await;

    std::env::set_var("TEST_MOCK2_KEY", "sk-test");

    let client = reqwest::Client::new();
    let providers = vec![ProviderConfig {
        name: "mock2".to_string(),
        api_url: mock_url,
        api_key_env: "TEST_MOCK2_KEY".to_string(),
        models: vec!["mock-explicit-model".to_string()],
        priority: 100,
        monthly_budget: 100.0,
        ..Default::default()
    }];
    let cfg = FrontierConfig {
        enabled: true,
        require_header: true,
        allow_auto_escalation: false,
        ..Default::default()
    };
    let cost_db = in_memory_cost_db();
    let mut headers = HeaderMap::new();
    headers.insert("x-herd-frontier", "true".parse().unwrap());

    let result = frontier_route_if_applicable(
        &client,
        &cfg,
        &providers,
        &cost_db,
        Some("mock-explicit-model"),
        &headers,
        None,
        br#"{"messages":[{"role":"user","content":"hi"}]}"#,
        "req-1",
    )
    .await;

    let response = result.expect("helper must return a response");
    assert_eq!(
        response.status(),
        axum::http::StatusCode::OK,
        "explicit X-Herd-Frontier: true header must allow direct call"
    );
}
