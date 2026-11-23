#![no_std]
//! Shared TTL constants and storage helpers for OpenAjo contracts.

use soroban_sdk::{Env, IntoVal, Val};

/// ~15 days at 5s ledgers.
pub const TTL_THRESHOLD: u32 = 259_200;
/// ~30 days at 5s ledgers.
pub const TTL_EXTEND: u32 = 518_400;

/// ~150 days at 5s ledgers. Used for records that must outlive the activity
/// that produced them (reputation), not for live workflow state.
pub const LONG_TTL_THRESHOLD: u32 = 2_592_000;
/// ~175 days at 5s ledgers — just under the network's ~180 day maximum entry
/// TTL, so the extension is accepted on every public network.
pub const LONG_TTL_EXTEND: u32 = 3_024_000;

/// Extend the instance storage TTL.
pub fn extend_instance(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(TTL_THRESHOLD, TTL_EXTEND);
}

/// Extend the persistent storage TTL for a given key.
pub fn extend_persistent<K: IntoVal<Env, Val>>(env: &Env, key: &K) {
    env.storage()
        .persistent()
        .extend_ttl(key, TTL_THRESHOLD, TTL_EXTEND);
}

/// Extend a persistent entry that must survive long idle periods to the long
/// (reputation) TTL window.
pub fn extend_persistent_long<K: IntoVal<Env, Val>>(env: &Env, key: &K) {
    env.storage()
        .persistent()
        .extend_ttl(key, LONG_TTL_THRESHOLD, LONG_TTL_EXTEND);
}
