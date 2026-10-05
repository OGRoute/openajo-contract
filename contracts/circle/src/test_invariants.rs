#![cfg(test)]
//! Value-conservation invariants.
//!
//! The lifecycle tests in `test` assert exact balances at specific points.
//! These assert the properties that must hold *after every state change*, so a
//! future change to settlement cannot quietly mint, burn or strand value:
//!
//! 1. Tokens are conserved: members' balances plus the contract's escrow
//!    always equal what was minted. The contract has no mint authority, so any
//!    drift means value was created or destroyed.
//! 2. Between cycles the contract escrows exactly the members' remaining
//!    deposits — every contribution collected has been paid out as the pot.
//! 3. A terminal circle (completed or cancelled) leaves the contract empty.

use crate::test::{
    advance_past_deadline, create_full_circle, setup, Fixture, CONTRIB, DEPOSIT, PERIOD,
};

/// Total tokens held by the three members in the fixture.
fn circulating(f: &Fixture) -> i128 {
    f.members.iter().map(|m| f.token.balance(m)).sum()
}

/// Tokens escrowed by the circle contract.
fn escrowed(f: &Fixture) -> i128 {
    f.token.balance(&f.client.address)
}

/// Invariant 1: nothing is minted or burned by any contract call.
fn assert_conserved(f: &Fixture, minted: i128) {
    assert_eq!(
        circulating(f) + escrowed(f),
        minted,
        "token supply changed: members {} + escrow {} != minted {}",
        circulating(f),
        escrowed(f),
        minted
    );
}

/// Sum of every member's remaining deposit for a circle.
fn deposits_remaining(f: &Fixture, id: u32) -> i128 {
    f.members
        .iter()
        .map(|m| f.client.get_member(&id, m).deposit_remaining)
        .sum()
}

/// Invariant 2: holds only at cycle boundaries, when no contribution for an
/// unsettled cycle is sitting in escrow.
fn assert_escrow_is_only_deposits(f: &Fixture, id: u32) {
    assert_eq!(
        escrowed(f),
        deposits_remaining(f, id),
        "escrow {} does not match remaining deposits {}",
        escrowed(f),
        deposits_remaining(f, id)
    );
}

#[test]
fn supply_is_conserved_at_every_step_of_a_clean_circle() {
    let f = setup();
    let minted = circulating(&f);
    assert_eq!(escrowed(&f), 0);

    let id = create_full_circle(&f);
    assert_conserved(&f, minted);
    assert_escrow_is_only_deposits(&f, id);

    for _cycle in 0..3u32 {
        for m in f.members.iter() {
            f.client.contribute(&id, m);
            assert_conserved(&f, minted);
        }
        f.client.settle_cycle(&id);
        assert_conserved(&f, minted);
    }

    // Completed: deposits returned, escrow empty, everyone whole again.
    assert_eq!(escrowed(&f), 0);
    assert_eq!(circulating(&f), minted);
}

#[test]
fn escrow_equals_remaining_deposits_between_cycles() {
    let f = setup();
    let minted = circulating(&f);
    let id = create_full_circle(&f);

    // Three members, three cycles: one pot of 3 x CONTRIB per cycle.
    for cycle in 0..3u32 {
        for (i, m) in f.members.iter().enumerate() {
            f.client.contribute(&id, m);
            // Mid-cycle the escrow also holds the contributions collected so far.
            assert_eq!(
                escrowed(&f),
                deposits_remaining(&f, id) + CONTRIB * (i as i128 + 1),
                "cycle {cycle}: escrow wrong after {} contributions",
                i + 1
            );
        }
        f.client.settle_cycle(&id);
        if cycle < 2 {
            assert_escrow_is_only_deposits(&f, id);
            assert_eq!(deposits_remaining(&f, id), 3 * DEPOSIT);
        }
        assert_conserved(&f, minted);
    }

    assert_eq!(escrowed(&f), 0);
}

#[test]
fn supply_is_conserved_when_a_member_is_slashed_and_defaults() {
    let f = setup();
    let [a, b, c] = &f.members;
    let minted = circulating(&f);
    let id = create_full_circle(&f);

    // Cycle 0: c misses. The slash covers the shortfall from c's deposit, so
    // the recipient is paid in full without new tokens appearing.
    f.client.contribute(&id, a);
    f.client.contribute(&id, b);
    advance_past_deadline(&f, id);
    f.client.settle_cycle(&id);
    assert_conserved(&f, minted);
    assert_escrow_is_only_deposits(&f, id);
    assert_eq!(
        f.client.get_member(&id, c).deposit_remaining,
        DEPOSIT - CONTRIB
    );

    // Cycle 1: c misses again with too little deposit left, so c defaults.
    f.client.contribute(&id, a);
    f.client.contribute(&id, b);
    advance_past_deadline(&f, id);
    f.client.settle_cycle(&id);
    assert_conserved(&f, minted);

    let st = f.client.get_member(&id, c);
    assert!(st.defaulted);
    assert_eq!(st.deposit_remaining, 0);

    // Terminal state: nothing stranded in the contract, and c's loss equals
    // exactly what the other two gained.
    assert_eq!(escrowed(&f), 0);
    assert_eq!(circulating(&f), minted);
}

#[test]
fn supply_is_conserved_through_leaving_and_cancelling() {
    let f = setup();
    let [a, b, c] = &f.members;
    let minted = circulating(&f);

    let id = f
        .client
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &3, &PERIOD);
    assert_conserved(&f, minted);

    f.client.join(&id, b);
    assert_conserved(&f, minted);
    assert_eq!(escrowed(&f), 2 * DEPOSIT);

    f.client.leave(&id, b);
    assert_conserved(&f, minted);
    assert_eq!(escrowed(&f), DEPOSIT);

    f.client.join(&id, c);
    f.client.cancel(&id, a);
    assert_conserved(&f, minted);
    assert_eq!(escrowed(&f), 0);
    assert_eq!(circulating(&f), minted);
}
