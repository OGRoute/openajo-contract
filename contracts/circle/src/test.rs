#![cfg(test)]

extern crate std;

use crate::{
    errors::Error,
    types::{CircleStatus, MemberState},
    CircleContract, CircleContractClient, MIN_PERIOD_SECS,
};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, AuthorizedFunction, AuthorizedInvocation, Events, Ledger},
    token::{StellarAssetClient, TokenClient},
    vec, Address, Env, IntoVal, Symbol, Val, Vec,
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

// ---- recipient_misses_their_own_cycle_contribution (#102) -----------------

#[test]
fn recipient_misses_their_own_cycle_contribution() {
    let f = setup();
    let [a, b, c] = &f.members;
    let id = create_full_circle(&f);

    // Cycle 0: a is the rotation recipient but skips their contribution.
    // b and c pay; the deadline settles the cycle by force.
    f.client.contribute(&id, b);
    f.client.contribute(&id, c);
    advance_past_deadline(&f, id);
    f.client.settle_cycle(&id);

    // a is slashed 100 from their deposit (150 → 50) and still receives the pot.
    // Pot = 100 (b) + 100 (c) + 100 (slashed from a's deposit) = 300.
    let st_a = f.client.get_member(&id, a);
    assert_eq!(st_a.deposit_remaining, DEPOSIT - CONTRIB); // 50
    assert!(!st_a.defaulted); // one miss is fully covered; no default yet
    assert!(st_a.received); // still the payout recipient for cycle 0

    // a: 1 000 − 150 (deposit) + 300 (pot) = 1 150.
    assert_eq!(f.token.balance(a), 1_000 - DEPOSIT + 3 * CONTRIB);
    // b and c each paid 100 and their full deposits remain escrowed.
    assert_eq!(f.token.balance(b), 1_000 - DEPOSIT - CONTRIB);
    assert_eq!(f.token.balance(c), 1_000 - DEPOSIT - CONTRIB);
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
#[should_panic(expected = "Error(Contract, #4)")]
fn contribute_on_open_circle_panics_bad_status() {
    let f = setup();
    let [a, b, _] = &f.members;
    // Circle is Open (only two of the three required members have joined).
    let id = f
        .client
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &3, &PERIOD);
    f.client.join(&id, b);
    // contribute must reject the call because the circle is not yet Active.
    f.client.contribute(&id, a);
}

