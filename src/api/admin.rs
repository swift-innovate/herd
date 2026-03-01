use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Deserialize)]
pub struct AddBackend {
    name: String,
    url: String,
    priority: Option<u32>,
    gpu_hot_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RemoveBackend {
    name: String,
}

#[derive(Debug, Serialize)]
pub struct AdminResponse {
    success: bool,
    message: String,
}

pub async fn add_backend(
    State(_pool): State<Arc<BackendPool>>,
    Json(_payload): Json<AddBackend>,
) -> Result<Json<AdminResponse>, StatusCode> {
    // Implementation would add backend to pool
    Ok(Json(AdminResponse {
        success: true,
        message: "Backend added".to_string(),
    }))
}

pub async fn remove_backend(
    State(_pool): State<Arc<BackendPool>>,
    Json(_payload): Json<RemoveBackend>,
) -> Result<Json<AdminResponse>, StatusCode> {
    // Implementation would remove backend from pool
    Ok(Json(AdminResponse {
        success: true,
        message: "Backend removed".to_string(),
    }))
}

pub async fn drain_backend(
    State(_pool): State<Arc<BackendPool>>,
    Json(_payload): Json<RemoveBackend>,
) -> Result<Json<AdminResponse>, StatusCode> {
    // Implementation would drain connections from backend
    Ok(Json(AdminResponse {
        success: true,
        message: "Backend drained".to_string(),
    }))
}