use soroban_sdk::{contracttype, Address};

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Admin address — manages the reporter allow-list. Instance storage.
    Admin,
    /// Contracts allowed to report outcomes. Persistent storage.
    Reporter(Address),
    /// Legacy member reputation (v1, no timestamps). Persistent storage.
    Rep(Address),
    /// Member reputation with timestamps (v2). Persistent storage.
    RepV2(Address),
}

/// Legacy record shape, kept for in-place migration when `get_reputation` reads
/// a `Rep` key that has not yet been upgraded.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReputationV1 {
    pub completed: u32,
    pub defaulted: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Reputation {
    pub completed: u32,
    pub defaulted: u32,
    /// Unix timestamp (seconds) of the most recent completion, or `None`.
    pub last_completed_at: Option<u64>,
    /// Unix timestamp (seconds) of the most recent default, or `None`.
    pub last_defaulted_at: Option<u64>,
}

impl From<ReputationV1> for Reputation {
    fn from(v1: ReputationV1) -> Self {
        Reputation {
            completed: v1.completed,
            defaulted: v1.defaulted,
            last_completed_at: None,
            last_defaulted_at: None,
        }
    }
}
