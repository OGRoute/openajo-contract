use soroban_sdk::{contracttype, Address};

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Admin address — manages the reporter allow-list. Instance storage.
    Admin,
    /// Contracts allowed to report outcomes. Instance storage.
    Reporter(Address),
    /// A member's permanent cross-circle history. Persistent storage.
    Rep(Address),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Reputation {
    /// Circles finished cleanly (deposit intact through completion).
    pub completed: u32,
    /// Circles defaulted on (deposit could not cover a missed contribution).
    pub defaulted: u32,
}
