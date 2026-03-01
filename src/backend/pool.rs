use crate::config::Backend;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct BackendState {
    pub config: Backend,
    pub healthy: bool,
    pub models: Vec<String>,
    pub current_model: Option<String>,
    pub gpu_metrics: Option<GpuMetrics>,
    pub failure_count: u32,
    pub last_check: Instant,
    pub last_request: Instant,
}

#[derive(Debug, Clone)]
pub struct GpuMetrics {
    pub utilization: f32,
    pub memory_used: u64,
    pub memory_total: u64,
    pub temperature: f32,
}

#[derive(Clone)]
pub struct BackendPool {
    pub backends: Arc<RwLock<Vec<BackendState>>>,
}

impl BackendPool {
    pub fn new(backends: Vec<Backend>) -> Self {
        let now = Instant::now();
        let states = backends
            .into_iter()
            .map(|config| BackendState {
                config,
                healthy: true,
                models: Vec::new(),
                current_model: None,
                gpu_metrics: None,
                failure_count: 0,
                last_check: now,
                last_request: now,
            })
            .collect();

        Self {
            backends: Arc::new(RwLock::new(states)),
        }
    }

    pub async fn all_healthy(&self) -> Vec<String> {
        let backends = self.backends.read().await;
        backends
            .iter()
            .filter(|b| b.healthy)
            .map(|b| b.config.name.clone())
            .collect()
    }

    pub async fn all(&self) -> Vec<String> {
        let backends = self.backends.read().await;
        backends.iter().map(|b| b.config.name.clone()).collect()
    }

    pub async fn get(&self, name: &str) -> Option<BackendState> {
        let backends = self.backends.read().await;
        backends
            .iter()
            .find(|b| b.config.name == name)
            .cloned()
    }

    pub async fn get_healthy(&self, name: &str) -> Option<BackendState> {
        let backends = self.backends.read().await;
        backends
            .iter()
            .find(|b| b.config.name == name && b.healthy)
            .cloned()
    }

    pub async fn get_by_priority(&self) -> Option<BackendState> {
        let backends = self.backends.read().await;
        backends
            .iter()
            .filter(|b| b.healthy)
            .max_by_key(|b| b.config.priority)
            .cloned()
    }

    pub async fn get_by_model(&self, model: &str) -> Option<BackendState> {
        let backends = self.backends.read().await;
        backends
            .iter()
            .filter(|b| b.healthy && b.models.contains(&model.to_string()))
            .max_by_key(|b| b.config.priority)
            .cloned()
    }

    pub async fn get_least_busy(&self) -> Option<BackendState> {
        let backends = self.backends.read().await;
        backends
            .iter()
            .filter(|b| b.healthy)
            .min_by(|a, b| {
                let a_busy = a.gpu_metrics.as_ref().map(|g| g.utilization).unwrap_or(0.0);
                let b_busy = b.gpu_metrics.as_ref().map(|g| g.utilization).unwrap_or(0.0);
                a_busy.partial_cmp(&b_busy).unwrap_or(std::cmp::Ordering::Equal)
            })
            .cloned()
    }

    pub async fn mark_healthy(&self, name: &str) {
        let mut backends = self.backends.write().await;
        if let Some(backend) = backends.iter_mut().find(|b| b.config.name == name) {
            backend.healthy = true;
            backend.failure_count = 0;
            backend.last_check = Instant::now();
        }
    }

    pub async fn mark_unhealthy(&self, name: &str) {
        let mut backends = self.backends.write().await;
        if let Some(backend) = backends.iter_mut().find(|b| b.config.name == name) {
            backend.failure_count += 1;
            if backend.failure_count >= 3 {
                backend.healthy = false;
            }
            backend.last_check = Instant::now();
        }
    }

    pub async fn update_models(&self, name: &str, models: Vec<String>) {
        let mut backends = self.backends.write().await;
        if let Some(backend) = backends.iter_mut().find(|b| b.config.name == name) {
            backend.models = models;
        }
    }

    pub async fn update_current_model(&self, name: &str, model: Option<String>) {
        let mut backends = self.backends.write().await;
        if let Some(backend) = backends.iter_mut().find(|b| b.config.name == name) {
            backend.current_model = model;
        }
    }

    pub async fn update_gpu_metrics(&self, name: &str, metrics: GpuMetrics) {
        let mut backends = self.backends.write().await;
        if let Some(backend) = backends.iter_mut().find(|b| b.config.name == name) {
            backend.gpu_metrics = Some(metrics);
        }
    }

    pub async fn touch_request(&self, name: &str) {
        let mut backends = self.backends.write().await;
        if let Some(backend) = backends.iter_mut().find(|b| b.config.name == name) {
            backend.last_request = Instant::now();
        }
    }

    pub async fn add(&self, backend: Backend) {
        let mut backends = self.backends.write().await;
        backends.push(BackendState {
            config: backend,
            healthy: true,
            models: Vec::new(),
            current_model: None,
            gpu_metrics: None,
            failure_count: 0,
            last_check: Instant::now(),
            last_request: Instant::now(),
        });
    }

    pub async fn update(&self, state: BackendState) {
        let mut backends = self.backends.write().await;
        if let Some(backend) = backends.iter_mut().find(|b| b.config.name == state.config.name) {
            *backend = state;
        }
    }

    pub async fn remove(&self, name: &str) -> bool {
        let mut backends = self.backends.write().await;
        let len_before = backends.len();
        backends.retain(|b| b.config.name != name);
        backends.len() < len_before
    }
}