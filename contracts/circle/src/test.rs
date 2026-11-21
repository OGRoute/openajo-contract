#![cfg(test)]

extern crate std;

use crate::{errors::Error, types::CircleStatus, CircleContract, CircleContractClient};
use soroban_sdk::{
    testutils::{Address as _, AuthorizedFunction, AuthorizedInvocation, Ledger},
    token::{StellarAssetClient, TokenClient},
    Address, Env, IntoVal, Symbol, Val, Vec,
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
    let id = f
        .client
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &3, &PERIOD);
    f.client.join(&id, b);
    f.client.join(&id, c);
    id
}

pub(crate) fn advance_past_deadline(f: &Fixture, id: u32) {
    let deadline = f.client.cycle_deadline(&id);
    f.env.ledger().with_mut(|l| l.timestamp = deadline + 1);
}

// ---- Happy path -----------------------------------------------------------

#[test]
fn full_lifecycle_3_members_3_cycles() {
    let f = setup();
    let [a, _b, _c] = &f.members;
    let id = create_full_circle(&f);

    assert_eq!(f.client.get_circle(&id).status, CircleStatus::Active);
    // Deposits escrowed.
    assert_eq!(f.token.balance(a), 1_000 - DEPOSIT);

    for cycle in 0..3u32 {
        for m in f.members.iter() {
            f.client.contribute(&id, m);
        }
        f.client.settle_cycle(&id);
        let recipient = &f.members[cycle as usize]; // join order
        assert!(f.client.get_member(&id, recipient).received);
    }

    let circle = f.client.get_circle(&id);
    assert_eq!(circle.status, CircleStatus::Completed);
    // Everyone paid 3x100 in, received a 300 pot once, and got the deposit
    // back: net zero.
    for m in f.members.iter() {
        assert_eq!(f.token.balance(m), 1_000);
        assert_eq!(f.rep.get_reputation(m).completed, 1);
        assert_eq!(f.rep.get_reputation(m).defaulted, 0);
    }
    // Contract holds nothing.
    assert_eq!(f.token.balance(&f.client.address), 0);
}

#[test]
fn completion_records_ended_at() {
    let f = setup();
    f.env.ledger().with_mut(|l| l.timestamp = 10_000);
    let id = create_full_circle(&f);
    let circle = f.client.get_circle(&id);
    assert_eq!(circle.started_at, 10_000);
    assert_eq!(circle.ended_at, 0);

    for _cycle in 0..2u32 {
        for m in f.members.iter() {
            f.client.contribute(&id, m);
        }
        f.client.settle_cycle(&id);
        assert_eq!(f.client.get_circle(&id).ended_at, 0);
    }

    // The final settlement happens before the last deadline.
    let settled_at = 10_000 + 2 * PERIOD + 7;
    f.env.ledger().with_mut(|l| l.timestamp = settled_at);
    for m in f.members.iter() {
        f.client.contribute(&id, m);
    }
    f.client.settle_cycle(&id);

    let circle = f.client.get_circle(&id);
    assert_eq!(circle.status, CircleStatus::Completed);
    assert_eq!(circle.ended_at, settled_at);
    for m in f.members.iter() {
        assert_eq!(f.token.balance(m), 1_000);
    }
    assert_eq!(f.token.balance(&f.client.address), 0);
}

// ---- Slashing and defaults ------------------------------------------------

#[test]
fn deadline_settlement_slashes_and_covers() {
    let f = setup();
    let [a, b, c] = &f.members;
    let id = create_full_circle(&f);

    // c skips cycle 0; deposit 150 covers the 100 slash.
    f.client.contribute(&id, a);
    f.client.contribute(&id, b);
    advance_past_deadline(&f, id);
    f.client.settle_cycle(&id);

    let st = f.client.get_member(&id, c);
    assert_eq!(st.deposit_remaining, DEPOSIT - CONTRIB); // 50 left
    assert!(!st.defaulted);
    // Recipient a still got the full 300 pot.
    assert_eq!(f.token.balance(a), 1_000 - DEPOSIT - CONTRIB + 3 * CONTRIB);
}

