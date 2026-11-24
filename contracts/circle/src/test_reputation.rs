#![cfg(test)]
//! Cross-circle reputation, exercised through the real `circle` contract.
//!
//! The point of a separate `reputation` contract is that a record outlives the
//! circle that produced it: deposits price default *within* one circle, and
//! reputation is what makes a pattern visible *across* them. The unit tests in
//! the reputation crate check the reporter rules; these check that running
//! actual circles produces an accumulating, per-address record.

extern crate std;

use soroban_sdk::Address;

use crate::test_utils::{
    advance_past_deadline, create_full_circle, funded_member, setup, Fixture, CONTRIB, DEPOSIT,
    PERIOD,
};
use crate::types::{CircleStatus, MemberState};

/// Runs a circle to completion with every member paying every cycle.
fn run_clean_circle(f: &Fixture) -> u32 {
    let id = create_full_circle(f);
    for _cycle in 0..3u32 {
        for m in f.members.iter() {
            f.client.contribute(&id, m);
        }
        f.client.settle_cycle(&id, &f.members[0]);
    }
    assert_eq!(f.client.get_circle(&id).status, CircleStatus::Completed);
    id
}

#[test]
fn completions_accumulate_across_circles() {
    let f = setup();

    run_clean_circle(&f);
    for m in f.members.iter() {
        assert_eq!(f.rep.get_reputation(m).completed, 1);
    }

    // Same members, a second circle. The record is keyed by address, not by
    // circle, so it adds up rather than resetting.
    run_clean_circle(&f);
    for m in f.members.iter() {
        let rep = f.rep.get_reputation(m);
        assert_eq!(rep.completed, 2);
        assert_eq!(rep.defaulted, 0);
    }
}

#[test]
fn a_default_in_one_circle_survives_a_clean_circle_later() {
    let f = setup();
    let [a, b, c] = &f.members;

    // Circle 1: c misses two cycles and defaults once the deposit runs out.
    let first = create_full_circle(&f);
    for _cycle in 0..2u32 {
        f.client.contribute(&first, a);
        f.client.contribute(&first, b);
        advance_past_deadline(&f, first);
        f.client.settle_cycle(&first, &f.members[0]);
    }
    assert!(f.client.get_member(&first, c).defaulted);
    assert_eq!(f.rep.get_reputation(c).defaulted, 1);
    assert_eq!(f.rep.get_reputation(c).completed, 0);

    // Circle 2: c pays every cycle and completes.
    run_clean_circle(&f);

    // A completion does not erase the default: both counts stand, which is
    // exactly what a circle of strangers needs to see.
    let rep = f.rep.get_reputation(c);
    assert_eq!(rep.completed, 1);
    assert_eq!(rep.defaulted, 1);

    // The members who never missed carry a clean record.
    for m in [a, b] {
        assert_eq!(f.rep.get_reputation(m).defaulted, 0);
    }
}

#[test]
fn a_cancelled_circle_records_nothing() {
    let f = setup();
    let [a, b, _c] = &f.members;

    let id = f
        .client
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &3, &PERIOD, &false);
    f.client.join(&id, b);
    f.client.cancel(&id, a);

    // Cancelling is not an outcome — nobody completed and nobody defaulted.
    for m in f.members.iter() {
        let rep = f.rep.get_reputation(m);
        assert_eq!(rep.completed, 0);
        assert_eq!(rep.defaulted, 0);
    }
}

#[test]
fn leaving_before_activation_records_nothing() {
    let f = setup();
    let [a, b, _c] = &f.members;

    let id = f
        .client
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &3, &PERIOD, &false);
    f.client.join(&id, b);
    f.client.leave(&id, b);

    let rep = f.rep.get_reputation(b);
    assert_eq!(rep.completed, 0);
    assert_eq!(rep.defaulted, 0);
}

// ---- One address in two circles at once -------------------------------------

