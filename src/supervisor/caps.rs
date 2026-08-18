use crate::supervisor::abi::{AbiError, CapId, Right};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct CapEntry {
    pub id: CapId,
    pub right: Right,
}

#[derive(Debug, Clone)]
pub struct CapTable {
    entries: HashMap<CapId, Right>,
    next_id: u32,
}

impl CapTable {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            next_id: 1,
        }
    }

    pub fn root() -> Self {
        let mut table = Self::new();
        table.entries.insert(CapId::new(1), Right::ROOT);
        table.next_id = 2;
        table
    }

    pub fn subset(parent: &CapTable, requested_rights: &[Right]) -> Result<CapTable, AbiError> {
        let parent_union = parent.union_all();

        for right in requested_rights {
            if !right.is_empty() && !parent_union.contains(*right) {
                return Err(AbiError::CapDenied);
            }
        }

        let mut child = CapTable::new();
        for right in requested_rights {
            if !right.is_empty() {
                let id = CapId::new(child.next_id);
                child.next_id += 1;
                child.entries.insert(id, *right);
            }
        }

        Ok(child)
    }

    pub fn holds(&self, right: Right) -> bool {
        let union = self.union_all();
        union.contains(right)
    }

    pub fn grant(&mut self, right: Right) -> Result<(), AbiError> {
        if right.is_empty() {
            return Err(AbiError::InvalidArg);
        }

        let id = CapId::new(self.next_id);
        self.next_id += 1;
        self.entries.insert(id, right);
        Ok(())
    }

    pub fn revoke_right(&mut self, right: Right) {
        self.entries.retain(|_, r| !r.contains(right));
    }

    fn union_all(&self) -> Right {
        self.entries
            .values()
            .fold(Right::empty(), |acc, r| acc.union(*r))
    }

    pub fn entries(&self) -> impl Iterator<Item = (&CapId, &Right)> {
        self.entries.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_cap_table_holds_all_rights() {
        let root = CapTable::root();
        assert!(root.holds(Right::ROOT));
        assert!(root.holds(Right::INFER));
        assert!(root.holds(Right::SPAWN_AGENT));
        assert!(root.holds(Right::SECRET_USE));
    }

    #[test]
    fn subset_succeeds_when_parent_has_rights() {
        let parent = CapTable::root();
        let requested = [Right::INFER, Right::SEND];
        let child = CapTable::subset(&parent, &requested).unwrap();

        assert!(child.holds(Right::INFER));
        assert!(child.holds(Right::SEND));
        assert!(!child.holds(Right::SPAWN_AGENT));
    }

    #[test]
    fn subset_fails_when_parent_lacks_rights() {
        let mut parent = CapTable::new();
        parent.grant(Right::INFER).unwrap();

        let requested = [Right::SPAWN_AGENT];
        let result = CapTable::subset(&parent, &requested);

        assert!(matches!(result, Err(AbiError::CapDenied)));
    }

    #[test]
    fn grant_adds_capability() {
        let mut table = CapTable::new();
        assert!(!table.holds(Right::INFER));

        table.grant(Right::INFER).unwrap();
        assert!(table.holds(Right::INFER));
    }

    #[test]
    fn grant_empty_right_fails() {
        let mut table = CapTable::new();
        let result = table.grant(Right::empty());
        assert!(matches!(result, Err(AbiError::InvalidArg)));
    }

    #[test]
    fn revoke_right_removes_all_matching() {
        let mut table = CapTable::new();
        table.grant(Right::INFER).unwrap();
        table.grant(Right::SEND).unwrap();
        table.grant(Right::INFER.union(Right::RECV)).unwrap();

        assert!(table.holds(Right::INFER));
        assert!(table.holds(Right::SEND));

        table.revoke_right(Right::INFER);

        assert!(!table.holds(Right::INFER));
        assert!(table.holds(Right::SEND));
        assert!(!table.holds(Right::RECV));
    }

    #[test]
    fn union_all_combines_entries() {
        let mut table = CapTable::new();
        table.grant(Right::INFER).unwrap();
        table.grant(Right::SEND).unwrap();

        let union = table.union_all();
        assert!(union.contains(Right::INFER));
        assert!(union.contains(Right::SEND));
        assert!(!union.contains(Right::SPAWN_AGENT));
    }

    #[test]
    fn holds_uses_mask_union_not_single_entry() {
        let mut table = CapTable::new();
        table.grant(Right::INFER).unwrap();
        table.grant(Right::SEND).unwrap();

        assert!(table.holds(Right::INFER.union(Right::SEND)));
    }
}
