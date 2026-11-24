#![allow(deprecated)]
//! Event emitters. Topics are `(symbol "rep", symbol "<action>")` — the shape
//! the app repo's indexer decodes.
//!
//! See the note in the circle contract's events module on why `publish` is
//! still used under soroban-sdk 28.

use soroban_sdk::{symbol_short, Address, Env};

/// Emitted once when the contract is initialized, so monitors can see which
/// admin took control and detect front-running.
pub fn init(env: &Env, admin: &Address) {
    env.events()
        .publish((symbol_short!("rep"), symbol_short!("init")), admin.clone());
}

/// Emitted whenever a reporter is allowed or revoked. Security-relevant so
/// monitors can detect unauthorized reporter changes.
pub fn reporter_set(env: &Env, reporter: &Address, allowed: bool) {
    env.events().publish(
        (symbol_short!("rep"), symbol_short!("reporter")),
        (reporter.clone(), allowed),
    );
}

pub fn completion(env: &Env, member: &Address, context: u64) {
    env.events().publish(
        (symbol_short!("rep"), symbol_short!("complete")),
        (member.clone(), context),
    );
}

pub fn defaulted(env: &Env, member: &Address, context: u64) {
    env.events().publish(
        (symbol_short!("rep"), symbol_short!("default")),
        (member.clone(), context),
    );
}