#[test]
fn insufficient_deposit_marks_default_and_skips_rotation() {
    let f = setup();
    let [a, b, c] = &f.members;
    let id = create_full_circle(&f);

    // Cycle 0: c misses, slashed 100 of 150.
    f.client.contribute(&id, a);
    f.client.contribute(&id, b);
    advance_past_deadline(&f, id);
    f.client.settle_cycle(&id); // a receives

    // Cycle 1: c misses again; only 50 left < 100 -> default.
    f.client.contribute(&id, a);
    f.client.contribute(&id, b);
    advance_past_deadline(&f, id);
    f.client.settle_cycle(&id); // b receives 100+100+50 = 250

    let st = f.client.get_member(&id, c);
    assert!(st.defaulted);
    assert_eq!(st.deposit_remaining, 0);
    assert_eq!(f.rep.get_reputation(c).defaulted, 1);

    // With a and b both paid, the circle completed and c never received.
    let circle = f.client.get_circle(&id);
    assert_eq!(circle.status, CircleStatus::Completed);
    assert!(!st.received);

    // Exact balances: a: -150 -200 +300 +150 = +100; b: -150 -200 +250 +150 = +50;
    // c: -150. Sums to zero; contract empty.
    assert_eq!(f.token.balance(a), 1_100);
    assert_eq!(f.token.balance(b), 1_050);
    assert_eq!(f.token.balance(c), 850);
    assert_eq!(f.token.balance(&f.client.address), 0);
    // a and b completed cleanly.
    assert_eq!(f.rep.get_reputation(a).completed, 1);
    assert_eq!(f.rep.get_reputation(b).completed, 1);
    assert_eq!(f.rep.get_reputation(c).completed, 0);
}

#[test]
fn slashed_total_accumulates_across_cycles() {
    let f = setup();
    let [a, b, c] = &f.members;
    let id = create_full_circle(&f);
    assert_eq!(f.client.get_circle(&id).slashed_total, 0);

    // Cycle 0: everyone pays, nothing slashed; a receives 300.
    for m in f.members.iter() {
        f.client.contribute(&id, m);
    }
    f.client.settle_cycle(&id);
    assert_eq!(f.client.get_circle(&id).slashed_total, 0);

    // Cycle 1: c misses and is slashed 100 of 150; b receives 300.
    f.client.contribute(&id, a);
    f.client.contribute(&id, b);
    advance_past_deadline(&f, id);
    f.client.settle_cycle(&id);
    assert_eq!(f.client.get_circle(&id).slashed_total, CONTRIB);

    // Cycle 2: c misses again; the last 50 is slashed and c defaults. a and
    // b have both received, so the 250 pot goes back to them, 125 each.
    f.client.contribute(&id, a);
    f.client.contribute(&id, b);
    advance_past_deadline(&f, id);
    f.client.settle_cycle(&id);

    let circle = f.client.get_circle(&id);
    assert_eq!(circle.status, CircleStatus::Completed);
    assert_eq!(circle.slashed_total, DEPOSIT);
    assert_eq!(f.client.get_member(&id, c).deposit_remaining, 0);

    // Exact balances: a: -150 -300 +300 +125 +150 = +125;
    // b: -150 -300 +300 +125 +150 = +125; c: -150 -100 = -250.
    assert_eq!(f.token.balance(a), 1_125);
    assert_eq!(f.token.balance(b), 1_125);
    assert_eq!(f.token.balance(c), 750);
    assert_eq!(f.token.balance(&f.client.address), 0);
}

#[test]
fn recipient_defaults_after_receiving_loses_deposit() {
    let f = setup();
    let [a, b, c] = &f.members;
    let id = create_full_circle(&f);

    // Cycle 0: all pay; a receives.
    for m in f.members.iter() {
        f.client.contribute(&id, m);
    }
    f.client.settle_cycle(&id);
    assert!(f.client.get_member(&id, a).received);

    // Cycles 1 and 2: a stops paying after collecting.
    // Cycle 1: a slashed 100 (deposit 150 -> 50), b receives.
    f.client.contribute(&id, b);
    f.client.contribute(&id, c);
    advance_past_deadline(&f, id);
    f.client.settle_cycle(&id);
    assert!(!f.client.get_member(&id, a).defaulted);

    // Cycle 2: a slashed 50 < 100 -> defaulted; c receives.
    f.client.contribute(&id, b);
    f.client.contribute(&id, c);
    advance_past_deadline(&f, id);
    f.client.settle_cycle(&id);

    let st_a = f.client.get_member(&id, a);
    assert!(st_a.defaulted);
    assert_eq!(st_a.deposit_remaining, 0);
    assert_eq!(f.rep.get_reputation(a).defaulted, 1);
    assert_eq!(f.client.get_circle(&id).status, CircleStatus::Completed);
}

