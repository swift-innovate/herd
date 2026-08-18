use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AgentId(pub u64);

impl AgentId {
    pub const ROOT: AgentId = AgentId(0);
}

impl fmt::Display for AgentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "agent:{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CapId(pub u32);

impl CapId {
    pub fn new(id: u32) -> Self {
        CapId(id)
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    Director = 0,
    Worker = 1,
    Ephemeral = 2,
}

impl Tier {
    pub fn can_delegate(parent: Tier, child: Tier) -> bool {
        child >= parent
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Right(pub u32);

impl Right {
    pub const TIER_MASK: u32 = 0x0000_00FF;
    pub const FLAVOR_MASK: u32 = 0xFFFF_FF00;

    pub const SERIAL_WRITE: Right = Right(1 << 8);
    pub const SPAWN_AGENT: Right = Right(1 << 9);
    pub const SCHED_YIELD: Right = Right(1 << 10);
    pub const SEND: Right = Right(1 << 11);
    pub const RECV: Right = Right(1 << 12);
    pub const INFER: Right = Right(1 << 13);
    pub const SECRET_USE: Right = Right(1 << 14);

    pub const ROOT: Right = Right(
        Self::SERIAL_WRITE.0
            | Self::SPAWN_AGENT.0
            | Self::SCHED_YIELD.0
            | Self::SEND.0
            | Self::RECV.0
            | Self::INFER.0
            | Self::SECRET_USE.0,
    );

    pub const fn empty() -> Self {
        Right(0)
    }

    pub fn union(self, other: Right) -> Right {
        Right(self.0 | other.0)
    }

    pub fn contains(self, other: Right) -> bool {
        (self.0 & other.0) == other.0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
}

#[repr(u64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Syscall {
    SpawnAgent = 1,
    Send = 2,
    Recv = 3,
    YieldAttention = 4,
    Grant = 5,
    Revoke = 6,
    RecvBlocking = 7,
    RequestInference = 8,
    ContextWrite = 9,
    ContextRead = 10,
    Seal = 11,
    InvokeAuthed = 12,
}

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbiError {
    Ok = 0,
    InvalidArg = -1,
    NotFound = -2,
    CapDenied = -3,
    TierDenied = -4,
    QueueFull = -5,
    QueueEmpty = -6,
    NoAgent = -7,
    Busy = -8,
    Timeout = -9,
}

impl fmt::Display for AbiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AbiError::Ok => write!(f, "ok"),
            AbiError::InvalidArg => write!(f, "invalid_arg"),
            AbiError::NotFound => write!(f, "not_found"),
            AbiError::CapDenied => write!(f, "cap_denied"),
            AbiError::TierDenied => write!(f, "tier_denied"),
            AbiError::QueueFull => write!(f, "queue_full"),
            AbiError::QueueEmpty => write!(f, "queue_empty"),
            AbiError::NoAgent => write!(f, "no_agent"),
            AbiError::Busy => write!(f, "busy"),
            AbiError::Timeout => write!(f, "timeout"),
        }
    }
}

impl std::error::Error for AbiError {}

pub const SPAWN_RIGHTS_MAX: usize = 8;

#[derive(Debug, Clone)]
pub struct SpawnSpec {
    pub tier: Tier,
    pub priority: u8,
    pub attention_budget: u32,
    pub rights_len: u8,
    pub rights: [Right; SPAWN_RIGHTS_MAX],
}

impl SpawnSpec {
    pub fn new(tier: Tier, priority: u8, attention_budget: u32) -> Self {
        Self {
            tier,
            priority,
            attention_budget,
            rights_len: 0,
            rights: [Right::empty(); SPAWN_RIGHTS_MAX],
        }
    }

    pub fn with_rights(mut self, rights: &[Right]) -> Self {
        let len = rights.len().min(SPAWN_RIGHTS_MAX);
        self.rights[..len].copy_from_slice(&rights[..len]);
        self.rights_len = len as u8;
        self
    }

    pub fn rights(&self) -> &[Right] {
        &self.rights[..self.rights_len as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn right_union_combines_bits() {
        let r1 = Right::INFER;
        let r2 = Right::SPAWN_AGENT;
        let combined = r1.union(r2);
        assert!(combined.contains(Right::INFER));
        assert!(combined.contains(Right::SPAWN_AGENT));
        assert!(!combined.contains(Right::SECRET_USE));
    }

    #[test]
    fn right_root_contains_all_rights() {
        assert!(Right::ROOT.contains(Right::INFER));
        assert!(Right::ROOT.contains(Right::SPAWN_AGENT));
        assert!(Right::ROOT.contains(Right::SECRET_USE));
        assert!(Right::ROOT.contains(Right::SEND));
        assert!(Right::ROOT.contains(Right::RECV));
        assert!(Right::ROOT.contains(Right::SERIAL_WRITE));
        assert!(Right::ROOT.contains(Right::SCHED_YIELD));
    }

    #[test]
    fn tier_delegation_rules() {
        assert!(Tier::can_delegate(Tier::Director, Tier::Director));
        assert!(Tier::can_delegate(Tier::Director, Tier::Worker));
        assert!(Tier::can_delegate(Tier::Director, Tier::Ephemeral));

        assert!(!Tier::can_delegate(Tier::Worker, Tier::Director));
        assert!(Tier::can_delegate(Tier::Worker, Tier::Worker));
        assert!(Tier::can_delegate(Tier::Worker, Tier::Ephemeral));

        assert!(!Tier::can_delegate(Tier::Ephemeral, Tier::Director));
        assert!(!Tier::can_delegate(Tier::Ephemeral, Tier::Worker));
        assert!(Tier::can_delegate(Tier::Ephemeral, Tier::Ephemeral));
    }

    #[test]
    fn spawn_spec_builder() {
        let spec = SpawnSpec::new(Tier::Worker, 5, 1000)
            .with_rights(&[Right::INFER, Right::SEND]);

        assert_eq!(spec.tier, Tier::Worker);
        assert_eq!(spec.priority, 5);
        assert_eq!(spec.attention_budget, 1000);
        assert_eq!(spec.rights().len(), 2);
        assert!(spec.rights()[0].contains(Right::INFER));
        assert!(spec.rights()[1].contains(Right::SEND));
    }

    #[test]
    fn spawn_spec_truncates_rights() {
        let many_rights = vec![Right::INFER; 20];
        let spec = SpawnSpec::new(Tier::Worker, 5, 1000).with_rights(&many_rights);
        assert_eq!(spec.rights().len(), SPAWN_RIGHTS_MAX);
    }
}
