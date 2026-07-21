use soroban_sdk::{contracttype, Address};

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// GlobalConfig — instance storage.
    Config,
    /// u32 total circles ever created — instance storage.
    Count,
    /// Circle by id — persistent storage.
    Circle(u32),
    /// Vec<Address> members in join order — persistent storage.
    Members(u32),
    /// MemberState by (circle, address) — persistent storage.
    Member(u32, Address),
    /// bool contributed for (circle, cycle, address) — temporary storage.
    Paid(u32, u32, Address),
}

#[contracttype]
#[derive(Clone)]
pub struct GlobalConfig {
    pub admin: Address,
    pub reputation: Address,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CircleStatus {
    Open,
    Active,
    Completed,
    Cancelled,
}

#[contracttype]
#[derive(Clone)]
pub struct Circle {
    pub creator: Address,
    /// SAC token address (USDC in production).
    pub token: Address,
    /// Per member per cycle, raw token units. > 0.
    pub contribution: i128,
    /// Flat security bond escrowed at join. >= 0. Deliberately does NOT fully
    /// collateralize a member — it prices default; reputation makes repeat
    /// defaulting visible. Do not "fix" by requiring deposit >= size*contribution.
    pub deposit: i128,
    /// Number of members required to activate. >= 2.
    pub size: u32,
    /// Seconds per cycle. >= 3600.
    pub period_secs: u64,
    pub status: CircleStatus,
    /// Ledger timestamp at activation; 0 while Open.
    pub started_at: u64,
    /// 0-based.
    pub current_cycle: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemberState {
    pub deposit_remaining: i128,
    pub received: bool,
    pub defaulted: bool,
}