// ---- Open-phase flows -----------------------------------------------------

#[test]
fn leave_refunds_and_cancel_refunds_all() {
    let f = setup();
    let [a, b, c] = &f.members;
    let id = f
        .client
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &3, &PERIOD);
    f.client.join(&id, b);

    f.client.leave(&id, b);
    assert_eq!(f.token.balance(b), 1_000);
    assert_eq!(f.client.get_members(&id).len(), 1);

    f.client.join(&id, c);
    f.client.cancel(&id, a);
    assert_eq!(f.client.get_circle(&id).status, CircleStatus::Cancelled);
    assert_eq!(f.token.balance(a), 1_000);
    assert_eq!(f.token.balance(c), 1_000);
    assert_eq!(f.token.balance(&f.client.address), 0);
}

#[test]
fn cancel_records_ended_at() {
    let f = setup();
    let [a, b, _] = &f.members;
    f.env.ledger().with_mut(|l| l.timestamp = 5_000);
    let id = f
        .client
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &3, &PERIOD);
    f.client.join(&id, b);
    assert_eq!(f.client.get_circle(&id).ended_at, 0);

    f.env.ledger().with_mut(|l| l.timestamp = 6_000);
    f.client.cancel(&id, a);

    let circle = f.client.get_circle(&id);
    assert_eq!(circle.status, CircleStatus::Cancelled);
    assert_eq!(circle.started_at, 0);
    assert_eq!(circle.ended_at, 6_000);
    assert_eq!(f.token.balance(a), 1_000);
    assert_eq!(f.token.balance(b), 1_000);
    assert_eq!(f.token.balance(&f.client.address), 0);
}

#[test]
fn cancel_zeroes_refunded_deposits() {
    let f = setup();
    let [a, b, c] = &f.members;
    let id = f
        .client
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &3, &PERIOD);
    f.client.join(&id, b);
    assert_eq!(f.client.get_member(&id, a).deposit_remaining, DEPOSIT);
    assert_eq!(f.client.get_member(&id, b).deposit_remaining, DEPOSIT);
    assert_eq!(f.token.balance(&f.client.address), 2 * DEPOSIT);

    f.client.cancel(&id, a);

    // Every refunded member's record now shows nothing left in escrow.
    for m in [a, b] {
        let st = f.client.get_member(&id, m);
        assert_eq!(st.deposit_remaining, 0);
        assert!(!st.received);
        assert!(!st.defaulted);
        assert_eq!(f.token.balance(m), 1_000);
    }
    assert_eq!(f.token.balance(c), 1_000);
    assert_eq!(f.token.balance(&f.client.address), 0);
}

// ---- Error paths ----------------------------------------------------------

#[test]
#[should_panic]
fn double_join_panics() {
    let f = setup();
    let [a, b, _] = &f.members;
    let id = f
        .client
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &3, &PERIOD);
    f.client.join(&id, b);
    f.client.join(&id, b);
}

#[test]
#[should_panic]
fn double_contribute_panics() {
    let f = setup();
    let id = create_full_circle(&f);
    let a = &f.members[0];
    f.client.contribute(&id, a);
    f.client.contribute(&id, a);
}

#[test]
#[should_panic]
fn contribute_after_default_panics() {
    let f = setup();
    let [a, b, c] = &f.members;
    let id = create_full_circle(&f);
    // Default c over two missed cycles.
    for _ in 0..2 {
        f.client.contribute(&id, a);
        f.client.contribute(&id, b);
        advance_past_deadline(&f, id);
        f.client.settle_cycle(&id);
    }
    f.client.contribute(&id, c);
}

#[test]
#[should_panic]
fn settle_before_due_panics() {
    let f = setup();
    let id = create_full_circle(&f);
    f.client.contribute(&id, &f.members[0]);
    // One member paid, deadline not reached.
    f.client.settle_cycle(&id);
}

#[test]
#[should_panic]
fn non_creator_cancel_panics() {
    let f = setup();
    let [a, b, _] = &f.members;
    let id = f
        .client
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &3, &PERIOD);
    f.client.join(&id, b);
    f.client.cancel(&id, b);
}

#[test]
#[should_panic]
fn creator_leave_panics() {
    let f = setup();
    let a = &f.members[0];
    let id = f
        .client
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &3, &PERIOD);
    f.client.leave(&id, a);
}