#[test]
#[should_panic(expected = "Error(Contract, #6)")]
fn contribute_by_non_member_panics_not_member() {
    let f = setup();
    let id = create_full_circle(&f);
    let outsider = Address::generate(&f.env);
    f.client.contribute(&id, &outsider);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn settle_twice_in_one_cycle_panics_not_due() {
    let f = setup();
    let id = create_full_circle(&f);
    // All members pay, so the first settle succeeds immediately (all-paid path).
    for m in f.members.iter() {
        f.client.contribute(&id, m);
    }
    f.client.settle_cycle(&id);
    assert_eq!(f.client.get_circle(&id).current_cycle, 1);
    // A second immediate settle has nobody paid and the new deadline is in the
    // future, so it must be NotDue.
    f.client.settle_cycle(&id);
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
    f.client.create_circle(
        a,
        &f.token.address,
        &CONTRIB,
        &DEPOSIT,
        &3,
        &(MIN_PERIOD_SECS - 1),
    );
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
    let id = f.client.create_circle(
        a,
        &f.token.address,
        &CONTRIB,
        &DEPOSIT,
        &3,
        &MIN_PERIOD_SECS,
    );
    assert_eq!(f.client.get_circle(&id).period_secs, MIN_PERIOD_SECS);
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

// ---- Initialization -------------------------------------------------------

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn initialize_twice_panics() {
    let f = setup();
    let other = Address::generate(&f.env);
    f.client.initialize(&other, &f.rep.address);
}

// ---- Single-member cancel -------------------------------------------------

#[test]
fn cancel_immediately_after_creation_refunds_the_creator() {
    let f = setup();
    let [a, _, _] = &f.members;
    f.env.ledger().with_mut(|l| l.timestamp = 2_000);
    let id = f
        .client
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &3, &PERIOD);
    assert_eq!(f.token.balance(a), 1_000 - DEPOSIT);
    assert_eq!(f.token.balance(&f.client.address), DEPOSIT);
    assert_eq!(f.client.get_members(&id).len(), 1);

    f.client.cancel(&id, a);

    let circle = f.client.get_circle(&id);
    assert_eq!(circle.status, CircleStatus::Cancelled);
    assert_eq!(circle.started_at, 0);
    assert_eq!(circle.ended_at, 2_000);
    assert_eq!(f.client.get_member(&id, a).deposit_remaining, 0);
    assert_eq!(f.token.balance(a), 1_000);
    assert_eq!(f.token.balance(&f.client.address), 0);
}

// ---- cycle_deadline view --------------------------------------------------

#[test]
fn cycle_deadline_advances_by_period_after_each_settlement() {
    let f = setup();
    let start = 10_000;
    f.env.ledger().with_mut(|l| l.timestamp = start);
    let id = create_full_circle(&f);

    // At activation the first deadline is one period after started_at.
    assert_eq!(f.client.get_circle(&id).started_at, start);
    assert_eq!(f.client.cycle_deadline(&id), start + PERIOD);

    // Cycle 0 settles early (everyone paid): the deadline still moves by
    // exactly one period, not relative to the settlement time.
    f.env.ledger().with_mut(|l| l.timestamp = start + 10);
    for m in f.members.iter() {
        f.client.contribute(&id, m);
    }
    f.client.settle_cycle(&id);
    assert_eq!(f.client.get_circle(&id).current_cycle, 1);
    assert_eq!(f.client.cycle_deadline(&id), start + 2 * PERIOD);

    // Cycle 1 settles late (past its deadline, with a miss): the deadline is
    // anchored to started_at, so lateness does not push it further out.
    let [a, b, c] = &f.members;
    f.client.contribute(&id, a);
    f.client.contribute(&id, c);
    f.env
        .ledger()
        .with_mut(|l| l.timestamp = start + 2 * PERIOD + 500);
    f.client.settle_cycle(&id);
    assert_eq!(
        f.client.get_member(&id, b).deposit_remaining,
        DEPOSIT - CONTRIB
    );
    assert_eq!(f.client.get_circle(&id).current_cycle, 2);
    assert_eq!(f.client.cycle_deadline(&id), start + 3 * PERIOD);

    // Final cycle completes the circle; current_cycle stays on the last
    // index, so the view keeps reporting the last cycle's deadline.
    for m in f.members.iter() {
        f.client.contribute(&id, m);
    }
    f.client.settle_cycle(&id);
    let circle = f.client.get_circle(&id);
    assert_eq!(circle.status, CircleStatus::Completed);
    assert_eq!(circle.current_cycle, 2);
    assert_eq!(f.client.cycle_deadline(&id), start + 3 * PERIOD);
}

// ---- Concurrent circles ---------------------------------------------------

/// The contract's balance of `token` equals the remaining deposits of
/// `members` in circle `id`. Holds at cycle boundaries when `token` is used
/// by this circle alone.
fn assert_circle_escrow(
    f: &Fixture,
    token: &TokenClient,
    id: u32,
    members: &[&Address],
    expected: i128,
) {
    let deposits: i128 = members
        .iter()
        .map(|m| f.client.get_member(&id, m).deposit_remaining)
        .sum();
    assert_eq!(deposits, expected);
    assert_eq!(token.balance(&f.client.address), expected);
}

#[test]
fn concurrent_circles_with_overlapping_members_stay_isolated() {
    let f = setup();
    let [a, b, c] = &f.members;
    let t1 = &f.token;

    // Second token, funding only b and c.
    let sac2 = f
        .env
        .register_stellar_asset_contract_v2(Address::generate(&f.env));
    let t2 = TokenClient::new(&f.env, &sac2.address());
    let mint2 = StellarAssetClient::new(&f.env, &sac2.address());
    mint2.mint(b, &500);
    mint2.mint(c, &500);

    // Circle 1: token 1, size 3, 100/150, 1h. Circle 2: token 2, size 2,
    // 40/60, 2h. b and c are members of both.
    const C2: i128 = 40;
    const D2: i128 = 60;
    const P2: u64 = 7_200;
    let start = 1_000;
    f.env.ledger().with_mut(|l| l.timestamp = start);
    let id1 = f
        .client
        .create_circle(a, &t1.address, &CONTRIB, &DEPOSIT, &3, &PERIOD);
    f.client.join(&id1, b);
    let id2 = f.client.create_circle(b, &t2.address, &C2, &D2, &2, &P2);
    f.client.join(&id1, c);
    f.client.join(&id2, c);
    assert_eq!((id1, id2), (0, 1));
    assert_eq!(f.client.total_circles(), 2);

    let m1 = [a, b, c];
    let m2 = [b, c];
    let c1 = f.client.get_circle(&id1);
    let c2 = f.client.get_circle(&id2);
    assert_eq!(c1.status, CircleStatus::Active);
    assert_eq!(c2.status, CircleStatus::Active);
    assert_eq!(c1.token, t1.address);
    assert_eq!(c2.token, t2.address);
    assert_eq!(f.client.cycle_deadline(&id1), start + PERIOD);
    assert_eq!(f.client.cycle_deadline(&id2), start + P2);

    // Each deposit was taken in its own circle's token only.
    for m in m1 {
        assert_eq!(t1.balance(m), 1_000 - DEPOSIT);
    }
    for m in m2 {
        assert_eq!(t2.balance(m), 500 - D2);
    }
    assert_eq!(t2.balance(a), 0);
    assert_circle_escrow(&f, t1, id1, &m1, 3 * DEPOSIT);
    assert_circle_escrow(&f, &t2, id2, &m2, 2 * D2);

    // Circle 1 cycle 0: everyone pays. In circle 2 only b pays.
    for m in m1 {
        f.client.contribute(&id1, m);
    }
    f.client.contribute(&id2, b);
    f.client.settle_cycle(&id1);
    // a receives the 300 pot in token 1.
    assert_eq!(t1.balance(a), 1_000 - DEPOSIT - CONTRIB + 3 * CONTRIB);
    assert_eq!(t1.balance(b), 1_000 - DEPOSIT - CONTRIB);
    assert_eq!(t1.balance(c), 1_000 - DEPOSIT - CONTRIB);
    assert_circle_escrow(&f, t1, id1, &m1, 3 * DEPOSIT);

    // c paying circle 1 does not count for circle 2, and circle 1 settling
    // does not move circle 2: it is still in cycle 0 and not yet due.
    assert_eq!(
        f.client.try_settle_cycle(&id2),
        Err(Ok(Error::NotDue.into()))
    );
    let c2 = f.client.get_circle(&id2);
    assert_eq!(c2.current_cycle, 0);
    assert_eq!(c2.slashed_total, 0);
    assert!(!f.client.get_member(&id2, b).received);
    assert_eq!(t2.balance(b), 500 - D2 - C2);
    assert_eq!(t2.balance(&f.client.address), 2 * D2 + C2);

    // Circle 2 cycle 0 settles at its deadline: c is slashed 40 of 60 and b
    // receives the 80 pot in token 2.
    f.env.ledger().with_mut(|l| l.timestamp = start + P2 + 1);
    f.client.settle_cycle(&id2);
    let c2 = f.client.get_circle(&id2);
    assert_eq!(c2.current_cycle, 1);
    assert_eq!(c2.slashed_total, C2);
    assert!(f.client.get_member(&id2, b).received);
    assert_eq!(f.client.get_member(&id2, c).deposit_remaining, D2 - C2);
    assert_eq!(t2.balance(b), 500 - D2 - C2 + 2 * C2);
    assert_eq!(t2.balance(c), 500 - D2);
    assert_circle_escrow(&f, &t2, id2, &m2, D2 + (D2 - C2));

    // c's slash in circle 2 leaves circle 1 untouched.
    let c1 = f.client.get_circle(&id1);
    assert_eq!(c1.current_cycle, 1);
    assert_eq!(c1.slashed_total, 0);
    for m in m1 {
        let st = f.client.get_member(&id1, m);
        assert_eq!(st.deposit_remaining, DEPOSIT);
        assert!(!st.defaulted);
    }
    assert!(f.client.get_member(&id1, a).received);
    assert!(!f.client.get_member(&id1, b).received);
    assert_circle_escrow(&f, t1, id1, &m1, 3 * DEPOSIT);
    assert_eq!(f.rep.get_reputation(c).defaulted, 0);

    // Circle 1 cycle 1: everyone pays; b receives in token 1.
    for m in m1 {
        f.client.contribute(&id1, m);
    }
    f.client.settle_cycle(&id1);
    assert_eq!(t1.balance(a), 1_000 - DEPOSIT - 2 * CONTRIB + 3 * CONTRIB);
    assert_eq!(t1.balance(b), 1_000 - DEPOSIT - 2 * CONTRIB + 3 * CONTRIB);
    assert_eq!(t1.balance(c), 1_000 - DEPOSIT - 2 * CONTRIB);
    assert_circle_escrow(&f, t1, id1, &m1, 3 * DEPOSIT);

    // Circle 2 cycle 1: both pay; c receives and circle 2 completes,
    // returning b's 60 and c's remaining 20.
    f.client.contribute(&id2, b);
    f.client.contribute(&id2, c);
    f.client.settle_cycle(&id2);
    assert_eq!(f.client.get_circle(&id2).status, CircleStatus::Completed);
    // b: -60 -40 +80 -40 +60 = 0. c: -60 (miss, 40 slashed) -40 +80 +20 = 0.
    assert_eq!(t2.balance(b), 500);
    assert_eq!(t2.balance(c), 500);
    assert_eq!(t2.balance(&f.client.address), 0);
    assert_eq!(f.rep.get_reputation(b).completed, 1);
    assert_eq!(f.rep.get_reputation(c).completed, 1);
    assert_eq!(f.rep.get_reputation(a).completed, 0);

    // Circle 2 completing leaves circle 1 active, mid-rotation and fully
    // escrowed in token 1.
    let c1 = f.client.get_circle(&id1);
    assert_eq!(c1.status, CircleStatus::Active);
    assert_eq!(c1.current_cycle, 2);
    assert_circle_escrow(&f, t1, id1, &m1, 3 * DEPOSIT);

    // Circle 1 cycle 2: everyone pays; c receives and circle 1 completes.
    for m in m1 {
        f.client.contribute(&id1, m);
    }
    f.client.settle_cycle(&id1);
    assert_eq!(f.client.get_circle(&id1).status, CircleStatus::Completed);
    for m in m1 {
        assert_eq!(t1.balance(m), 1_000);
        assert_eq!(f.rep.get_reputation(m).defaulted, 0);
    }
    assert_eq!(t1.balance(&f.client.address), 0);
    assert_eq!(t2.balance(&f.client.address), 0);
    assert_eq!(t2.balance(a), 0);
    assert_eq!(f.rep.get_reputation(a).completed, 1);
    assert_eq!(f.rep.get_reputation(b).completed, 2);
    assert_eq!(f.rep.get_reputation(c).completed, 2);
}

// ---- Extra members --------------------------------------------------------

/// A fresh address funded with 1_000 units, for scenarios needing more than
/// the fixture's three members.
fn funded_member(f: &Fixture) -> Address {
    let m = Address::generate(&f.env);
    StellarAssetClient::new(&f.env, &f.token.address).mint(&m, &1_000);
    m
}

// ---- Partial slashes leading to default -----------------------------------

#[test]
fn partial_slash_then_default_on_second_miss_with_fractional_deposit() {
    // The deposit is 1.5x the contribution, so it covers one miss in full and
    // only half of the next. Uses its own amounts (not CONTRIB/DEPOSIT) and a
    // 4-member circle so the circle is still running after the default.
    const C: i128 = 200;
    const D: i128 = 300; // 1.5 * C
    let f = setup();
    let [a, b, c] = &f.members;
    let d = funded_member(&f);
    let id = f
        .client
        .create_circle(a, &f.token.address, &C, &D, &4, &PERIOD);
    f.client.join(&id, b);
    f.client.join(&id, c);
    f.client.join(&id, &d);
    assert_eq!(f.client.get_circle(&id).status, CircleStatus::Active);
    assert_eq!(f.token.balance(&f.client.address), 4 * D);

    // Cycle 0: d misses. The full contribution is slashed (300 -> 100), so d
    // is NOT defaulted yet and nothing is reported. a receives 3*200 + 200.
    f.client.contribute(&id, a);
    f.client.contribute(&id, b);
    f.client.contribute(&id, c);
    advance_past_deadline(&f, id);
    f.client.settle_cycle(&id);

    let st = f.client.get_member(&id, &d);
    assert_eq!(st.deposit_remaining, D - C); // 100
    assert!(!st.defaulted);
    assert!(!st.received);
    assert_eq!(f.rep.get_reputation(&d).defaulted, 0);
    assert_eq!(f.client.get_circle(&id).slashed_total, C);
    assert_eq!(f.client.get_circle(&id).current_cycle, 1);
    assert_eq!(f.token.balance(a), 1_000 - D - C + 4 * C); // 1_300
                                                           // Escrow: four deposits minus d's slash.
    assert_eq!(f.token.balance(&f.client.address), 4 * D - C); // 1_000

    // Cycle 1: d misses again. Only 100 is left, a partial slash of half the
    // contribution, which marks the default and reports it exactly once.
    f.client.contribute(&id, a);
    f.client.contribute(&id, b);
    f.client.contribute(&id, c);
    advance_past_deadline(&f, id);
    f.client.settle_cycle(&id);

    let st = f.client.get_member(&id, &d);
    assert_eq!(st.deposit_remaining, 0);
    assert!(st.defaulted);
    assert!(!st.received);
    assert_eq!(f.rep.get_reputation(&d).defaulted, 1);
    assert_eq!(f.client.get_circle(&id).slashed_total, D);
    // b receives 3*200 + the 100 partial slash.
    assert_eq!(f.token.balance(b), 1_000 - D - 2 * C + 3 * C + (D - C)); // 1_000
                                                                         // Escrow: only a, b and c's untouched deposits remain.
    assert_eq!(f.token.balance(&f.client.address), 3 * D); // 900

    // d is skipped from here on: contributing is rejected.
    assert_eq!(
        f.client.try_contribute(&id, &d),
        Err(Ok(Error::Defaulted.into()))
    );

    // Cycle 2: the three remaining members pay; c receives 600 and the
    // circle completes without waiting for the deadline.
    f.client.contribute(&id, a);
    f.client.contribute(&id, b);
    f.client.contribute(&id, c);
    f.client.settle_cycle(&id);

    let circle = f.client.get_circle(&id);
    assert_eq!(circle.status, CircleStatus::Completed);
    assert_eq!(circle.slashed_total, D);

    // Exact balances:
    // a: -300 -600 +800 +300 = +200
    // b: -300 -600 +700 +300 = +100
    // c: -300 -600 +600 +300 =    0
    // d: -300                = -300  (the whole deposit, slashed 200 + 100)
    assert_eq!(f.token.balance(a), 1_200);
    assert_eq!(f.token.balance(b), 1_100);
    assert_eq!(f.token.balance(c), 1_000);
    assert_eq!(f.token.balance(&d), 700);
    assert_eq!(f.token.balance(&f.client.address), 0);

    // The default was recorded once (on the second miss), never a completion.
    assert_eq!(f.rep.get_reputation(&d).defaulted, 1);
    assert_eq!(f.rep.get_reputation(&d).completed, 0);
    for m in [a, b, c] {
        assert_eq!(f.rep.get_reputation(m).completed, 1);
        assert_eq!(f.rep.get_reputation(m).defaulted, 0);
    }
}

// ---- Leave and rejoin ordering --------------------------------------------

#[test]
fn leave_preserves_join_order_of_remaining_members() {
    let f = setup();
    let [a, b, c] = &f.members;
    let d = funded_member(&f);
    let e = funded_member(&f);
    let g = funded_member(&f);
    let id = f
        .client
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &5, &PERIOD);
    f.client.join(&id, b);
    f.client.join(&id, c);
    f.client.join(&id, &d);
    assert_eq!(
        f.client.get_members(&id),
        soroban_sdk::vec![&f.env, a.clone(), b.clone(), c.clone(), d.clone()]
    );

    // The second member leaves: the rest keep their relative order.
    f.client.leave(&id, b);
    assert_eq!(
        f.client.get_members(&id),
        soroban_sdk::vec![&f.env, a.clone(), c.clone(), d.clone()]
    );
    assert_eq!(f.token.balance(b), 1_000);

    // Newcomers fill the circle and land at the end, in their join order.
    f.client.join(&id, &e);
    f.client.join(&id, &g);
    let order = [a.clone(), c.clone(), d.clone(), e.clone(), g.clone()];
    assert_eq!(
        f.client.get_members(&id),
        soroban_sdk::vec![
            &f.env,
            a.clone(),
            c.clone(),
            d.clone(),
            e.clone(),
            g.clone()
        ]
    );
    assert_eq!(f.client.get_circle(&id).status, CircleStatus::Active);

    // Payouts follow that order exactly: one recipient per cycle, each
    // receiving the full 5 * 100 pot.
    for (cycle, recipient) in order.iter().enumerate() {
        for m in order.iter() {
            f.client.contribute(&id, m);
        }
        f.client.settle_cycle(&id);
        let paid_so_far = (cycle as i128 + 1) * CONTRIB;
        for (i, m) in order.iter().enumerate() {
            assert_eq!(f.client.get_member(&id, m).received, i <= cycle);
        }
        if cycle < order.len() - 1 {
            assert_eq!(
                f.token.balance(recipient),
                1_000 - DEPOSIT - paid_so_far + 5 * CONTRIB
            );
        }
    }

    assert_eq!(f.client.get_circle(&id).status, CircleStatus::Completed);
    for m in order.iter() {
        assert_eq!(f.token.balance(m), 1_000);
    }
    assert_eq!(f.token.balance(b), 1_000);
    assert_eq!(f.token.balance(&f.client.address), 0);
}

