use crate::backend::BackendPool;
use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Serialize)]
pub struct Status {
    healthy_backends: Vec<BackendStatus>,
    unhealthy_backends: Vec<String>,
    routing_strategy: String,
}

#[derive(Debug, Serialize)]
pub struct BackendStatus {
    name: String,
    url: String,
    priority: u32,
    healthy: bool,
    models: Vec<String>,
    gpu: Option<GpuStatus>,
}

#[derive(Debug, Serialize)]
pub struct GpuStatus {
    utilization: f32,
    memory_used: u64,
    memory_total: u64,
    memory_percent: f32,
    temperature: f32,
}

pub async fn get_status(State(pool): State<Arc<BackendPool>>) -> Json<Status> {
    // Implementation would query the pool for all backend states
    Json(Status {
        healthy_backends: vec![],
        unhealthy_backends: vec![],
        routing_strategy: "model_aware".to_string(),
    })
}

pub async fn get_backend_status(
    State(pool): State<Arc<BackendPool>>,
) -> Json<Vec<BackendStatus>> {
    Json(vec![])
}