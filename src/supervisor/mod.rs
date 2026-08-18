mod abi;
mod acb;
mod attention;
mod caps;
mod secrets;

pub use abi::{AbiError, AgentId, CapId, Right, SpawnSpec, Syscall, Tier};
pub use acb::Acb;
pub use attention::AttentionScheduler;
pub use caps::CapTable;
pub use secrets::{SecretHandle, SecretStore};

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct Supervisor {
    agents: Arc<RwLock<HashMap<AgentId, Acb>>>,
    next_agent_id: Arc<std::sync::atomic::AtomicU64>,
    scheduler: Arc<AttentionScheduler>,
}

impl Supervisor {
    pub fn new(default_attention_tokens: u32) -> Self {
        let mut agents = HashMap::new();
        let root_acb = Acb::root(default_attention_tokens);
        agents.insert(AgentId::ROOT, root_acb);

        Self {
            agents: Arc::new(RwLock::new(agents)),
            next_agent_id: Arc::new(std::sync::atomic::AtomicU64::new(1)),
            scheduler: Arc::new(AttentionScheduler::new()),
        }
    }

    pub async fn get_or_create_root(&self) -> AgentId {
        AgentId::ROOT
    }

    pub async fn spawn_agent(
        &self,
        parent: AgentId,
        spec: SpawnSpec,
    ) -> Result<AgentId, AbiError> {
        let mut agents = self.agents.write().await;

        let parent_acb = agents.get(&parent).ok_or(AbiError::NoAgent)?;

        if !parent_acb.caps.holds(Right::SPAWN_AGENT) {
            return Err(AbiError::CapDenied);
        }

        if !Tier::can_delegate(parent_acb.tier, spec.tier) {
            return Err(AbiError::TierDenied);
        }

        let child_caps = CapTable::subset(&parent_acb.caps, &spec.rights)?;

        let child_id = AgentId(
            self.next_agent_id
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst),
        );
        let child_acb = Acb {
            id: child_id,
            tier: spec.tier,
            parent: Some(parent),
            caps: child_caps,
            attention_budget: spec.attention_budget,
        };