#[test]
#[should_panic]
fn create_bad_params_panics() {
    let f = setup();
    let a = &f.members[0];
    // size 1 is invalid.
    f.client
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &1, &PERIOD);
}

#[test]
fn zero_deposit_create_and_join_need_no_balance() {
    let f = setup();
    let a = Address::generate(&f.env);
    let b = Address::generate(&f.env);
    let id = f
        .client
        .create_circle(&a, &f.token.address, &CONTRIB, &0, &2, &PERIOD);
    f.client.join(&id, &b);
    assert_eq!(f.token.balance(&a), 0);
    assert_eq!(f.token.balance(&b), 0);
    assert_eq!(f.token.balance(&f.client.address), 0);
    assert_eq!(f.client.get_circle(&id).status, CircleStatus::Active);
}

#[test]
#[should_panic]
fn create_zero_contribution_panics() {
    let f = setup();
    let a = &f.members[0];
    f.client
        .create_circle(a, &f.token.address, &0, &DEPOSIT, &3, &PERIOD);
}

#[test]
#[should_panic]
fn create_negative_deposit_panics() {
    let f = setup();
    let a = &f.members[0];
    f.client
        .create_circle(a, &f.token.address, &CONTRIB, &-1, &3, &PERIOD);
}

#[test]
#[should_panic]
fn create_size_one_panics() {
    let f = setup();
    let a = &f.members[0];
    f.client
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &1, &PERIOD);
}

#[test]
#[should_panic]
fn create_short_period_panics() {
    let f = setup();
    let a = &f.members[0];
    f.client
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &3, &3599);
}

#[test]
fn create_min_contribution_accepted() {
    let f = setup();
    let a = &f.members[0];
    let id = f
        .client
        .create_circle(a, &f.token.address, &1, &DEPOSIT, &3, &PERIOD);
    assert_eq!(f.client.get_circle(&id).contribution, 1);
}

#[test]
fn create_zero_deposit_accepted() {
    let f = setup();
    let a = &f.members[0];
    let id = f
        .client
        .create_circle(a, &f.token.address, &CONTRIB, &0, &3, &PERIOD);
    assert_eq!(f.client.get_circle(&id).deposit, 0);
    assert_eq!(f.token.balance(a), 1_000);
}

#[test]
fn create_min_size_accepted() {
    let f = setup();
    let a = &f.members[0];
    let id = f
        .client
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &2, &PERIOD);
    assert_eq!(f.client.get_circle(&id).size, 2);
}

#[test]
fn create_min_period_accepted() {
    let f = setup();
    let a = &f.members[0];
    let id = f
        .client
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &3, &3600);
    assert_eq!(f.client.get_circle(&id).period_secs, 3600);
}

#[test]
#[should_panic]
fn join_full_circle_panics() {
    let f = setup();
    let id = create_full_circle(&f); // Active now, so join is BadStatus
    let d = Address::generate(&f.env);
    f.client.join(&id, &d);
}

// ---- Status and membership guards -----------------------------------------