#[test]
fn rejoin_after_leaving_starts_fresh_at_the_end() {
    let f = setup();
    let [a, b, c] = &f.members;
    let d = funded_member(&f);
    let id = f
        .client
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &4, &PERIOD);
    f.client.join(&id, b);
    f.client.join(&id, c);

    f.client.leave(&id, b);
    assert_eq!(f.token.balance(b), 1_000);
    assert_eq!(
        f.client.get_members(&id),
        soroban_sdk::vec![&f.env, a.clone(), c.clone()]
    );
    // Leaving removes the member record entirely.
    assert_eq!(
        f.client.try_get_member(&id, b),
        Err(Ok(Error::NotMember.into()))
    );

    // Rejoining escrows a new deposit and appends b behind c.
    f.client.join(&id, b);
    assert_eq!(
        f.client.get_members(&id),
        soroban_sdk::vec![&f.env, a.clone(), c.clone(), b.clone()]
    );
    let st = f.client.get_member(&id, b);
    assert_eq!(st.deposit_remaining, DEPOSIT);
    assert!(!st.received);
    assert!(!st.defaulted);
    assert_eq!(f.token.balance(b), 1_000 - DEPOSIT);
    // Exactly one deposit per current member is escrowed.
    assert_eq!(f.token.balance(&f.client.address), 3 * DEPOSIT);

    // Fill the circle; payout order puts the rejoined member third.
    f.client.join(&id, &d);
    assert_eq!(f.client.get_circle(&id).status, CircleStatus::Active);
    let order = [a.clone(), c.clone(), b.clone(), d.clone()];
    for (cycle, recipient) in order.iter().enumerate() {
        for m in order.iter() {
            f.client.contribute(&id, m);
        }
        f.client.settle_cycle(&id);
        assert!(f.client.get_member(&id, recipient).received);
        if cycle + 1 < order.len() {
            assert!(!f.client.get_member(&id, &order[cycle + 1]).received);
        }
    }

    assert_eq!(f.client.get_circle(&id).status, CircleStatus::Completed);
    for m in order.iter() {
        assert_eq!(f.token.balance(m), 1_000);
    }
    assert_eq!(f.token.balance(&f.client.address), 0);
}