        agents.insert(child_id, child_acb);
        Ok(child_id)
    }

    pub async fn charge_tokens(&self, agent_id: AgentId, tokens: u32) -> Result<(), AbiError> {
        let mut agents = self.agents.write().await;
        let acb = agents.get_mut(&agent_id).ok_or(AbiError::NoAgent)?;

        if !self.scheduler.can_spend(acb, tokens) {
            return Err(AbiError::QueueEmpty);
        }

        self.scheduler.spend(acb, tokens);
        Ok(())
    }

    pub async fn check_budget(&self, agent_id: AgentId) -> Result<u32, AbiError> {
        let agents = self.agents.read().await;
        let acb = agents.get(&agent_id).ok_or(AbiError::NoAgent)?;
        Ok(acb.attention_budget)
    }

    pub async fn check_cap(&self, agent_id: AgentId, right: Right) -> Result<bool, AbiError> {
        let agents = self.agents.read().await;
        let acb = agents.get(&agent_id).ok_or(AbiError::NoAgent)?;
        Ok(acb.caps.holds(right))
    }

    pub async fn grant_right(
        &self,
        agent_id: AgentId,
        target: AgentId,
        right: Right,
    ) -> Result<(), AbiError> {
        let mut agents = self.agents.write().await;

        let granter = agents.get(&agent_id).ok_or(AbiError::NoAgent)?;
        if granter.tier != Tier::Director {
            return Err(AbiError::TierDenied);
        }
        if !granter.caps.holds(right) {
            return Err(AbiError::CapDenied);
        }

        let target_acb = agents.get_mut(&target).ok_or(AbiError::NotFound)?;
        target_acb.caps.grant(right)?;
        Ok(())
    }

    pub async fn revoke_right(
        &self,
        agent_id: AgentId,
        target: AgentId,
        right: Right,
    ) -> Result<(), AbiError> {
        let mut agents = self.agents.write().await;

        let revoker = agents.get(&agent_id).ok_or(AbiError::NoAgent)?;
        if revoker.tier != Tier::Director {
            return Err(AbiError::TierDenied);
        }

        let target_acb = agents.get_mut(&target).ok_or(AbiError::NotFound)?;
        target_acb.caps.revoke_right(right);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn root_agent_has_full_rights() {
        let supervisor = Supervisor::new(1000);
        let root = supervisor.get_or_create_root().await;

        assert!(supervisor.check_cap(root, Right::ROOT).await.unwrap());
        assert!(supervisor
            .check_cap(root, Right::SPAWN_AGENT)
            .await
            .unwrap());
        assert!(supervisor.check_cap(root, Right::INFER).await.unwrap());
    }

    #[tokio::test]
    async fn spawn_child_with_subset_caps() {
        let supervisor = Supervisor::new(1000);
        let root = supervisor.get_or_create_root().await;

        let spec = SpawnSpec {
            tier: Tier::Worker,
            priority: 5,
            attention_budget: 500,
            rights_len: 1,
            rights: [Right::INFER, Right::empty(), Right::empty(), Right::empty(), Right::empty(), Right::empty(), Right::empty(), Right::empty()],
        };

        let child = supervisor.spawn_agent(root, spec).await.unwrap();
        assert!(supervisor.check_cap(child, Right::INFER).await.unwrap());
        assert!(!supervisor
            .check_cap(child, Right::SPAWN_AGENT)
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn spawn_with_extra_caps_denied() {
        let supervisor = Supervisor::new(1000);
        let root = supervisor.get_or_create_root().await;

        let spec = SpawnSpec {
            tier: Tier::Worker,
            priority: 5,
            attention_budget: 500,
            rights_len: 1,
            rights: [Right::INFER, Right::empty(), Right::empty(), Right::empty(), Right::empty(), Right::empty(), Right::empty(), Right::empty()],
        };
        let worker = supervisor.spawn_agent(root, spec).await.unwrap();

        let spec2 = SpawnSpec {
            tier: Tier::Worker,
            priority: 5,
            attention_budget: 500,
            rights_len: 1,
            rights: [Right::SPAWN_AGENT, Right::empty(), Right::empty(), Right::empty(), Right::empty(), Right::empty(), Right::empty(), Right::empty()],
        };
        let result = supervisor.spawn_agent(worker, spec2).await;
        assert!(matches!(result, Err(AbiError::CapDenied)));
    }

    #[tokio::test]
    async fn charge_tokens_exhausts_budget() {
        let supervisor = Supervisor::new(100);
        let root = supervisor.get_or_create_root().await;

        supervisor.charge_tokens(root, 50).await.unwrap();
        assert_eq!(supervisor.check_budget(root).await.unwrap(), 50);

        supervisor.charge_tokens(root, 50).await.unwrap();
        assert_eq!(supervisor.check_budget(root).await.unwrap(), 0);

        let result = supervisor.charge_tokens(root, 1).await;
        assert!(matches!(result, Err(AbiError::QueueEmpty)));
    }

    #[tokio::test]
    async fn worker_cannot_mint_director() {
        let supervisor = Supervisor::new(1000);
        let root = supervisor.get_or_create_root().await;

        let spec = SpawnSpec {
            tier: Tier::Worker,
            priority: 5,
            attention_budget: 500,
            rights_len: 1,
            rights: [Right::SPAWN_AGENT, Right::empty(), Right::empty(), Right::empty(), Right::empty(), Right::empty(), Right::empty(), Right::empty()],
        };
        let worker = supervisor.spawn_agent(root, spec).await.unwrap();

        let spec2 = SpawnSpec {
            tier: Tier::Director,
            priority: 5,
            attention_budget: 500,
            rights_len: 1,
            rights: [Right::INFER, Right::empty(), Right::empty(), Right::empty(), Right::empty(), Right::empty(), Right::empty(), Right::empty()],
        };
        let result = supervisor.spawn_agent(worker, spec2).await;
        assert!(matches!(result, Err(AbiError::TierDenied)));
    }
}
