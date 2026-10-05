#![cfg(test)]
//! Cross-circle reputation, exercised through the real `circle` contract.
//!
//! The point of a separate `reputation` contract is that a record outlives the
//! circle that produced it: deposits price default *within* one circle, and
//! reputation is what makes a pattern visible *across* them. The unit tests in
//! the reputation crate check the reporter rules; these check that running
//! actual circles produces an accumulating, per-address record.

use crate::test::{
    advance_past_deadline, create_full_circle, setup, Fixture, CONTRIB, DEPOSIT, PERIOD,
};
use crate::types::CircleStatus;

/// Runs a circle to completion with every member paying every cycle.
fn run_clean_circle(f: &Fixture) -> u32 {
    let id = create_full_circle(f);
    for _cycle in 0..3u32 {
        for m in f.members.iter() {
            f.client.contribute(&id, m);
        }
        f.client.settle_cycle(&id);
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
        f.client.settle_cycle(&first);
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
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &3, &PERIOD);
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
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &3, &PERIOD);
    f.client.join(&id, b);
    f.client.leave(&id, b);

    let rep = f.rep.get_reputation(b);
    assert_eq!(rep.completed, 0);
    assert_eq!(rep.defaulted, 0);
}
