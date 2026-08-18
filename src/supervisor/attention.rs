use crate::supervisor::acb::Acb;

pub struct AttentionScheduler {}

impl AttentionScheduler {
    pub fn new() -> Self {
        Self {}
    }

    pub fn can_spend(&self, acb: &Acb, tokens: u32) -> bool {
        acb.attention_budget >= tokens
    }

    pub fn spend(&self, acb: &mut Acb, tokens: u32) {
        acb.attention_budget = acb.attention_budget.saturating_sub(tokens);
    }

    pub fn is_exhausted(&self, acb: &Acb) -> bool {
        acb.attention_budget == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::supervisor::abi::{AgentId, Tier};
    use crate::supervisor::caps::CapTable;

    fn make_acb(budget: u32) -> Acb {
        Acb::new(
            AgentId(1),
            Tier::Worker,
            Some(AgentId::ROOT),
            CapTable::new(),
            budget,
        )
    }

    #[test]
    fn can_spend_returns_true_when_budget_sufficient() {
        let acb = make_acb(100);
        let scheduler = AttentionScheduler::new();
        assert!(scheduler.can_spend(&acb, 50));
        assert!(scheduler.can_spend(&acb, 100));
    }

    #[test]
    fn can_spend_returns_false_when_budget_insufficient() {
        let acb = make_acb(50);
        let scheduler = AttentionScheduler::new();
        assert!(!scheduler.can_spend(&acb, 51));
        assert!(!scheduler.can_spend(&acb, 100));
    }

    #[test]
    fn spend_decrements_budget() {
        let mut acb = make_acb(100);
        let scheduler = AttentionScheduler::new();

        scheduler.spend(&mut acb, 30);
        assert_eq!(acb.attention_budget, 70);

        scheduler.spend(&mut acb, 70);
        assert_eq!(acb.attention_budget, 0);
    }

    #[test]
    fn spend_uses_saturating_subtraction() {
        let mut acb = make_acb(10);
        let scheduler = AttentionScheduler::new();

        scheduler.spend(&mut acb, 15);
        assert_eq!(acb.attention_budget, 0);

        scheduler.spend(&mut acb, 100);
        assert_eq!(acb.attention_budget, 0);
    }

    #[test]
    fn is_exhausted_detects_zero_budget() {
        let scheduler = AttentionScheduler::new();
        assert!(!scheduler.is_exhausted(&make_acb(1)));

        let mut exhausted = make_acb(10);
        exhausted.attention_budget = 0;
        assert!(scheduler.is_exhausted(&exhausted));
    }

    #[test]
    fn spend_until_exhausted() {
        let mut acb = make_acb(100);
        let scheduler = AttentionScheduler::new();

        while scheduler.can_spend(&acb, 1) {
            scheduler.spend(&mut acb, 1);
        }

        assert!(scheduler.is_exhausted(&acb));
        assert!(!scheduler.can_spend(&acb, 1));
    }
}
