//! Storage helpers for the circle contract.

use soroban_sdk::{Env, Vec};
use ttl_helpers::extend_persistent;

use crate::errors::Error;
use crate::types::{Circle, DataKey, GlobalConfig, MemberState};

/// Read the global config from instance storage.
pub fn read_config(env: &Env) -> GlobalConfig {
    env.storage()
        .instance()
        .get(&DataKey::Config)
        .unwrap_or_else(|| panic_with_error!(env, Error::NotInitialized))
}

/// Read a circle from persistent storage.
pub fn read_circle(env: &Env, id: u32) -> Circle {
    env.storage()
        .persistent()
        .get(&DataKey::Circle(id))
        .unwrap_or_else(|| panic_with_error!(env, Error::NotFound))
}

/// Write a circle to persistent storage and extend its TTL.
pub fn write_circle(env: &Env, id: u32, circle: &Circle) {
    let key = DataKey::Circle(id);
    env.storage().persistent().set(&key, circle);
    extend_persistent(env, &key);
}

/// Read the member list for a circle.
pub fn read_members(env: &Env, id: u32) -> Vec<Address> {
    env.storage()
        .persistent()
        .get(&DataKey::Members(id))
        .unwrap_or_else(|| panic_with_error!(env, Error::NotFound))
}

/// Write the member list for a circle and extend its TTL.
pub fn write_members(env: &Env, id: u32, members: &Vec<Address>) {
    let key = DataKey::Members(id);
    env.storage().persistent().set(&key, members);
    extend_persistent(env, &key);
}

/// Read a member's state.
pub fn read_member(env: &Env, id: u32, member: &Address) -> MemberState {
    env.storage()
        .persistent()
        .get(&DataKey::Member(id, member.clone()))
        .unwrap_or_else(|| panic_with_error!(env, Error::NotMember))
}

/// Write a member's state and extend its TTL.
pub fn write_member(env: &Env, id: u32, member: &Address, state: &MemberState) {
    let key = DataKey::Member(id, member.clone());
    env.storage().persistent().set(&key, state);
    extend_persistent(env, &key);
}

/// Check if a member has paid for a given cycle.
pub fn is_paid(env: &Env, id: u32, cycle: u32, member: &Address) -> bool {
    env.storage()
        .temporary()
        .get(&DataKey::Paid(id, cycle, member.clone()))
        .unwrap_or(false)
}

/// Read the custom payout order for a circle, if one has been set.
pub fn read_payout_order(env: &Env, id: u32) -> Option<Vec<Address>> {
    env.storage().persistent().get(&DataKey::PayoutOrder(id))
}

/// Write a custom payout order and extend its TTL.
pub fn write_payout_order(env: &Env, id: u32, order: &Vec<Address>) {
    let key = DataKey::PayoutOrder(id);
    env.storage().persistent().set(&key, order);
    extend_persistent(env, &key);
}

use soroban_sdk::{panic_with_error, Address};