#[test]
fn a_default_in_one_circle_does_not_leak_into_a_concurrent_one() {
    let f = setup();
    let [a, b, c] = &f.members;

    // Circle A: a, b, c. Circle B: b, c. Both active at the same time, with c
    // (and b) holding a separate member record in each.
    let circle_a = create_full_circle(&f);
    // Set join order: a=cycle0, b=cycle1 for circle_a. a is keeper.
    f.client.set_payout_order(
        &circle_a,
        a,
        &soroban_sdk::vec![&f.env, a.clone(), b.clone(), c.clone()],
    );
    let circle_b =
        f.client
            .create_circle(b, &f.token.address, &CONTRIB, &DEPOSIT, &2, &PERIOD, &false);
    f.client.join(&circle_b, c);
    // Set join order for circle_b: b=cycle0, c=cycle1.
    f.client.set_payout_order(
        &circle_b,
        b,
        &soroban_sdk::vec![&f.env, b.clone(), c.clone()],
    );
    assert_eq!(f.client.get_circle(&circle_a).status, CircleStatus::Active);
    assert_eq!(f.client.get_circle(&circle_b).status, CircleStatus::Active);

    // Cycle 0: c pays in B but skips A.
    f.client.contribute(&circle_b, b);
    f.client.contribute(&circle_b, c);
    f.client.settle_cycle(&circle_b, &f.members[0]);
    f.client.contribute(&circle_a, a);
    f.client.contribute(&circle_a, b);
    advance_past_deadline(&f, circle_a);
    f.client.settle_cycle(&circle_a, &f.members[0]);

    // The slash in A only touches c's record in A.
    let c_in_a = f.client.get_member(&circle_a, c);
    assert_eq!(c_in_a.deposit_remaining, DEPOSIT - CONTRIB);
    assert!(!c_in_a.defaulted);
    assert_eq!(
        f.client.get_member(&circle_b, c),
        MemberState {
            deposit_remaining: DEPOSIT,
            received: false,
            defaulted: false,
        }
    );
    assert_eq!(f.client.get_circle(&circle_b).slashed_total, 0);

    // Cycle 1 in A: c misses again, the remaining deposit cannot cover it, and
    // c defaults. b receives and A completes, while B is still mid-rotation.
    f.client.contribute(&circle_a, a);
    f.client.contribute(&circle_a, b);
    advance_past_deadline(&f, circle_a);
    f.client.settle_cycle(&circle_a, &f.members[0]);
    assert_eq!(
        f.client.get_circle(&circle_a).status,
        CircleStatus::Completed
    );
    assert_eq!(
        f.client.get_member(&circle_a, c),
        MemberState {
            deposit_remaining: 0,
            received: false,
            defaulted: true,
        }
    );
    assert_eq!(f.rep.get_reputation(c).defaulted, 1);
    assert_eq!(f.rep.get_reputation(c).completed, 0);

    // c is still a member in good standing in B.
    let circle = f.client.get_circle(&circle_b);
    assert_eq!(circle.status, CircleStatus::Active);
    assert_eq!(circle.current_cycle, 1);
    assert_eq!(circle.slashed_total, 0);
    assert_eq!(
        f.client.get_member(&circle_b, c),
        MemberState {
            deposit_remaining: DEPOSIT,
            received: false,
            defaulted: false,
        }
    );

    // Cycle 1 in B: c can still contribute there, receives the pot, and B
    // completes with c's deposit returned in full.
    f.client.contribute(&circle_b, b);
    f.client.contribute(&circle_b, c);
    f.client.settle_cycle(&circle_b, &f.members[0]);
    assert_eq!(
        f.client.get_circle(&circle_b).status,
        CircleStatus::Completed
    );
    assert_eq!(f.client.get_circle(&circle_b).slashed_total, 0);
    for m in [b, c] {
        assert_eq!(
            f.client.get_member(&circle_b, m),
            MemberState {
                deposit_remaining: 0,
                received: true,
                defaulted: false,
            }
        );
    }

    // B completing does not rewrite c's record in A.
    assert!(f.client.get_member(&circle_a, c).defaulted);

    // Reputation carries one outcome per circle per address.
    let rep = f.rep.get_reputation(c);
    assert_eq!((rep.completed, rep.defaulted), (1, 1));
    let rep = f.rep.get_reputation(b);
    assert_eq!((rep.completed, rep.defaulted), (2, 0));
    let rep = f.rep.get_reputation(a);
    assert_eq!((rep.completed, rep.defaulted), (1, 0));

    // Keeper a earns +1 per slash settlement in circle_a (2 slashes = +2).
    // a: -150 -200 +299(receive) +1(keeper0) +1(keeper1) +150 = 1_101
    // b: A -150 -200 +249(receive) +150. B -150 -200 +200 +150 = 1_049
    // c: A -150 (slashed in full). B -150 -200 +200 +150 = 850
    assert_eq!(f.token.balance(a), 1_101);
    assert_eq!(f.token.balance(b), 1_049);
    assert_eq!(f.token.balance(c), 850);
    assert_eq!(f.token.balance(&f.client.address), 0);
}

// ---- Exactly one report per member per circle -------------------------------

#[derive(Clone, Copy, Debug)]
enum Outcome {
    Completed,
    Defaulted,
}

/// (completed, defaulted) for each address.
fn records(f: &Fixture, everyone: &[&Address]) -> std::vec::Vec<(u32, u32)> {
    everyone
        .iter()
        .map(|m| {
            let r = f.rep.get_reputation(m);
            (r.completed, r.defaulted)
        })
        .collect()
}

/// Since `before`, each address with `Some(outcome)` moved by exactly one
/// report of that kind; each `None` address is unchanged.
fn assert_reported(
    f: &Fixture,
    everyone: &[&Address],
    before: &[(u32, u32)],
    expected: &[Option<Outcome>],
) {
    let after = records(f, everyone);
    for i in 0..everyone.len() {
        let (c0, d0) = before[i];
        let want = match expected[i] {
            None => (c0, d0),
            Some(Outcome::Completed) => (c0 + 1, d0),
            Some(Outcome::Defaulted) => (c0, d0 + 1),
        };
        assert_eq!(after[i], want, "member #{i}: expected {:?}", expected[i]);
    }
}

