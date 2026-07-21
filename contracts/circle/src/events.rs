use soroban_sdk::{symbol_short, Address, Env};

// Event shapes are the API contract for the app repo's indexer.
// Do not change topics or data tuples without coordinating there.

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
