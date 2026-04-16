//! Frontier cost recording helper.
//!
//! After a successful frontier proxy, call [`record_frontier_cost`] to extract
//! token usage from the provider's response, look up pricing (respecting any
//! per-provider overrides), and persist the row to the [`CostDb`].
//!
//! The helper is best-effort: it never panics, never propagates errors, and
//! never blocks — missing usage or missing pricing simply returns `None` so
//! the caller can skip setting the `X-Herd-Cost-Estimate` header.

use crate::providers::{cost_db::CostDb, pricing};

/// Record a frontier response's token usage and cost.
///
/// Returns the computed cost in USD (so the caller can set
/// `X-Herd-Cost-Estimate`), or `None` if usage couldn't be extracted or
/// pricing is unknown.
///
/// This function is intentionally infallible from the caller's perspective:
/// DB errors are logged at `warn` level and swallowed.
pub fn record_frontier_cost(
    cost_db: &CostDb,
    provider: &crate::config::ProviderConfig,
    model: &str,
    response_body: &serde_json::Value,
    request_id: Option<&str>,
) -> Option<f32> {
    // 1. Select the right adapter for this provider (mirrors get_adapter in mod.rs).
    let adapter = crate::providers::get_adapter(provider);

    // 2. Pull (tokens_in, tokens_out) from the response shape.
    let (tokens_in, tokens_out) = adapter.extract_usage(response_body)?;

    // 3. Look up pricing (per-provider override wins over built-in table).
    let model_pricing = pricing::get_pricing_with_overrides(model, &provider.pricing)?;

    // 4. Compute cost in USD.
    let cost = pricing::calculate_cost(&model_pricing, tokens_in, tokens_out);

    // 5. Persist. Errors are logged and swallowed — we never fail the hot path.
    if let Err(e) = cost_db.record_cost(
        &provider.name,
        model,
        tokens_in,
        tokens_out,
        cost,
        request_id,
    ) {
        tracing::warn!(
            provider = %provider.name,
            model = %model,
            "Failed to record frontier cost: {}",
            e
        );
    }

    Some(cost)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{PricingOverride, ProviderConfig};
    use rusqlite::Connection;
    use serde_json::json;

    fn in_memory_db() -> CostDb {
        CostDb::new(Connection::open_in_memory().unwrap())
    }

    fn anthropic_provider() -> ProviderConfig {
        ProviderConfig {
            name: "anthropic".to_string(),
            api_url: "https://api.anthropic.com".to_string(),
            api_key_env: "ANTHROPIC_API_KEY".to_string(),
            models: vec!["claude-sonnet-4-20250514".to_string()],
            ..Default::default()
        }
    }

    fn openai_provider() -> ProviderConfig {
        ProviderConfig {
            name: "openai".to_string(),
            api_url: "https://api.openai.com".to_string(),
            api_key_env: "OPENAI_API_KEY".to_string(),
            models: vec!["gpt-4.1".to_string()],
            ..Default::default()
        }
    }

    #[test]
    fn records_cost_for_anthropic_response() {
        let db = in_memory_db();
        let provider = anthropic_provider();
        let body = json!({
            "id": "msg_1",
            "usage": {
                "input_tokens": 1_000_000,
                "output_tokens": 500_000,
            }
        });

        let cost = record_frontier_cost(
            &db,
            &provider,
            "claude-sonnet-4-20250514",
            &body,
            Some("req-a"),
        )
        .expect("should return cost");

        // claude-sonnet-4: $3/Mtok input, $15/Mtok output
        // 1M * $3 + 0.5M * $15 = $3 + $7.5 = $10.5
        assert!((cost - 10.5).abs() < 1e-4, "expected 10.5, got {cost}");

        let summary = db.cost_summary().unwrap();
        assert_eq!(summary.len(), 1);
        assert_eq!(summary[0].provider, "anthropic");
        assert_eq!(summary[0].total_tokens_in, 1_000_000);
        assert_eq!(summary[0].total_tokens_out, 500_000);
        assert!((summary[0].total_cost_usd - 10.5_f64).abs() < 1e-3);
    }

    #[test]
    fn records_cost_for_openai_response() {
        let db = in_memory_db();
        let provider = openai_provider();
        let body = json!({
            "id": "chatcmpl-1",
            "usage": {
                "prompt_tokens": 500_000,
                "completion_tokens": 250_000,
            }
        });

        let cost = record_frontier_cost(&db, &provider, "gpt-4.1", &body, Some("req-o"))
            .expect("should return cost");

        // gpt-4.1: $2/Mtok input, $8/Mtok output
        // 0.5M * $2 + 0.25M * $8 = $1 + $2 = $3
        assert!((cost - 3.0).abs() < 1e-4, "expected 3.0, got {cost}");

        let spend = db.monthly_spend("openai").unwrap();
        assert!((spend - 3.0_f64).abs() < 1e-3);
    }

    #[test]
    fn returns_none_when_usage_missing() {
        let db = in_memory_db();
        let provider = openai_provider();
        let body = json!({ "id": "chatcmpl-1", "choices": [] });

        let result = record_frontier_cost(&db, &provider, "gpt-4.1", &body, None);
        assert!(result.is_none());

        // Nothing was written.
        let summary = db.cost_summary().unwrap();
        assert!(summary.is_empty());
    }

    #[test]
    fn returns_none_for_unknown_model_without_override() {
        let db = in_memory_db();
        let provider = openai_provider();
        let body = json!({
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 50,
            }
        });

        let result = record_frontier_cost(&db, &provider, "some-unknown-model-xyz", &body, None);
        assert!(result.is_none());

        let summary = db.cost_summary().unwrap();
        assert!(summary.is_empty());
    }

    #[test]
    fn pricing_override_takes_precedence_over_builtin() {
        let db = in_memory_db();
        let mut provider = openai_provider();
        // Built-in for gpt-4.1 is $2/$8 per Mtok. Override to $1/$2 per Mtok.
        provider.pricing.insert(
            "gpt-4.1".to_string(),
            PricingOverride {
                input_per_mtok: 1.0,
                output_per_mtok: 2.0,
            },
        );

        let body = json!({
            "usage": {
                "prompt_tokens": 1_000_000,
                "completion_tokens": 1_000_000,
            }
        });

        let cost = record_frontier_cost(&db, &provider, "gpt-4.1", &body, Some("req-ov"))
            .expect("should return cost");

        // Override: 1M * $1 + 1M * $2 = $3 (vs built-in which would be $10)
        assert!(
            (cost - 3.0).abs() < 1e-4,
            "expected override cost 3.0, got {cost}"
        );

        let summary = db.cost_summary().unwrap();
        assert_eq!(summary.len(), 1);
        assert!((summary[0].total_cost_usd - 3.0_f64).abs() < 1e-3);
    }

    #[test]
    fn override_enables_pricing_for_unknown_model() {
        // Bonus: an override on an otherwise unknown model turns None into Some.
        let db = in_memory_db();
        let mut provider = openai_provider();
        provider.pricing.insert(
            "custom-model-7b".to_string(),
            PricingOverride {
                input_per_mtok: 0.5,
                output_per_mtok: 1.0,
            },
        );

        let body = json!({
            "usage": {
                "prompt_tokens": 2_000_000,
                "completion_tokens": 1_000_000,
            }
        });

        let cost = record_frontier_cost(&db, &provider, "custom-model-7b", &body, None)
            .expect("should return cost");

        // 2M * $0.5 + 1M * $1 = $2
        assert!((cost - 2.0).abs() < 1e-4, "expected 2.0, got {cost}");

        let spend = db.monthly_spend("openai").unwrap();
        assert!((spend - 2.0_f64).abs() < 1e-3);
    }

    #[test]
    fn summary_reflects_recorded_row() {
        // Explicit end-to-end proof: after one record, cost_summary sees the row
        // with all fields intact (tokens, cost, request_count).
        let db = in_memory_db();
        let provider = anthropic_provider();
        let body = json!({
            "usage": {
                "input_tokens": 250,
                "output_tokens": 120,
            }
        });

        let cost = record_frontier_cost(
            &db,
            &provider,
            "claude-sonnet-4-20250514",
            &body,
            Some("req-summary"),
        );
        assert!(cost.is_some());

        let summary = db.cost_summary().unwrap();
        assert_eq!(summary.len(), 1);
        let entry = &summary[0];
        assert_eq!(entry.provider, "anthropic");
        assert_eq!(entry.total_tokens_in, 250);
        assert_eq!(entry.total_tokens_out, 120);
        assert_eq!(entry.request_count, 1);
        assert!(entry.total_cost_usd > 0.0);
    }
}
