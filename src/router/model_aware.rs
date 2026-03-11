use crate::backend::BackendPool;
use crate::router::{RoutedBackend, Router};
use async_trait::async_trait;

#[derive(Clone)]
pub struct ModelAwareRouter {
    pool: BackendPool,
}

impl ModelAwareRouter {
    pub fn new(pool: BackendPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl Router for ModelAwareRouter {
    async fn route(
        &self,
        model: Option<&str>,
        tags: Option<&[String]>,
    ) -> anyhow::Result<RoutedBackend> {
        // If model specified, try to find backend with model loaded
        if let Some(model_name) = model {
            let backend = if let Some(tags) = tags {
                self.pool.get_by_model_tagged(model_name, tags).await
            } else {
                self.pool.get_by_model(model_name).await
            };
            if let Some(backend) = backend {
                tracing::debug!(
                    "Routing {} to {} (model loaded)",
                    model_name,
                    backend.config.name
                );
                return Ok(RoutedBackend {
                    name: backend.config.name.clone(),
                    url: backend.config.url.clone(),
                });
            }
        }

        // Fall back to highest priority healthy backend
        let backend = if let Some(tags) = tags {
            self.pool.get_by_priority_tagged(tags).await
        } else {
            self.pool.get_by_priority().await
        }
        .ok_or_else(|| anyhow::anyhow!("No healthy backends available"))?;

        tracing::debug!("Routing to {} (no model preference)", backend.config.name);
        Ok(RoutedBackend {
            name: backend.config.name.clone(),
            url: backend.config.url.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::BackendPool;
    use crate::config::Backend;
    use std::time::Duration;

    fn make_backend(name: &str, priority: u32) -> Backend {
        Backend {
            name: name.into(),
            url: "http://localhost:11434".into(),
            priority,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn routes_to_backend_with_model() {
        let pool = BackendPool::new(
            vec![make_backend("gpu1", 100), make_backend("gpu2", 50)],
            3,
            Duration::from_secs(60),
        );

        // Only gpu2 has the model loaded
        pool.update_models("gpu2", vec!["llama3".into()]).await;

        let router = ModelAwareRouter::new(pool);
        let result = router.route(Some("llama3"), None).await.unwrap();
        assert_eq!(result.name, "gpu2");
    }

    #[tokio::test]
    async fn falls_back_to_priority() {
        let pool = BackendPool::new(
            vec![make_backend("gpu1", 100), make_backend("gpu2", 50)],
            3,
            Duration::from_secs(60),
        );

        // No backend has the requested model
        let router = ModelAwareRouter::new(pool);
        let result = router.route(Some("llama3"), None).await.unwrap();
        // Falls back to highest priority
        assert_eq!(result.name, "gpu1");
    }

    #[tokio::test]
    async fn prefers_least_busy_with_model() {
        use crate::backend::GpuMetrics;

        let pool = BackendPool::new(
            vec![make_backend("gpu1", 100), make_backend("gpu2", 50)],
            3,
            Duration::from_secs(60),
        );

        // Both backends have the model loaded
        pool.update_models("gpu1", vec!["llama3".into()]).await;
        pool.update_models("gpu2", vec!["llama3".into()]).await;

        // gpu1 is under heavy load, gpu2 is idle
        pool.update_gpu_metrics(
            "gpu1",
            GpuMetrics {
                utilization: 90.0,
                memory_used: 12000,
                memory_total: 16000,
                temperature: 75.0,
            },
        )
        .await;
        pool.update_gpu_metrics(
            "gpu2",
            GpuMetrics {
                utilization: 5.0,
                memory_used: 2000,
                memory_total: 16000,
                temperature: 40.0,
            },
        )
        .await;

        let router = ModelAwareRouter::new(pool);
        let result = router.route(Some("llama3"), None).await.unwrap();
        // Should pick gpu2 (least busy) despite gpu1 having higher priority
        assert_eq!(result.name, "gpu2");
    }
}
