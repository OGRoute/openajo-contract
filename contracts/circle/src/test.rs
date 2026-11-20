#![cfg(test)]

use crate::{
    types::{CircleParams, CircleStatus},
    CircleContract, CircleContractClient,
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

pub(crate) fn params(token: &Address, size: u32) -> CircleParams {
    CircleParams {
        token: token.clone(),
        contribution: CONTRIB,
        deposit: DEPOSIT,
        size,
        period_secs: PERIOD,
    }
}

pub(crate) fn create_full_circle(f: &Fixture) -> u32 {
    let [a, b, c] = &f.members;
    let id = f.client.create_circle(a, &params(&f.token.address, 3));
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
    let id = f.client.create_circle(a, &params(&f.token.address, 3));
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
    let id = f.client.create_circle(a, &params(&f.token.address, 3));
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
    let id = f.client.create_circle(a, &params(&f.token.address, 3));
    f.client.join(&id, b);
    f.client.cancel(&id, b);
}

#[test]
#[should_panic]
fn creator_leave_panics() {
    let f = setup();
    let a = &f.members[0];
    let id = f.client.create_circle(a, &params(&f.token.address, 3));
    f.client.leave(&id, a);
}

#[test]
#[should_panic]
fn create_bad_params_panics() {
    let f = setup();
    let a = &f.members[0];
    // size 1 is invalid.
    f.client.create_circle(a, &params(&f.token.address, 1));
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
    assert_eq!(f.client.get_circle(&id).grace_secs, 0);
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

#[test]
fn member_state_new_starts_clean() {
    let s = crate::types::MemberState::new(DEPOSIT);
    assert_eq!(s.deposit_remaining, DEPOSIT);
    assert!(!s.received);
    assert!(!s.defaulted);
}