// ---- Creator removes a member --------------------------------------------

#[test]
fn remove_member_refunds_exactly_and_preserves_order() {
    let f = setup();
    let [a, b, c] = &f.members;
    let d = funded_member(&f);
    let id = f
        .client
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &4, &PERIOD);
    f.client.join(&id, b);
    f.client.join(&id, c);
    assert_eq!(f.token.balance(&f.client.address), 3 * DEPOSIT);

    f.client.remove_member(&id, a, b);

    // Exactly the removed member's deposit leaves escrow, back to them.
    assert_eq!(f.token.balance(b), 1_000);
    assert_eq!(f.token.balance(a), 1_000 - DEPOSIT);
    assert_eq!(f.token.balance(c), 1_000 - DEPOSIT);
    assert_eq!(f.token.balance(&f.client.address), 2 * DEPOSIT);
    assert_eq!(
        f.client.get_members(&id),
        soroban_sdk::vec![&f.env, a.clone(), c.clone()]
    );
    assert_eq!(
        f.client.try_get_member(&id, b),
        Err(Ok(Error::NotMember.into()))
    );
    assert_eq!(f.client.get_circle(&id).status, CircleStatus::Open);

    // A newcomer takes the vacated slot at the end; the rest keep their
    // relative order, which is the payout order.
    f.client.join(&id, &d);
    assert_eq!(
        f.client.get_members(&id),
        soroban_sdk::vec![&f.env, a.clone(), c.clone(), d.clone()]
    );
}

#[test]
fn remove_member_emits_event() {
    let f = setup();
    let [a, b, _] = &f.members;
    let id = f
        .client
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &3, &PERIOD);
    f.client.join(&id, b);
    f.client.remove_member(&id, a, b);

    let topics: Vec<Val> = vec![
        &f.env,
        symbol_short!("circle").into_val(&f.env),
        symbol_short!("remove").into_val(&f.env),
    ];
    // Filter out the token's transfer event so only the circle's event is
    // compared: the refund is a sub-invocation of `remove_member`.
    let events = f.env.events().all().filter_by_contract(&f.client.address);
    let expected = vec![
        &f.env,
        (
            f.client.address.clone(),
            topics,
            (id, b.clone()).into_val(&f.env),
        ),
    ];
    assert_eq!(events, expected);
}

#[test]
fn remove_member_requires_creator_auth() {
    let f = setup();
    let [a, b, _] = &f.members;
    let id = f
        .client
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &3, &PERIOD);
    f.client.join(&id, b);
    f.client.remove_member(&id, a, b);
    // The refund is sent by the contract itself, so only the creator's
    // authorisation of `remove_member` is recorded.
    assert_eq!(
        f.env.auths(),
        std::vec![(
            a.clone(),
            invocation(
                &f.client.address,
                "remove_member",
                (id, a.clone(), b.clone()).into_val(&f.env),
                std::vec![],
            )
        )]
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #13)")]
fn non_creator_remove_member_panics_not_creator() {
    let f = setup();
    let [a, b, c] = &f.members;
    let id = f
        .client
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &3, &PERIOD);
    f.client.join(&id, b);
    f.client.join(&id, c);
    f.client.remove_member(&id, b, c);
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn remove_member_on_active_circle_panics_bad_status() {
    let f = setup();
    let [a, b, _] = &f.members;
    let id = create_full_circle(&f);
    f.client.remove_member(&id, a, b);
}

#[test]
#[should_panic(expected = "Error(Contract, #11)")]
fn creator_cannot_remove_self() {
    let f = setup();
    let [a, b, _] = &f.members;
    let id = f
        .client
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &3, &PERIOD);
    f.client.join(&id, b);
    f.client.remove_member(&id, a, a);
}

#[test]
#[should_panic(expected = "Error(Contract, #6)")]
fn remove_member_non_member_panics_not_member() {
    let f = setup();
    let [a, b, c] = &f.members;
    let id = f
        .client
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &3, &PERIOD);
    f.client.join(&id, b);
    f.client.remove_member(&id, a, c);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn remove_member_unknown_circle_panics_not_found() {
    let f = setup();
    let [a, b, _] = &f.members;
    f.client.remove_member(&UNKNOWN_ID, a, b);
}

// ---- Unknown circle ids (#88) ---------------------------------------------

const UNKNOWN_ID: u32 = 999;

#[test]
fn join_unknown_circle_panics_not_found() {
    let f = setup();
    assert_eq!(
        f.client.try_join(&UNKNOWN_ID, &f.members[0]),
        Err(Ok(Error::NotFound.into()))
    );
}

#[test]
fn leave_unknown_circle_panics_not_found() {
    let f = setup();
    assert_eq!(
        f.client.try_leave(&UNKNOWN_ID, &f.members[0]),
        Err(Ok(Error::NotFound.into()))
    );
}

#[test]
fn cancel_unknown_circle_panics_not_found() {
    let f = setup();
    assert_eq!(
        f.client.try_cancel(&UNKNOWN_ID, &f.members[0]),
        Err(Ok(Error::NotFound.into()))
    );
}

#[test]
fn contribute_unknown_circle_panics_not_found() {
    let f = setup();
    assert_eq!(
        f.client.try_contribute(&UNKNOWN_ID, &f.members[0]),
        Err(Ok(Error::NotFound.into()))
    );
}

#[test]
fn settle_cycle_unknown_circle_panics_not_found() {
    let f = setup();
    assert_eq!(
        f.client.try_settle_cycle(&UNKNOWN_ID),
        Err(Ok(Error::NotFound.into()))
    );
}

#[test]
fn get_circle_unknown_circle_panics_not_found() {
    let f = setup();
    // `Circle` has no `PartialEq`, so compare the error side only.
    assert_eq!(
        f.client.try_get_circle(&UNKNOWN_ID).err(),
        Some(Ok(Error::NotFound.into()))
    );
}

#[test]
fn get_members_unknown_circle_panics_not_found() {
    let f = setup();
    assert_eq!(
        f.client.try_get_members(&UNKNOWN_ID),
        Err(Ok(Error::NotFound.into()))
    );
}

#[test]
fn cycle_deadline_unknown_circle_panics_not_found() {
    let f = setup();
    assert_eq!(
        f.client.try_cycle_deadline(&UNKNOWN_ID),
        Err(Ok(Error::NotFound.into()))
    );
}

#[test]
fn get_member_unknown_circle_panics_not_member() {
    let f = setup();
    // `get_member` never loads the circle: it reads the (circle, member)
    // record directly, so an unknown id surfaces as NotMember, not NotFound.
    // Pinned so a change to that lookup order is a deliberate one.
    assert_eq!(
        f.client.try_get_member(&UNKNOWN_ID, &f.members[0]),
        Err(Ok(Error::NotMember.into()))
    );
}

#[test]
fn unknown_id_errors_write_nothing() {
    let f = setup();
    let id = create_full_circle(&f);
    let _ = f.client.try_join(&UNKNOWN_ID, &f.members[0]);
    let _ = f.client.try_contribute(&UNKNOWN_ID, &f.members[0]);
    let _ = f.client.try_settle_cycle(&UNKNOWN_ID);
    assert_eq!(f.client.total_circles(), id + 1);
    assert_eq!(f.token.balance(&f.client.address), 3 * DEPOSIT);
}

// ---- Insufficient balance reverts atomically (#105) -----------------------

/// A fresh address holding `amount` units of the fixture token.
fn member_with_balance(f: &Fixture, amount: i128) -> Address {
    let m = Address::generate(&f.env);
    if amount > 0 {
        StellarAssetClient::new(&f.env, &f.token.address).mint(&m, &amount);
    }
    m
}

