use crate::supervisor::abi::{AgentId, Tier};
use crate::supervisor::caps::CapTable;

#[derive(Debug, Clone)]
pub struct Acb {
    pub id: AgentId,
    pub tier: Tier,
    pub parent: Option<AgentId>,
    pub caps: CapTable,
    pub attention_budget: u32,
}

impl Acb {
    pub fn root(default_attention: u32) -> Self {
        let attention_budget = if default_attention > 0 {
            default_attention
        } else {
            u32::MAX / 4
        };

        Self {
            id: AgentId::ROOT,
            tier: Tier::Director,
            parent: None,
            caps: CapTable::root(),
            attention_budget,
        }
    }

    pub fn new(
        id: AgentId,
        tier: Tier,
        parent: Option<AgentId>,
        caps: CapTable,
        attention_budget: u32,
    ) -> Self {
        let attention_budget = if attention_budget > 0 {
            attention_budget
        } else {
            1
        };

        Self {
            id,
            tier,
            parent,
            caps,
            attention_budget,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_acb_has_director_tier() {
        let root = Acb::root(1000);
        assert_eq!(root.id, AgentId::ROOT);
        assert_eq!(root.tier, Tier::Director);
        assert!(root.parent.is_none());
    }

    #[test]
    fn root_acb_defaults_to_quarter_max_when_zero() {
        let root = Acb::root(0);
        assert_eq!(root.attention_budget, u32::MAX / 4);
    }

    #[test]
    fn child_acb_coerces_zero_budget_to_one() {
        let acb = Acb::new(
            AgentId(1),
            Tier::Worker,
            Some(AgentId::ROOT),
            CapTable::new(),
            0,
        );
        assert_eq!(acb.attention_budget, 1);
    }

    #[test]
    fn child_acb_preserves_nonzero_budget() {
        let acb = Acb::new(
            AgentId(1),
            Tier::Worker,
            Some(AgentId::ROOT),
            CapTable::new(),
            500,
        );
        assert_eq!(acb.attention_budget, 500);
    }
}
