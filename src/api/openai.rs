use crate::server::AppState;
use axum::{extract::State, Json};
use serde_json::{json, Value};

/// GET /v1/models — OpenAI-compatible model listing.
/// Aggregates unique model names from all healthy backends.
pub async fn list_models(State(state): State<AppState>) -> Json<Value> {
    let mut seen = std::collections::HashSet::new();
    let mut models = Vec::new();

    for name in state.pool.all_healthy().await {
        if let Some(backend) = state.pool.get(&name).await {
            for model in &backend.models {
                if seen.insert(model.clone()) {
                    models.push(json!({
                        "id": model,
                        "object": "model",
                        "created": 0,
                        "owned_by": "ollama",
                    }));
                }
            }
        }
    }

    Json(json!({ "object": "list", "data": models }))
}