fn complete_circle(f: &Fixture, id: u32) {
    for _ in 0..3u32 {
        for m in f.members.iter() {
            f.client.contribute(&id, m);
        }
        f.client.settle_cycle(&id);
    }
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn join_active_circle_panics_bad_status() {
    let f = setup();
    let id = create_full_circle(&f);
    assert_eq!(f.client.get_circle(&id).status, CircleStatus::Active);
    let d = Address::generate(&f.env);
    f.client.join(&id, &d);
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn join_completed_circle_panics_bad_status() {
    let f = setup();
    let id = create_full_circle(&f);
    complete_circle(&f, id);
    assert_eq!(f.client.get_circle(&id).status, CircleStatus::Completed);
    let d = Address::generate(&f.env);
    f.client.join(&id, &d);
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn join_cancelled_circle_panics_bad_status() {
    let f = setup();
    let a = &f.members[0];
    let id = f
        .client
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &3, &PERIOD);
    f.client.cancel(&id, a);
    assert_eq!(f.client.get_circle(&id).status, CircleStatus::Cancelled);
    let d = Address::generate(&f.env);
    f.client.join(&id, &d);
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn leave_active_circle_panics_bad_status() {
    let f = setup();
    let id = create_full_circle(&f);
    f.client.leave(&id, &f.members[1]);
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn cancel_active_circle_panics_bad_status() {
    let f = setup();
    let id = create_full_circle(&f);
    f.client.cancel(&id, &f.members[0]);
}

#[test]
#[should_panic(expected = "Error(Contract, #6)")]
fn leave_by_non_member_panics_not_member() {
    let f = setup();
    let a = &f.members[0];
    let id = f
        .client
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &3, &PERIOD);
    let outsider = Address::generate(&f.env);
    f.client.leave(&id, &outsider);
}

#[test]
fn contribution_in_one_cycle_does_not_count_for_the_next() {
    let f = setup();
    let [a, b, c] = &f.members;
    let id = create_full_circle(&f);

    // Cycle 0: everyone pays and the cycle settles.
    for m in f.members.iter() {
        f.client.contribute(&id, m);
    }
    f.client.settle_cycle(&id);
    assert_eq!(f.client.get_circle(&id).current_cycle, 1);
    let a_balance = f.token.balance(a);

    // Cycle 1: c skips. Its cycle-0 payment must not cover cycle 1, so it is
    // slashed one contribution, while a and b are not.
    f.client.contribute(&id, a);
    f.client.contribute(&id, b);
    advance_past_deadline(&f, id);
    f.client.settle_cycle(&id);

    assert_eq!(
        f.client.get_member(&id, c).deposit_remaining,
        DEPOSIT - CONTRIB
    );
    assert_eq!(f.client.get_member(&id, a).deposit_remaining, DEPOSIT);
    assert_eq!(f.client.get_circle(&id).slashed_total, CONTRIB);
    // a paid again in cycle 1 (the cycle-0 payment did not carry over).
    assert!(f.token.balance(a) < a_balance);
}

// ---- Authorization --------------------------------------------------------
//
// `setup()` uses `mock_all_auths`, so a dropped `require_auth` would not make
// any other test fail. These assert the exact signer and invocation tree
// recorded for every user-facing call.

/// A recorded invocation of `fn_name` on `contract` with `args`.
fn invocation(
    contract: &Address,
    fn_name: &str,
    args: Vec<Val>,
    sub_invocations: std::vec::Vec<AuthorizedInvocation>,
) -> AuthorizedInvocation {
    AuthorizedInvocation {
        function: AuthorizedFunction::Contract((
            contract.clone(),
            Symbol::new(contract.env(), fn_name),
            args,
        )),
        sub_invocations,
    }
}

/// The token `transfer(from, to, amount)` the signer authorises as a
/// sub-invocation of the circle call.
fn transfer_invocation(f: &Fixture, from: &Address, amount: i128) -> AuthorizedInvocation {
    invocation(
        &f.token.address,
        "transfer",
        (from.clone(), f.client.address.clone(), amount).into_val(&f.env),
        std::vec![],
    )
}

#[test]
fn create_circle_requires_creator_auth() {
    let f = setup();
    let a = &f.members[0];
    f.client
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &3, &PERIOD);
    assert_eq!(
        f.env.auths(),
        std::vec![(
            a.clone(),
            invocation(
                &f.client.address,
                "create_circle",
                (
                    a.clone(),
                    f.token.address.clone(),
                    CONTRIB,
                    DEPOSIT,
                    3u32,
                    PERIOD
                )
                    .into_val(&f.env),
                std::vec![transfer_invocation(&f, a, DEPOSIT)],
            )
        )]
    );
}

#[test]
fn create_circle_zero_deposit_requires_creator_auth() {
    let f = setup();
    let a = &f.members[0];
    f.client
        .create_circle(a, &f.token.address, &CONTRIB, &0, &3, &PERIOD);
    // No deposit, so no token transfer is authorised under the call.
    assert_eq!(
        f.env.auths(),
        std::vec![(
            a.clone(),
            invocation(
                &f.client.address,
                "create_circle",
                (
                    a.clone(),
                    f.token.address.clone(),
                    CONTRIB,
                    0i128,
                    3u32,
                    PERIOD
                )
                    .into_val(&f.env),
                std::vec![],
            )
        )]
    );
}

#[test]
fn join_requires_member_auth() {
    let f = setup();
    let [a, b, _] = &f.members;
    let id = f
        .client
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &3, &PERIOD);
    f.client.join(&id, b);
    assert_eq!(
        f.env.auths(),
        std::vec![(
            b.clone(),
            invocation(
                &f.client.address,
                "join",
                (id, b.clone()).into_val(&f.env),
                std::vec![transfer_invocation(&f, b, DEPOSIT)],
            )
        )]
    );
}

#[test]
fn leave_requires_member_auth() {
    let f = setup();
    let [a, b, _] = &f.members;
    let id = f
        .client
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &3, &PERIOD);
    f.client.join(&id, b);
    f.client.leave(&id, b);
    // The refund is sent by the contract itself, so only the member's
    // authorisation of `leave` is recorded.
    assert_eq!(
        f.env.auths(),
        std::vec![(
            b.clone(),
            invocation(
                &f.client.address,
                "leave",
                (id, b.clone()).into_val(&f.env),
                std::vec![],
            )
        )]
    );
}

