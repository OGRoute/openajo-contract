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

use soroban_sdk::{symbol_short, Address, Env, Vec};

/// Emitted once when the contract is initialized, with the admin that took
/// control and the reputation contract it reports to.
pub fn init(env: &Env, admin: &Address, reputation: &Address) {
    env.events().publish(
        (symbol_short!("circle"), symbol_short!("init")),
        (admin.clone(), reputation.clone()),
    );
}

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

/// Emitted when the creator removes a member from an `Open` circle.
pub fn remove(env: &Env, id: u32, member: &Address) {
    env.events().publish(
        (symbol_short!("circle"), symbol_short!("remove")),
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

/// Emitted once per settled cycle with a summary: who paid, how much was
/// slashed in total (this cycle only), and who received the payout (None in
/// the edge-case where every remaining member defaulted this cycle).
pub fn settle(
    env: &Env,
    id: u32,
    cycle: u32,
    payers: &Vec<Address>,
    slashed_total: i128,
    recipient: Option<Address>,
) {
    env.events().publish(
        (symbol_short!("circle"), symbol_short!("settle")),
        (id, cycle, payers.clone(), slashed_total, recipient),
    );
}