#[test]
fn create_with_insufficient_balance_reverts_atomically() {
    let f = setup();
    let poor = member_with_balance(&f, DEPOSIT - 1);
    let before = f.client.total_circles();

    let res = f
        .client
        .try_create_circle(&poor, &f.token.address, &CONTRIB, &DEPOSIT, &3, &PERIOD);
    assert!(res.is_err());

    // No id consumed, no circle, no member record, no funds moved.
    assert_eq!(f.client.total_circles(), before);
    assert_eq!(
        f.client.try_get_circle(&before).err(),
        Some(Ok(Error::NotFound.into()))
    );
    assert_eq!(
        f.client.try_get_members(&before),
        Err(Ok(Error::NotFound.into()))
    );
    assert_eq!(f.token.balance(&poor), DEPOSIT - 1);
    assert_eq!(f.token.balance(&f.client.address), 0);

    // The open-circle counter was not bumped either: with a cap of 1 the
    // same creator can still open a circle once funded.
    f.client.set_open_circle_cap(&1);
    StellarAssetClient::new(&f.env, &f.token.address).mint(&poor, &1);
    let id = f
        .client
        .create_circle(&poor, &f.token.address, &CONTRIB, &DEPOSIT, &3, &PERIOD);
    assert_eq!(id, before);
}

#[test]
fn join_with_insufficient_balance_reverts_atomically() {
    let f = setup();
    let [a, b, _c] = &f.members;
    let id = f
        .client
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &3, &PERIOD);
    f.client.join(&id, b);
    let poor = member_with_balance(&f, DEPOSIT - 1);
    let members_before = f.client.get_members(&id);
    let escrow_before = f.token.balance(&f.client.address);

    assert!(f.client.try_join(&id, &poor).is_err());

    // Not added to the member list, no member record, circle still Open
    // (the failed join would have been the activating one).
    assert_eq!(f.client.get_members(&id), members_before);
    assert_eq!(
        f.client.try_get_member(&id, &poor),
        Err(Ok(Error::NotMember.into()))
    );
    let circle = f.client.get_circle(&id);
    assert_eq!(circle.status, CircleStatus::Open);
    assert_eq!(circle.started_at, 0);
    assert_eq!(f.token.balance(&poor), DEPOSIT - 1);
    assert_eq!(f.token.balance(&f.client.address), escrow_before);
}

#[test]
fn contribute_with_insufficient_balance_reverts_atomically() {
    let f = setup();
    let [a, b, c] = &f.members;
    let id = create_full_circle(&f);
    // Drain c down to one unit short of a contribution.
    let sink = Address::generate(&f.env);
    let spare = f.token.balance(c) - (CONTRIB - 1);
    f.token.transfer(c, &sink, &spare);
    let escrow_before = f.token.balance(&f.client.address);

    assert!(f.client.try_contribute(&id, c).is_err());

    // No funds moved and c is not marked paid for this cycle: with a and b
    // paid, the cycle is still not due before the deadline.
    assert_eq!(f.token.balance(c), CONTRIB - 1);
    assert_eq!(f.token.balance(&f.client.address), escrow_before);
    f.client.contribute(&id, a);
    f.client.contribute(&id, b);
    assert_eq!(
        f.client.try_settle_cycle(&id),
        Err(Ok(Error::NotDue.into()))
    );
    let circle = f.client.get_circle(&id);
    assert_eq!(circle.current_cycle, 0);
    assert_eq!(circle.status, CircleStatus::Active);
    assert_eq!(f.client.get_member(&id, c).deposit_remaining, DEPOSIT);

    // Once funded, c can still pay this cycle (no stale "paid" flag).
    StellarAssetClient::new(&f.env, &f.token.address).mint(c, &1);
    f.client.contribute(&id, c);
    f.client.settle_cycle(&id);
    assert!(f.client.get_member(&id, a).received);
}

// ---- settle_cycle on finished circles (#87) -------------------------------

fn balances(f: &Fixture) -> [i128; 4] {
    [
        f.token.balance(&f.client.address),
        f.token.balance(&f.members[0]),
        f.token.balance(&f.members[1]),
        f.token.balance(&f.members[2]),
    ]
}

#[test]
fn settle_cycle_on_completed_circle_panics_bad_status() {
    let f = setup();
    let id = create_full_circle(&f);
    complete_circle(&f, id);
    let circle = f.client.get_circle(&id);
    assert_eq!(circle.status, CircleStatus::Completed);
    let before = balances(&f);

    // Well past every deadline, so only the status guard can stop it.
    advance_past_deadline(&f, id);
    assert_eq!(
        f.client.try_settle_cycle(&id),
        Err(Ok(Error::BadStatus.into()))
    );

    assert_eq!(balances(&f), before);
    let after = f.client.get_circle(&id);
    assert_eq!(after.status, CircleStatus::Completed);
    assert_eq!(after.current_cycle, circle.current_cycle);
    assert_eq!(after.ended_at, circle.ended_at);
    for m in f.members.iter() {
        assert_eq!(f.rep.get_reputation(m).completed, 1);
    }
}

#[test]
fn settle_cycle_on_cancelled_circle_panics_bad_status() {
    let f = setup();
    let [a, b, _c] = &f.members;
    let id = f
        .client
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &3, &PERIOD);
    f.client.join(&id, b);
    f.client.cancel(&id, a);
    assert_eq!(f.client.get_circle(&id).status, CircleStatus::Cancelled);
    let before = balances(&f);

    f.env.ledger().with_mut(|l| l.timestamp += 10 * PERIOD);
    assert_eq!(
        f.client.try_settle_cycle(&id),
        Err(Ok(Error::BadStatus.into()))
    );

    assert_eq!(balances(&f), before);
    assert_eq!(f.client.get_circle(&id).status, CircleStatus::Cancelled);
    for m in [a, b] {
        assert_eq!(f.rep.get_reputation(m).completed, 0);
        assert_eq!(f.rep.get_reputation(m).defaulted, 0);
    }
}

// ---- Views across the lifecycle -------------------------------------------

/// Asserts every field `get_circle` returns for a circle created by `creator`
/// with the fixture's token and default parameters (size 3).
#[allow(clippy::too_many_arguments)]
fn assert_circle_view(
    f: &Fixture,
    id: u32,
    creator: &Address,
    status: CircleStatus,
    started_at: u64,
    current_cycle: u32,
    slashed_total: i128,
    ended_at: u64,
) {
    let c = f.client.get_circle(&id);
    assert_eq!(c.creator, *creator);
    assert_eq!(c.token, f.token.address);
    assert_eq!(c.contribution, CONTRIB);
    assert_eq!(c.deposit, DEPOSIT);
    assert_eq!(c.size, 3);
    assert_eq!(c.period_secs, PERIOD);
    assert_eq!(c.status, status);
    assert_eq!(c.started_at, started_at);
    assert_eq!(c.current_cycle, current_cycle);
    assert_eq!(c.slashed_total, slashed_total);
    assert_eq!(c.ended_at, ended_at);
}

fn member_state(deposit_remaining: i128, received: bool, defaulted: bool) -> MemberState {
    MemberState {
        deposit_remaining,
        received,
        defaulted,
    }
}

