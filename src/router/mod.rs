pub mod least_busy;
pub mod model_aware;
pub mod priority;
pub mod weighted_round_robin;

use crate::backend::BackendPool;
use crate::config::RoutingStrategy;
use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait Router: Send + Sync {
    async fn route(&self, model: Option<&str>, tags: Option<&[String]>) -> Result<RoutedBackend>;
}

#[derive(Clone, Debug)]
pub struct RoutedBackend {
    pub name: String,
    pub url: String,
}

#[derive(Clone)]
pub enum RouterEnum {
    Priority(priority::PriorityRouter),
    ModelAware(model_aware::ModelAwareRouter),
    LeastBusy(least_busy::LeastBusyRouter),
    WeightedRoundRobin(weighted_round_robin::WeightedRoundRobinRouter),
}

#[async_trait]
impl Router for RouterEnum {
    async fn route(&self, model: Option<&str>, tags: Option<&[String]>) -> Result<RoutedBackend> {
        match self {
            RouterEnum::Priority(r) => r.route(model, tags).await,
            RouterEnum::ModelAware(r) => r.route(model, tags).await,
            RouterEnum::LeastBusy(r) => r.route(model, tags).await,
            RouterEnum::WeightedRoundRobin(r) => r.route(model, tags).await,
        }
    }
}

pub fn create_router(strategy: RoutingStrategy, pool: BackendPool) -> RouterEnum {
    match strategy {
        RoutingStrategy::Priority => RouterEnum::Priority(priority::PriorityRouter::new(pool)),
        RoutingStrategy::ModelAware => {
            RouterEnum::ModelAware(model_aware::ModelAwareRouter::new(pool))
        }
        RoutingStrategy::LeastBusy => RouterEnum::LeastBusy(least_busy::LeastBusyRouter::new(pool)),
        RoutingStrategy::WeightedRoundRobin => RouterEnum::WeightedRoundRobin(
            weighted_round_robin::WeightedRoundRobinRouter::new(pool),
        ),
    }
}
