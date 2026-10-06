#![allow(deprecated)]
//! Event emitters. Topics are `(symbol "rep", symbol "<outcome>")` with the
//! member address as data — the shape the app repo's indexer decodes.
//!
//! See the note in the circle contract's events module on why `publish` is
//! still used under soroban-sdk 28.

use soroban_sdk::{symbol_short, Address, Env};

pub fn completion(env: &Env, member: &Address) {
    env.events().publish(
        (symbol_short!("rep"), symbol_short!("complete")),
        member.clone(),
    );
}

pub fn defaulted(env: &Env, member: &Address) {
    env.events().publish(
        (symbol_short!("rep"), symbol_short!("default")),
        member.clone(),
    );
}