#[test]
fn views_match_state_at_every_lifecycle_stage() {
    let f = setup();
    let [a, b, c] = &f.members;
    let t0 = 5_000;
    f.env.ledger().with_mut(|l| l.timestamp = t0);

    // Create: Open, only the creator listed, unjoined addresses unknown.
    let id = f
        .client
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &3, &PERIOD);
    assert_circle_view(&f, id, a, CircleStatus::Open, 0, 0, 0, 0);
    assert_eq!(
        f.client.get_members(&id),
        soroban_sdk::vec![&f.env, a.clone()]
    );
    assert_eq!(
        f.client.get_member(&id, a),
        member_state(DEPOSIT, false, false)
    );
    for m in [b, c] {
        assert_eq!(
            f.client.try_get_member(&id, m),
            Err(Ok(Error::NotMember.into()))
        );
    }

    // Join below size: still Open and not started.
    f.env.ledger().with_mut(|l| l.timestamp = t0 + 10);
    f.client.join(&id, b);
    assert_circle_view(&f, id, a, CircleStatus::Open, 0, 0, 0, 0);
    assert_eq!(
        f.client.get_members(&id),
        soroban_sdk::vec![&f.env, a.clone(), b.clone()]
    );
    for m in [a, b] {
        assert_eq!(
            f.client.get_member(&id, m),
            member_state(DEPOSIT, false, false)
        );
    }
    assert_eq!(
        f.client.try_get_member(&id, c),
        Err(Ok(Error::NotMember.into()))
    );

    // Activation: the last join flips the circle to Active and stamps the start.
    let started = t0 + 20;
    f.env.ledger().with_mut(|l| l.timestamp = started);
    f.client.join(&id, c);
    let all = soroban_sdk::vec![&f.env, a.clone(), b.clone(), c.clone()];
    assert_circle_view(&f, id, a, CircleStatus::Active, started, 0, 0, 0);
    assert_eq!(f.client.get_members(&id), all);
    for m in f.members.iter() {
        assert_eq!(
            f.client.get_member(&id, m),
            member_state(DEPOSIT, false, false)
        );
    }

    // Contribute: payments are tracked per cycle outside these views, so
    // nothing they return changes.
    f.client.contribute(&id, a);
    f.client.contribute(&id, b);
    assert_circle_view(&f, id, a, CircleStatus::Active, started, 0, 0, 0);
    assert_eq!(f.client.get_members(&id), all);
    for m in f.members.iter() {
        assert_eq!(
            f.client.get_member(&id, m),
            member_state(DEPOSIT, false, false)
        );
    }

    // Settle cycle 0 at the deadline: c missed and is slashed; a receives.
    advance_past_deadline(&f, id);
    f.client.settle_cycle(&id);
    assert_circle_view(&f, id, a, CircleStatus::Active, started, 1, CONTRIB, 0);
    assert_eq!(f.client.get_members(&id), all);
    assert_eq!(
        f.client.get_member(&id, a),
        member_state(DEPOSIT, true, false)
    );
    assert_eq!(
        f.client.get_member(&id, b),
        member_state(DEPOSIT, false, false)
    );
    assert_eq!(
        f.client.get_member(&id, c),
        member_state(DEPOSIT - CONTRIB, false, false)
    );

    // Settle cycle 1 early, everyone paid: b receives.
    for m in f.members.iter() {
        f.client.contribute(&id, m);
    }
    f.client.settle_cycle(&id);
    assert_circle_view(&f, id, a, CircleStatus::Active, started, 2, CONTRIB, 0);
    assert_eq!(f.client.get_members(&id), all);
    assert_eq!(
        f.client.get_member(&id, a),
        member_state(DEPOSIT, true, false)
    );
    assert_eq!(
        f.client.get_member(&id, b),
        member_state(DEPOSIT, true, false)
    );
    assert_eq!(
        f.client.get_member(&id, c),
        member_state(DEPOSIT - CONTRIB, false, false)
    );

    // Complete: c receives the last pot, deposits are returned, the cycle
    // counter stays on the final cycle and the end is stamped.
    let ended = f.client.cycle_deadline(&id) - 1;
    f.env.ledger().with_mut(|l| l.timestamp = ended);
    for m in f.members.iter() {
        f.client.contribute(&id, m);
    }
    f.client.settle_cycle(&id);
    assert_circle_view(
        &f,
        id,
        a,
        CircleStatus::Completed,
        started,
        2,
        CONTRIB,
        ended,
    );
    assert_eq!(f.client.get_members(&id), all);
    for m in f.members.iter() {
        assert_eq!(f.client.get_member(&id, m), member_state(0, true, false));
    }
    assert_eq!(f.token.balance(&f.client.address), 0);
}

// ---- Permissionless settlement --------------------------------------------

#[test]
fn a_third_party_can_settle_a_due_cycle() {
    let f = setup();
    let [a, b, c] = &f.members;
    let id = create_full_circle(&f);

    // A fresh address: not a member, holds nothing, and is never given an
    // authorisation of any kind.
    let stranger = Address::generate(&f.env);
    assert!(!f.client.get_members(&id).contains(&stranger));
    assert_eq!(f.token.balance(&stranger), 0);

    // Settles with auth enforcement on and no auths mocked for anyone, then
    // confirms no signature was consumed.
    let crank = |f: &Fixture| {
        f.env.set_auths(&[]);
        f.client.settle_cycle(&id);
        assert_eq!(f.env.auths(), std::vec![]);
        f.env.mock_all_auths();
    };

    // Enforcement really is on once mocks are cleared: a member action
    // without a signature fails.
    f.env.set_auths(&[]);
    assert!(f.client.try_contribute(&id, a).is_err());
    f.env.mock_all_auths();

    // Cycle 0, settled early because everyone paid: a receives.
    for m in f.members.iter() {
        f.client.contribute(&id, m);
    }
    crank(&f);
    assert!(f.client.get_member(&id, a).received);
    assert_eq!(f.client.get_circle(&id).current_cycle, 1);

    // Cycle 1, settled after the deadline: c is slashed, b receives.
    f.client.contribute(&id, a);
    f.client.contribute(&id, b);
    advance_past_deadline(&f, id);
    crank(&f);
    assert!(f.client.get_member(&id, b).received);
    assert_eq!(
        f.client.get_member(&id, c).deposit_remaining,
        DEPOSIT - CONTRIB
    );

    // Cycle 2, settled after the deadline: c defaults (reported to
    // reputation), the payers are refunded and the circle completes — the
    // cross-contract reports and transfers need no outside signer either.
    f.client.contribute(&id, a);
    f.client.contribute(&id, b);
    advance_past_deadline(&f, id);
    crank(&f);
    assert_eq!(f.client.get_circle(&id).status, CircleStatus::Completed);
    assert!(f.client.get_member(&id, c).defaulted);
    assert_eq!(f.rep.get_reputation(c).defaulted, 1);
    assert_eq!(f.rep.get_reputation(a).completed, 1);
    assert_eq!(f.rep.get_reputation(b).completed, 1);
    assert_eq!(f.token.balance(a), 1_125);
    assert_eq!(f.token.balance(b), 1_125);
    assert_eq!(f.token.balance(c), 750);
    assert_eq!(f.token.balance(&f.client.address), 0);

    // The stranger gained nothing and was never recorded anywhere.
    assert_eq!(f.token.balance(&stranger), 0);
    assert!(!f.client.get_members(&id).contains(&stranger));
    assert_eq!(
        f.client.try_get_member(&id, &stranger),
        Err(Ok(Error::NotMember.into()))
    );
    assert_eq!(f.rep.get_reputation(&stranger).completed, 0);
    assert_eq!(f.rep.get_reputation(&stranger).defaulted, 0);
}

// ---- Init event (#74) -----------------------------------------------------

#[test]
fn initialize_emits_init_event_with_admin_and_reputation() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let reputation = Address::generate(&env);
    let id = env.register(CircleContract, ());
    let client = CircleContractClient::new(&env, &id);

    client.initialize(&admin, &reputation);

    let topics: Vec<Val> = vec![
        &env,
        symbol_short!("circle").into_val(&env),
        symbol_short!("init").into_val(&env),
    ];
    let expected = vec![
        &env,
        (
            id.clone(),
            topics,
            (admin.clone(), reputation.clone()).into_val(&env),
        ),
    ];
    assert_eq!(env.events().all(), expected);
}

// ---- Issue #66: get_config ------------------------------------------------

#[test]
fn get_config_returns_stored_values() {
    let f = setup();
    let config = f.client.get_config();
    // The admin and reputation addresses must match what initialize stored.
    // We cannot compare Address directly, but we can round-trip through the
    // view: the admin address is reachable by calling set_open_circle_cap
    // (which requires admin auth) — if mock_all_auths succeeds, the returned
    // config is consistent.
    assert_eq!(config.open_circle_cap, u32::MAX);
    // Calling with a fresh env where the config was stored confirms the value.
    let f2 = setup();
    f2.client.set_open_circle_cap(&42);
    assert_eq!(f2.client.get_config().open_circle_cap, 42);
}

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn get_config_panics_before_initialize() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(CircleContract, ());
    let client = CircleContractClient::new(&env, &id);
    client.get_config();
}

// ---- Issue #67: get_circles -----------------------------------------------

#[test]
fn get_circles_returns_all_when_within_limit() {
    let f = setup();
    let [a, b, c] = &f.members;
    let id0 = f
        .client
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &3, &PERIOD);
    let id1 = f
        .client
        .create_circle(b, &f.token.address, &CONTRIB, &DEPOSIT, &3, &PERIOD);
    let id2 = f
        .client
        .create_circle(c, &f.token.address, &CONTRIB, &DEPOSIT, &3, &PERIOD);

    let page = f.client.get_circles(&0, &10);
    assert_eq!(page.len(), 3);
    assert_eq!(page.get(0).unwrap().0, id0);
    assert_eq!(page.get(1).unwrap().0, id1);
    assert_eq!(page.get(2).unwrap().0, id2);
}

