#![cfg(test)]
//! Shared fixtures for the circle test modules.
//!
//! Each test module imports from here instead of reaching into another
//! test module.

use crate::{CircleContract, CircleContractClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{StellarAssetClient, TokenClient},
    Address, Env,
};

pub(crate) const CONTRIB: i128 = 100;
pub(crate) const DEPOSIT: i128 = 150;
pub(crate) const PERIOD: u64 = 3600;

pub(crate) struct Fixture<'a> {
    pub(crate) env: Env,
    pub(crate) client: CircleContractClient<'a>,
    pub(crate) rep: reputation::ReputationContractClient<'a>,
    pub(crate) token: TokenClient<'a>,
    pub(crate) members: [Address; 3],
}

/// Registers reputation + circle + a SAC token, wires the reporter, funds
/// three members with 1_000 units each.
pub(crate) fn setup<'a>() -> Fixture<'a> {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);

    let rep_id = env.register(reputation::ReputationContract, ());
    let rep = reputation::ReputationContractClient::new(&env, &rep_id);
    rep.initialize(&admin);

    let circle_id = env.register(CircleContract, ());
    let client = CircleContractClient::new(&env, &circle_id);
    client.initialize(&admin, &rep_id);
    rep.set_reporter(&admin, &circle_id, &true);

    let sac = env.register_stellar_asset_contract_v2(admin.clone());
    let token = TokenClient::new(&env, &sac.address());
    let mint = StellarAssetClient::new(&env, &sac.address());

    let members = [
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env),
    ];
    for m in members.iter() {
        mint.mint(m, &1_000);
    }

    Fixture {
        env,
        client,
        rep,
        token,
        members,
    }
}

pub(crate) fn create_full_circle(f: &Fixture) -> u32 {
    let [a, b, c] = &f.members;
    let id = f.client.create_circle(
        a,
        &f.token.address,
        &CONTRIB,
        &DEPOSIT,
        &3,
        &PERIOD,
        &None,
        &0u64,
    );
    f.client.join(&id, b);
    f.client.join(&id, c);
    id
}

pub(crate) fn advance_past_deadline(f: &Fixture, id: u32) {
    let deadline = f.client.cycle_deadline(&id);
    f.env.ledger().with_mut(|l| l.timestamp = deadline + 1);
}

/// A fresh address funded with 1_000 units of the fixture token, for
/// scenarios needing more than the fixture's three members.
pub(crate) fn funded_member(f: &Fixture) -> Address {
    let m = Address::generate(&f.env);
    StellarAssetClient::new(&f.env, &f.token.address).mint(&m, &1_000);
    m
}

// ---- Happy path -----------------------------------------------------------

/// A fixture with a caller-chosen number of members, for generated scenarios.
pub(crate) struct SizedFixture<'a> {
    pub(crate) env: Env,
    pub(crate) client: CircleContractClient<'a>,
    pub(crate) token: TokenClient<'a>,
    pub(crate) members: std::vec::Vec<Address>,
}

/// Like [`setup`], but with `n` members each funded with `balance` units.
pub(crate) fn setup_sized<'a>(n: usize, balance: i128) -> SizedFixture<'a> {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);

    let rep_id = env.register(reputation::ReputationContract, ());
    let rep = reputation::ReputationContractClient::new(&env, &rep_id);
    rep.initialize(&admin);

    let circle_id = env.register(CircleContract, ());
    let client = CircleContractClient::new(&env, &circle_id);
    client.initialize(&admin, &rep_id);
    rep.set_reporter(&admin, &circle_id, &true);

    let sac = env.register_stellar_asset_contract_v2(admin.clone());
    let token = TokenClient::new(&env, &sac.address());
    let mint = StellarAssetClient::new(&env, &sac.address());

    let members: std::vec::Vec<Address> = (0..n).map(|_| Address::generate(&env)).collect();
    for m in members.iter() {
        mint.mint(m, &balance);
    }

    SizedFixture {
        env,
        client,
        token,
        members,
    }
}

/// Moves the ledger just past the current cycle deadline of a sized fixture.
pub(crate) fn advance_past_deadline_sized(f: &SizedFixture, id: u32) {
    let deadline = f.client.cycle_deadline(&id);
    f.env.ledger().with_mut(|l| l.timestamp = deadline + 1);
}