#[test]
fn cancel_requires_creator_auth() {
    let f = setup();
    let [a, b, _] = &f.members;
    let id = f
        .client
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &3, &PERIOD);
    f.client.join(&id, b);
    f.client.cancel(&id, a);
    assert_eq!(
        f.env.auths(),
        std::vec![(
            a.clone(),
            invocation(
                &f.client.address,
                "cancel",
                (id, a.clone()).into_val(&f.env),
                std::vec![],
            )
        )]
    );
}

#[test]
fn contribute_requires_member_auth() {
    let f = setup();
    let id = create_full_circle(&f);
    let b = &f.members[1];
    f.client.contribute(&id, b);
    assert_eq!(
        f.env.auths(),
        std::vec![(
            b.clone(),
            invocation(
                &f.client.address,
                "contribute",
                (id, b.clone()).into_val(&f.env),
                std::vec![transfer_invocation(&f, b, CONTRIB)],
            )
        )]
    );
}

#[test]
fn settle_cycle_requires_no_auth() {
    let f = setup();
    let id = create_full_circle(&f);
    for m in f.members.iter() {
        f.client.contribute(&id, m);
    }
    f.client.settle_cycle(&id);
    // Permissionless crank: nobody's authorisation is consumed.
    assert_eq!(f.env.auths(), std::vec![]);
    assert!(f.client.get_member(&id, &f.members[0]).received);
}

// ---- Deadline boundary ----------------------------------------------------

#[test]
fn settlement_at_exact_deadline_is_not_due() {
    let f = setup();
    let [a, b, c] = &f.members;
    f.env.ledger().with_mut(|l| l.timestamp = 10_000);
    let id = create_full_circle(&f);
    let deadline = f.client.cycle_deadline(&id);
    assert_eq!(deadline, 10_000 + PERIOD);

    // c has not paid, so only the deadline can make the cycle due.
    f.client.contribute(&id, a);
    f.client.contribute(&id, b);

    // At exactly the deadline the cycle is still open.
    f.env.ledger().with_mut(|l| l.timestamp = deadline);
    assert_eq!(
        f.client.try_settle_cycle(&id),
        Err(Ok(Error::NotDue.into()))
    );
    assert_eq!(f.client.get_circle(&id).current_cycle, 0);
    assert_eq!(f.client.get_member(&id, c).deposit_remaining, DEPOSIT);

    // One second later it settles: c is slashed and a receives the pot.
    f.env.ledger().with_mut(|l| l.timestamp = deadline + 1);
    assert_eq!(f.client.try_settle_cycle(&id), Ok(Ok(())));
    assert_eq!(f.client.get_circle(&id).current_cycle, 1);
    assert_eq!(
        f.client.get_member(&id, c).deposit_remaining,
        DEPOSIT - CONTRIB
    );
    assert!(f.client.get_member(&id, a).received);
}

// ---- Minimum-size circle --------------------------------------------------