#[test]
fn each_member_is_reported_exactly_once_per_circle() {
    use Outcome::{Completed as C, Defaulted as D};

    let f = setup();
    let [a, b, c] = &f.members;
    let d = funded_member(&f);
    let everyone = [a, b, c, &d];

    // Circle 1 — default before receiving. c misses cycles 0 and 1; the
    // second miss exhausts the deposit. a and b complete.
    let before = records(&f, &everyone);
    let id = create_full_circle(&f);
    // Set join order so a=cycle0, b=cycle1, c would be cycle2 (but c defaults).
    f.client.set_payout_order(
        &id,
        a,
        &soroban_sdk::vec![&f.env, a.clone(), b.clone(), c.clone()],
    );
    for _cycle in 0..2u32 {
        f.client.contribute(&id, a);
        f.client.contribute(&id, b);
        advance_past_deadline(&f, id);
        f.client.settle_cycle(&id, &f.members[0]);
        if f.client.get_circle(&id).status == CircleStatus::Active {
            // A slash that leaves the member solvent reports nothing.
            assert_reported(&f, &everyone, &before, &[None; 4]);
        }
    }
    assert_eq!(f.client.get_circle(&id).status, CircleStatus::Completed);
    assert_reported(&f, &everyone, &before, &[Some(C), Some(C), Some(D), None]);

    // Circle 2 — default after receiving. a collects in cycle 0, then misses
    // cycles 1 and 2. a gets a default and no completion.
    let before = records(&f, &everyone);
    let id = create_full_circle(&f);
    // Set join order so a=cycle0, b=cycle1, c=cycle2.
    f.client.set_payout_order(
        &id,
        a,
        &soroban_sdk::vec![&f.env, a.clone(), b.clone(), c.clone()],
    );
    for m in f.members.iter() {
        f.client.contribute(&id, m);
    }
    f.client.settle_cycle(&id, &f.members[0]);
    for _cycle in 1..3u32 {
        f.client.contribute(&id, b);
        f.client.contribute(&id, c);
        advance_past_deadline(&f, id);
        f.client.settle_cycle(&id, &f.members[0]);
    }
    assert_eq!(f.client.get_circle(&id).status, CircleStatus::Completed);
    assert!(f.client.get_member(&id, a).received);
    assert_reported(&f, &everyone, &before, &[Some(D), Some(C), Some(C), None]);

    // Circle 3 — zero deposit, four members. d misses cycle 0 and defaults
    // at once; later settlements skip d and must not report d again.
    let before = records(&f, &everyone);
    let id = f
        .client
        .create_circle(a, &f.token.address, &CONTRIB, &0, &4, &PERIOD, &false);
    f.client.join(&id, b);
    f.client.join(&id, c);
    f.client.join(&id, &d);
    // Set join order so a=cycle0, b=cycle1, c=cycle2, d would be cycle3 (defaults).
    f.client.set_payout_order(
        &id,
        a,
        &soroban_sdk::vec![&f.env, a.clone(), b.clone(), c.clone(), d.clone()],
    );
    for _cycle in 0..3u32 {
        for m in f.members.iter() {
            f.client.contribute(&id, m);
        }
        advance_past_deadline(&f, id);
        f.client.settle_cycle(&id, &f.members[0]);
        if f.client.get_circle(&id).status == CircleStatus::Active {
            assert_reported(&f, &everyone, &before, &[None, None, None, Some(D)]);
        }
    }
    assert_eq!(f.client.get_circle(&id).status, CircleStatus::Completed);
    assert_reported(
        &f,
        &everyone,
        &before,
        &[Some(C), Some(C), Some(C), Some(D)],
    );

    // Circle 4 — defaults leave nobody to pay. Deposit 50 < contribution:
    // after a collects in cycle 0, b and c both miss cycle 1 and default in
    // the same settlement, so the pot is refunded to a and the circle ends.
    let before = records(&f, &everyone);
    let id = f
        .client
        .create_circle(a, &f.token.address, &CONTRIB, &50, &3, &PERIOD, &false);
    f.client.join(&id, b);
    f.client.join(&id, c);
    // Set join order so a receives cycle 0.
    f.client.set_payout_order(
        &id,
        a,
        &soroban_sdk::vec![&f.env, a.clone(), b.clone(), c.clone()],
    );
    for m in f.members.iter() {
        f.client.contribute(&id, m);
    }
    f.client.settle_cycle(&id, &f.members[0]);
    assert_reported(&f, &everyone, &before, &[None; 4]);
    f.client.contribute(&id, a);
    advance_past_deadline(&f, id);
    f.client.settle_cycle(&id, &f.members[0]);
    assert_eq!(f.client.get_circle(&id).status, CircleStatus::Completed);
    assert_reported(&f, &everyone, &before, &[Some(C), Some(D), Some(D), None]);

    // Cumulative: four circles, one outcome each for every participant.
    assert_eq!(
        records(&f, &everyone),
        std::vec![(3, 1), (3, 1), (2, 2), (0, 1)]
    );
    assert_eq!(f.token.balance(&f.client.address), 0);
}
