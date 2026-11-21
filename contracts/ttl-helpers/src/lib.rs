#![no_std]
//! Shared TTL constants and storage helpers for OpenAjo contracts.

use soroban_sdk::{Env, IntoVal, Val};

/// ~15 days at 5s ledgers.
pub const TTL_THRESHOLD: u32 = 259_200;
/// ~30 days at 5s ledgers.
pub const TTL_EXTEND: u32 = 518_400;

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