#[test]
fn min_size_two_member_lifecycle() {
    let f = setup();
    let [a, b, _] = &f.members;
    let id = f
        .client
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &2, &PERIOD);
    assert_eq!(f.client.get_circle(&id).status, CircleStatus::Open);
    f.client.join(&id, b);

    // The second member fills it and starts it.
    assert_eq!(f.client.get_circle(&id).status, CircleStatus::Active);
    assert_eq!(f.token.balance(a), 1_000 - DEPOSIT);
    assert_eq!(f.token.balance(b), 1_000 - DEPOSIT);
    assert_eq!(f.token.balance(&f.client.address), 2 * DEPOSIT);

    // Cycle 0: both pay; a (first in join order) receives the 200 pot.
    f.client.contribute(&id, a);
    f.client.contribute(&id, b);
    f.client.settle_cycle(&id);
    assert!(f.client.get_member(&id, a).received);
    assert!(!f.client.get_member(&id, b).received);
    assert_eq!(f.client.get_circle(&id).current_cycle, 1);
    assert_eq!(f.token.balance(a), 1_000 - DEPOSIT - CONTRIB + 2 * CONTRIB);
    assert_eq!(f.token.balance(b), 1_000 - DEPOSIT - CONTRIB);
    assert_eq!(f.token.balance(&f.client.address), 2 * DEPOSIT);

    // Cycle 1: both pay; b receives and the circle completes.
    f.client.contribute(&id, a);
    f.client.contribute(&id, b);
    f.client.settle_cycle(&id);

    let circle = f.client.get_circle(&id);
    assert_eq!(circle.status, CircleStatus::Completed);
    assert_eq!(circle.current_cycle, 1);
    assert_eq!(circle.slashed_total, 0);
    // Each paid 2x100 in, received 200 once and got the deposit back.
    for m in [a, b] {
        let st = f.client.get_member(&id, m);
        assert!(st.received);
        assert!(!st.defaulted);
        assert_eq!(st.deposit_remaining, 0);
        assert_eq!(f.token.balance(m), 1_000);
        assert_eq!(f.rep.get_reputation(m).completed, 1);
        assert_eq!(f.rep.get_reputation(m).defaulted, 0);
    }
    assert_eq!(f.token.balance(&f.client.address), 0);
}

// ---- Zero-deposit circle --------------------------------------------------

#[test]
fn zero_deposit_lifecycle_miss_defaults_immediately() {
    let f = setup();
    let [a, b, c] = &f.members;
    let id = f
        .client
        .create_circle(a, &f.token.address, &CONTRIB, &0, &3, &PERIOD);
    f.client.join(&id, b);
    f.client.join(&id, c);
    assert_eq!(f.client.get_circle(&id).status, CircleStatus::Active);
    assert_eq!(f.token.balance(&f.client.address), 0);

    // Cycle 1 (index 0): b misses. With no deposit to slash, b defaults at
    // the first miss; a receives the 200 paid by a and c.
    f.client.contribute(&id, a);
    f.client.contribute(&id, c);
    advance_past_deadline(&f, id);
    f.client.settle_cycle(&id);

    let st_b = f.client.get_member(&id, b);
    assert!(st_b.defaulted);
    assert!(!st_b.received);
    assert_eq!(st_b.deposit_remaining, 0);
    assert_eq!(f.rep.get_reputation(b).defaulted, 1);
    assert_eq!(f.client.get_circle(&id).slashed_total, 0);
    assert!(f.client.get_member(&id, a).received);
    assert_eq!(f.token.balance(a), 1_000 - CONTRIB + 2 * CONTRIB);
    assert_eq!(f.token.balance(b), 1_000);
    assert_eq!(f.token.balance(c), 1_000 - CONTRIB);
    assert_eq!(f.token.balance(&f.client.address), 0);

    // b can no longer contribute.
    assert_eq!(
        f.client.try_contribute(&id, b),
        Err(Ok(Error::Defaulted.into()))
    );

    // Cycle 2 (index 1): b is skipped in rotation, so c receives instead,
    // and the circle completes with no third cycle.
    f.client.contribute(&id, a);
    f.client.contribute(&id, c);
    f.client.settle_cycle(&id);

    let circle = f.client.get_circle(&id);
    assert_eq!(circle.status, CircleStatus::Completed);
    assert_eq!(circle.current_cycle, 1);
    assert_eq!(circle.slashed_total, 0);
    assert!(f.client.get_member(&id, c).received);
    assert!(!f.client.get_member(&id, b).received);

    // Exact balances: a: -100 +200 -100 = 0; c: -100 -100 +200 = 0; b: 0.
    assert_eq!(f.token.balance(a), 1_000);
    assert_eq!(f.token.balance(b), 1_000);
    assert_eq!(f.token.balance(c), 1_000);
    assert_eq!(f.token.balance(&f.client.address), 0);

    // Reported: a and c completed; b defaulted and did not complete.
    assert_eq!(f.rep.get_reputation(a).completed, 1);
    assert_eq!(f.rep.get_reputation(c).completed, 1);
    assert_eq!(f.rep.get_reputation(a).defaulted, 0);
    assert_eq!(f.rep.get_reputation(c).defaulted, 0);
    assert_eq!(f.rep.get_reputation(b).completed, 0);
    assert_eq!(f.rep.get_reputation(b).defaulted, 1);
}
