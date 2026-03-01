use crate::backend::BackendPool;
use crate::router::{Router, RoutedBackend};
use async_trait::async_trait;

#[derive(Clone)]
pub struct PriorityRouter {
    pool: BackendPool,
}

impl PriorityRouter {
    pub fn new(pool: BackendPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl Router for PriorityRouter {
    async fn route(&self, _model: Option<&str>) -> anyhow::Result<RoutedBackend> {
        // Route to highest priority healthy backend
        let backend = self
            .pool
            .get_by_priority()
            .await
            .ok_or_else(|| anyhow::anyhow!("No healthy backends available"))?;

        Ok(RoutedBackend {
            name: backend.config.name.clone(),
            url: backend.config.url.clone(),
        })
    }
}