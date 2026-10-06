#![allow(deprecated)]
//! Event emitters.
//!
//! Event shapes are the API contract for the app repo's indexer: topics are
//! `(symbol "circle", symbol "<action>")` and the data is a tuple. Do not
//! change topics or data tuples without coordinating there.
//!
//! soroban-sdk 28 deprecates `Events::publish` in favour of the
//! `#[contractevent]` macro, which emits a different wire format — the event
//! name becomes the first topic and the data becomes a struct. Adopting it is a
//! breaking change for every consumer, so it ships as its own coordinated
//! change (openajo-contract#12 / openajo-app#22) rather than riding along with
//! an SDK bump. The deprecated call is deliberate until then.

use soroban_sdk::{symbol_short, Address, Env};

pub fn create(env: &Env, id: u32, creator: &Address) {
    env.events().publish(
        (symbol_short!("circle"), symbol_short!("create")),
        (id, creator.clone()),
    );
}

pub fn join(env: &Env, id: u32, member: &Address) {
    env.events().publish(
        (symbol_short!("circle"), symbol_short!("join")),
        (id, member.clone()),
    );
}

pub fn start(env: &Env, id: u32) {
    env.events()
        .publish((symbol_short!("circle"), symbol_short!("start")), id);
}

pub fn contrib(env: &Env, id: u32, member: &Address, cycle: u32) {
    env.events().publish(
        (symbol_short!("circle"), symbol_short!("contrib")),
        (id, member.clone(), cycle),
    );
}

pub fn slash(env: &Env, id: u32, member: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("circle"), symbol_short!("slash")),
        (id, member.clone(), amount),
    );
}

pub fn defaulted(env: &Env, id: u32, member: &Address) {
    env.events().publish(
        (symbol_short!("circle"), symbol_short!("default")),
        (id, member.clone()),
    );
}

pub fn payout(env: &Env, id: u32, recipient: &Address, amount: i128, cycle: u32) {
    env.events().publish(
        (symbol_short!("circle"), symbol_short!("payout")),
        (id, recipient.clone(), amount, cycle),
    );
}

pub fn complete(env: &Env, id: u32) {
    env.events()
        .publish((symbol_short!("circle"), symbol_short!("complete")), id);
}

pub fn cancel(env: &Env, id: u32) {
    env.events()
        .publish((symbol_short!("circle"), symbol_short!("cancel")), id);
}