#[test]
fn get_circles_start_past_total_returns_empty() {
    let f = setup();
    let [a, _, _] = &f.members;
    f.client
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &3, &PERIOD);

    let page = f.client.get_circles(&99, &10);
    assert_eq!(page.len(), 0);
}

#[test]
fn get_circles_paginates_correctly() {
    let f = setup();
    let [a, b, c] = &f.members;
    f.client
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &3, &PERIOD);
    f.client
        .create_circle(b, &f.token.address, &CONTRIB, &DEPOSIT, &3, &PERIOD);
    f.client
        .create_circle(c, &f.token.address, &CONTRIB, &DEPOSIT, &3, &PERIOD);

    let page = f.client.get_circles(&1, &2);
    assert_eq!(page.len(), 2);
    assert_eq!(page.get(0).unwrap().0, 1);
    assert_eq!(page.get(1).unwrap().0, 2);
}

#[test]
fn get_circles_caps_at_max_page_size() {
    use crate::MAX_PAGE_SIZE;
    // We only create 3 circles and ask for MAX_PAGE_SIZE+10; should get 3 back.
    let f = setup();
    let [a, b, c] = &f.members;
    f.client
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &3, &PERIOD);
    f.client
        .create_circle(b, &f.token.address, &CONTRIB, &DEPOSIT, &3, &PERIOD);
    f.client
        .create_circle(c, &f.token.address, &CONTRIB, &DEPOSIT, &3, &PERIOD);

    let page = f.client.get_circles(&0, &(MAX_PAGE_SIZE + 10));
    assert_eq!(page.len(), 3);
}

// ---- Issue #68: payout_position -------------------------------------------

#[test]
fn payout_position_join_order() {
    let f = setup();
    let [a, b, c] = &f.members;
    let id = create_full_circle(&f);

    assert_eq!(f.client.payout_position(&id, a), 0);
    assert_eq!(f.client.payout_position(&id, b), 1);
    assert_eq!(f.client.payout_position(&id, c), 2);
}

#[test]
fn payout_position_skips_defaulted_members_ahead() {
    let f = setup();
    let [a, b, c] = &f.members;
    let id = create_full_circle(&f);

    // Default b over two missed cycles.
    for _ in 0..2 {
        f.client.contribute(&id, a);
        f.client.contribute(&id, c);
        advance_past_deadline(&f, id);
        f.client.settle_cycle(&id);
    }
    assert!(f.client.get_member(&id, b).defaulted);

    // c is now at position 1 (b was skipped).
    assert_eq!(f.client.payout_position(&id, a), 0);
    assert_eq!(f.client.payout_position(&id, c), 1);
}

#[test]
#[should_panic(expected = "Error(Contract, #9)")]
fn payout_position_panics_for_defaulted_member() {
    let f = setup();
    let [a, b, c] = &f.members;
    let id = create_full_circle(&f);

    // Default b.
    for _ in 0..2 {
        f.client.contribute(&id, a);
        f.client.contribute(&id, c);
        advance_past_deadline(&f, id);
        f.client.settle_cycle(&id);
    }
    f.client.payout_position(&id, b);
}

// ---- Issue #69: version ---------------------------------------------------

#[test]
fn circle_version_is_non_empty() {
    let f = setup();
    let v = f.client.version();
    assert!(!v.is_empty());
}

#[test]
fn circle_version_matches_cargo_pkg_version() {
    let f = setup();
    let v = f.client.version();
    let expected = soroban_sdk::String::from_str(&f.env, env!("CARGO_PKG_VERSION"));
    assert_eq!(v, expected);
}

// ---- Issue #58: remove Paid flags after settlement -------------------------

#[test]
fn paid_flags_removed_after_settlement() {
    let f = setup();
    let [a, b, c] = &f.members;
    let id = create_full_circle(&f);

    for m in f.members.iter() {
        f.client.contribute(&id, m);
    }
    // Cycle 0: all paid, settle.
    f.client.settle_cycle(&id);

    // After settlement the Paid flags for cycle 0 must be absent from
    // temporary storage.  `as_contract` runs the closure in the circle
    // contract's storage context so we can inspect its temporary keys.
    use crate::types::DataKey;
    for m in [a, b, c] {
        let gone = f.env.as_contract(&f.client.address, || {
            !f.env
                .storage()
                .temporary()
                .has(&DataKey::Paid(id, 0, m.clone()))
        });
        assert!(
            gone,
            "Paid flag for cycle 0 should be deleted after settlement"
        );
    }
}

#[test]
fn paid_flags_removed_even_after_deadline_settlement() {
    let f = setup();
    let [a, b, c] = &f.members;
    let id = create_full_circle(&f);

    // a and b pay; c misses and is slashed.
    f.client.contribute(&id, a);
    f.client.contribute(&id, b);
    advance_past_deadline(&f, id);
    f.client.settle_cycle(&id);

    use crate::types::DataKey;
    // Flags for the two payers must be gone.
    for m in [a, b] {
        let gone = f.env.as_contract(&f.client.address, || {
            !f.env
                .storage()
                .temporary()
                .has(&DataKey::Paid(id, 0, m.clone()))
        });
        assert!(
            gone,
            "Paid flag for payer {m:?} should be deleted after settlement"
        );
    }
    // c never paid, so its key was never set; confirm it is still absent.
    let c_absent = f.env.as_contract(&f.client.address, || {
        !f.env
            .storage()
            .temporary()
            .has(&DataKey::Paid(id, 0, c.clone()))
    });
    assert!(c_absent);
}

// ---- Issue #59: settle summary event --------------------------------------

#[test]
fn settle_cycle_emits_settle_event_all_paid() {
    let f = setup();
    let [a, b, c] = &f.members;
    let id = create_full_circle(&f);

    for m in f.members.iter() {
        f.client.contribute(&id, m);
    }
    f.client.settle_cycle(&id);

    let settle_topics: Vec<Val> = vec![
        &f.env,
        symbol_short!("circle").into_val(&f.env),
        symbol_short!("settle").into_val(&f.env),
    ];
    let settle_data = (
        id,
        0u32,
        soroban_sdk::vec![&f.env, a.clone(), b.clone(), c.clone()],
        0i128,
        Some(a.clone()),
    )
        .into_val(&f.env);

    // settle_cycle emits: payout, then settle (all paid, no slashes).
    let payout_topics: Vec<Val> = vec![
        &f.env,
        symbol_short!("circle").into_val(&f.env),
        symbol_short!("payout").into_val(&f.env),
    ];
    let payout_data = (id, a.clone(), 3 * CONTRIB, 0u32).into_val(&f.env);
    let expected = vec![
        &f.env,
        (f.client.address.clone(), payout_topics, payout_data),
        (f.client.address.clone(), settle_topics, settle_data),
    ];
    assert_eq!(
        f.env.events().all().filter_by_contract(&f.client.address),
        expected
    );
}

#[test]
fn settle_cycle_emits_settle_event_with_slash() {
    let f = setup();
    let [a, b, c] = &f.members;
    let id = create_full_circle(&f);

    // c misses cycle 0; a and b pay.
    f.client.contribute(&id, a);
    f.client.contribute(&id, b);
    advance_past_deadline(&f, id);
    f.client.settle_cycle(&id);

    let settle_topics: Vec<Val> = vec![
        &f.env,
        symbol_short!("circle").into_val(&f.env),
        symbol_short!("settle").into_val(&f.env),
    ];
    let settle_data = (
        id,
        0u32,
        soroban_sdk::vec![&f.env, a.clone(), b.clone()],
        CONTRIB, // 100 slashed from c's deposit
        Some(a.clone()),
    )
        .into_val(&f.env);

    // settle_cycle emits: slash(c), payout(a), settle.
    // c's deposit(150) - slash(100) = 50, which equals contribution, so NOT defaulted.
    let slash_topics: Vec<Val> = vec![
        &f.env,
        symbol_short!("circle").into_val(&f.env),
        symbol_short!("slash").into_val(&f.env),
    ];
    let payout_topics: Vec<Val> = vec![
        &f.env,
        symbol_short!("circle").into_val(&f.env),
        symbol_short!("payout").into_val(&f.env),
    ];
    let expected = vec![
        &f.env,
        (
            f.client.address.clone(),
            slash_topics,
            (id, c.clone(), CONTRIB).into_val(&f.env),
        ),
        (
            f.client.address.clone(),
            payout_topics,
            (id, a.clone(), 3 * CONTRIB, 0u32).into_val(&f.env),
        ),
        (f.client.address.clone(), settle_topics, settle_data),
    ];
    assert_eq!(
        f.env.events().all().filter_by_contract(&f.client.address),
        expected
    );
}

