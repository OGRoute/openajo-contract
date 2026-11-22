#![no_main]
//! Drives arbitrary sequences of `circle` entry-point calls and checks:
//!
//! 1. every failure is an expected contract `Error` (never a host panic, an
//!    arithmetic overflow or an unexpected trap), and
//! 2. value is conserved: member balances plus contract escrow always equal
//!    what was minted, and a terminal circle leaves the contract empty.

use arbitrary::Arbitrary;
use circle::{CircleContract, CircleContractClient};
use libfuzzer_sys::fuzz_target;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{StellarAssetClient, TokenClient},
    Address, Env,
};

const ACTORS: usize = 4;
const MINT: i128 = 1_000_000;

#[derive(Arbitrary, Debug)]
struct Params {
    contribution: u16,
    deposit: u16,
    size: u8,
    period_secs: u16,
}

#[derive(Arbitrary, Debug)]
enum Op {
    Join(u8),
    Leave(u8),
    Cancel(u8),
    Contribute(u8),
    Settle,
    /// Move the ledger past the current cycle deadline.
    PastDeadline,
    /// Advance the ledger by a small amount without crossing a deadline.
    Tick(u8),
}

#[derive(Arbitrary, Debug)]
struct Input {
    params: Params,
    ops: Vec<Op>,
}

/// Accepts success or a declared contract `Error`; anything else is a bug.
macro_rules! expect_ok_or_contract_error {
    ($call:expr) => {
        match $call {
            Ok(Ok(_)) | Err(Ok(_)) => {}
            other => panic!("unexpected failure (not a contract error): {:?}", other),
        }
    };
}

fuzz_target!(|input: Input| {
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
    let actors: Vec<Address> = (0..ACTORS).map(|_| Address::generate(&env)).collect();
    for a in actors.iter() {
        mint.mint(a, &MINT);
    }
    let minted = MINT * ACTORS as i128;

    let p = &input.params;
    let created = client.try_create_circle(
        &actors[0],
        &token.address,
        &(p.contribution as i128),
        &(p.deposit as i128),
        &(p.size as u32),
        &(p.period_secs as u64),
    );
    let id = match created {
        Ok(Ok(id)) => id,
        Err(Ok(_)) => return, // rejected parameters are an expected error
        other => panic!("unexpected create_circle failure: {:?}", other),
    };

    let who = |i: u8| &actors[i as usize % ACTORS];
    for op in input.ops.iter() {
        match op {
            Op::Join(i) => expect_ok_or_contract_error!(client.try_join(&id, who(*i))),
            Op::Leave(i) => expect_ok_or_contract_error!(client.try_leave(&id, who(*i))),
            Op::Cancel(i) => expect_ok_or_contract_error!(client.try_cancel(&id, who(*i))),
            Op::Contribute(i) => {
                expect_ok_or_contract_error!(client.try_contribute(&id, who(*i)))
            }
            Op::Settle => expect_ok_or_contract_error!(client.try_settle_cycle(&id)),
            Op::PastDeadline => {
                let deadline = client.cycle_deadline(&id);
                env.ledger().with_mut(|l| l.timestamp = deadline + 1);
            }
            Op::Tick(s) => {
                env.ledger().with_mut(|l| l.timestamp += *s as u64);
            }
        }

        let held: i128 = actors.iter().map(|a| token.balance(a)).sum();
        let escrow = token.balance(&client.address);
        assert_eq!(held + escrow, minted, "supply drifted after {:?}", op);

        // CircleStatus is crate-private, so compare through its Debug form.
        let status = format!("{:?}", client.get_circle(&id).status);
        if status == "Completed" || status == "Cancelled" {
            assert_eq!(escrow, 0, "terminal circle strands escrow after {:?}", op);
        }
    }
});
