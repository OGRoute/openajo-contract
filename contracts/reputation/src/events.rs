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