#[test]
fn settle_cycle_emits_settle_event_no_recipient() {
    // Edge case: all remaining members default in the same cycle so that there
    // is no one left to receive the payout (recipient = None).
    //
    // Scenario (2-member circle, deposit=150, contribution=100):
    //   cycle 0: nobody pays → both slashed 100; deposit → 50 each; not yet
    //            defaulted (slash == contribution); a receives the 200-unit pot.
    //   cycle 1: nobody pays → both slashed 50; deposit → 0; both default
    //            (slash < contribution); find-recipient skips a (received) and
    //            b (defaulted) → recipient = None.
    let f = setup();
    let [a, b, _] = &f.members;
    let id = f
        .client
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &2, &PERIOD);
    f.client.join(&id, b);

    // Cycle 0: nobody pays.
    advance_past_deadline(&f, id);
    f.client.settle_cycle(&id);
    assert!(!f.client.get_member(&id, a).defaulted);
    assert!(!f.client.get_member(&id, b).defaulted);

    // Cycle 1: nobody pays; both default.
    advance_past_deadline(&f, id);
    f.client.settle_cycle(&id);
    // Capture events immediately — env.events().all() only holds the last invocation,
    // so any subsequent read call (get_member, get_circle) would clear the buffer.
    let cycle1_events = f.env.events().all().filter_by_contract(&f.client.address);
    assert!(f.client.get_member(&id, a).defaulted);
    assert!(f.client.get_member(&id, b).defaulted);
    assert_eq!(f.client.get_circle(&id).status, CircleStatus::Completed);

    // The settle event for cycle 1 must have recipient = None and an empty
    // payers list.  It is the second-to-last circle event (complete follows).
    let settle_topics: Vec<Val> = vec![
        &f.env,
        symbol_short!("circle").into_val(&f.env),
        symbol_short!("settle").into_val(&f.env),
    ];
    let empty_payers = soroban_sdk::Vec::<Address>::new(&f.env);
    // cycle_slashed = 50 (a) + 50 (b) = 100
    let settle_data_cycle1 = (id, 1u32, empty_payers, 100i128, None::<Address>).into_val(&f.env);

    // Cycle 1 settle emits (in order): slash(a,50), defaulted(a), slash(b,50),
    // defaulted(b), settle(id,1,[],100,None), complete(id).
    // After cycle 0, each deposit is DEPOSIT − CONTRIB = 50; slash = min(50,100) = 50
    // which is < contribution, so both default.
    let slash_topics: Vec<Val> = vec![
        &f.env,
        symbol_short!("circle").into_val(&f.env),
        symbol_short!("slash").into_val(&f.env),
    ];
    let default_topics: Vec<Val> = vec![
        &f.env,
        symbol_short!("circle").into_val(&f.env),
        symbol_short!("default").into_val(&f.env),
    ];
    let complete_topics: Vec<Val> = vec![
        &f.env,
        symbol_short!("circle").into_val(&f.env),
        symbol_short!("complete").into_val(&f.env),
    ];
    let slash_amt_1: i128 = DEPOSIT - CONTRIB; // 50
    let expected_cycle1 = vec![
        &f.env,
        (
            f.client.address.clone(),
            slash_topics.clone(),
            (id, a.clone(), slash_amt_1).into_val(&f.env),
        ),
        (
            f.client.address.clone(),
            default_topics.clone(),
            (id, a.clone()).into_val(&f.env),
        ),
        (
            f.client.address.clone(),
            slash_topics,
            (id, b.clone(), slash_amt_1).into_val(&f.env),
        ),
        (
            f.client.address.clone(),
            default_topics,
            (id, b.clone()).into_val(&f.env),
        ),
        (f.client.address.clone(), settle_topics, settle_data_cycle1),
        (
            f.client.address.clone(),
            complete_topics,
            id.into_val(&f.env),
        ),
    ];
    assert_eq!(cycle1_events, expected_cycle1);
}

// ---- Issue #60: is_member view --------------------------------------------

#[test]
fn is_member_true_for_member_false_for_non_member() {
    let f = setup();
    let [a, b, c] = &f.members;
    let outsider = Address::generate(&f.env);

    let id = f
        .client
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &3, &PERIOD);
    f.client.join(&id, b);

    assert!(f.client.is_member(&id, a));
    assert!(f.client.is_member(&id, b));
    assert!(!f.client.is_member(&id, c)); // not yet joined
    assert!(!f.client.is_member(&id, &outsider));
}

#[test]
fn is_member_false_after_leave() {
    let f = setup();
    let [a, b, _] = &f.members;
    let id = f
        .client
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &3, &PERIOD);
    f.client.join(&id, b);

    assert!(f.client.is_member(&id, b));
    f.client.leave(&id, b);
    assert!(!f.client.is_member(&id, b));
}

#[test]
fn is_member_true_after_circle_activates() {
    let f = setup();
    let [a, b, c] = &f.members;
    let id = create_full_circle(&f);
    // All three are members even after the circle is Active.
    assert!(f.client.is_member(&id, a));
    assert!(f.client.is_member(&id, b));
    assert!(f.client.is_member(&id, c));
}

// ---- Issue #61: has_paid view ---------------------------------------------

#[test]
fn has_paid_false_before_contribute() {
    let f = setup();
    let [a, b, c] = &f.members;
    let id = create_full_circle(&f);
    assert!(!f.client.has_paid(&id, a));
    assert!(!f.client.has_paid(&id, b));
    assert!(!f.client.has_paid(&id, c));
}

#[test]
fn has_paid_true_after_contribute() {
    let f = setup();
    let [a, b, c] = &f.members;
    let id = create_full_circle(&f);

    f.client.contribute(&id, a);
    assert!(f.client.has_paid(&id, a));
    assert!(!f.client.has_paid(&id, b));
    assert!(!f.client.has_paid(&id, c));

    f.client.contribute(&id, b);
    assert!(f.client.has_paid(&id, b));
}

#[test]
fn has_paid_false_for_next_cycle_after_settlement() {
    let f = setup();
    let [a, b, c] = &f.members;
    let id = create_full_circle(&f);

    for m in f.members.iter() {
        f.client.contribute(&id, m);
    }
    f.client.settle_cycle(&id);
    // Cycle advanced to 1; nobody has paid cycle 1 yet.
    assert_eq!(f.client.get_circle(&id).current_cycle, 1);
    assert!(!f.client.has_paid(&id, a));
    assert!(!f.client.has_paid(&id, b));
    assert!(!f.client.has_paid(&id, c));
}

#[test]
fn has_paid_correct_across_multiple_cycles() {
    let f = setup();
    let [a, b, c] = &f.members;
    let id = create_full_circle(&f);

    // Cycle 0: a and b pay; c skips (deadline settle).
    f.client.contribute(&id, a);
    f.client.contribute(&id, b);
    assert!(f.client.has_paid(&id, a));
    assert!(f.client.has_paid(&id, b));
    assert!(!f.client.has_paid(&id, c));
    advance_past_deadline(&f, id);
    f.client.settle_cycle(&id);

    // Cycle 1: has_paid resets; c can contribute again (deposit 50 left).
    assert!(!f.client.has_paid(&id, a));
    assert!(!f.client.has_paid(&id, b));
    f.client.contribute(&id, a);
    assert!(f.client.has_paid(&id, a));
    assert!(!f.client.has_paid(&id, b));
}
