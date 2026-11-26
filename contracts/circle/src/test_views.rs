#![cfg(test)]

extern crate std;

use crate::test::{advance_past_deadline, create_full_circle, setup, CONTRIB, DEPOSIT, PERIOD};

// ---- get_unpaid ------------------------------------------------------------

#[test]
fn get_unpaid_returns_all_members_before_any_payment() {
    let f = setup();
    let id = create_full_circle(&f);
    let unpaid = f.client.get_unpaid(&id);
    assert_eq!(unpaid.len(), 3);
    for m in f.members.iter() {
        assert!(unpaid.contains(m));
    }
}

#[test]
fn get_unpaid_excludes_members_who_paid() {
    let f = setup();
    let [a, b, c] = &f.members;
    let id = create_full_circle(&f);
    f.client.contribute(&id, a);
    f.client.contribute(&id, b);
    let unpaid = f.client.get_unpaid(&id);
    assert_eq!(unpaid.len(), 1);
    assert!(unpaid.contains(c));
    assert!(!unpaid.contains(a));
    assert!(!unpaid.contains(b));
}

#[test]
fn get_unpaid_empty_when_all_paid() {
    let f = setup();
    let id = create_full_circle(&f);
    for m in f.members.iter() {
        f.client.contribute(&id, m);
    }
    assert_eq!(f.client.get_unpaid(&id).len(), 0);
}

#[test]
fn get_unpaid_excludes_defaulted_members() {
    let f = setup();
    let [a, b, c] = &f.members;
    let id = create_full_circle(&f);
    // Default c over two missed cycles (deposit 150: slashed 100 in cycle 0,
    // then 50 in cycle 1 which exhausts the deposit and marks a default).
    for _ in 0..2 {
        f.client.contribute(&id, a);
        f.client.contribute(&id, b);
        advance_past_deadline(&f, id);
        f.client.settle_cycle(&id);
    }
    assert!(f.client.get_member(&id, c).defaulted);
    // c is defaulted: it must not appear in the unpaid list for cycle 2.
    let unpaid = f.client.get_unpaid(&id);
    assert!(!unpaid.contains(c));
    // a and b have not yet paid cycle 2 and are not defaulted.
    assert!(unpaid.contains(a));
    assert!(unpaid.contains(b));
}

// ---- next_recipient --------------------------------------------------------

#[test]
fn next_recipient_is_first_member_before_any_payout() {
    let f = setup();
    let [a, _b, _c] = &f.members;
    let id = create_full_circle(&f);
    assert_eq!(f.client.next_recipient(&id), Some(a.clone()));
}

#[test]
fn next_recipient_advances_in_join_order() {
    let f = setup();
    let [_a, b, c] = &f.members;
    let id = create_full_circle(&f);
    // Settle cycle 0: a receives.
    for m in f.members.iter() {
        f.client.contribute(&id, m);
    }
    f.client.settle_cycle(&id);
    assert_eq!(f.client.next_recipient(&id), Some(b.clone()));
    // Settle cycle 1: b receives.
    for m in f.members.iter() {
        f.client.contribute(&id, m);
    }
    f.client.settle_cycle(&id);
    assert_eq!(f.client.next_recipient(&id), Some(c.clone()));
}

#[test]
fn next_recipient_skips_defaulted_members() {
    let f = setup();
    let [a, b, c] = &f.members;
    // Zero-deposit circle: a single missed payment defaults the member immediately.
    let id = f
        .client
        .create_circle(a, &f.token.address, &CONTRIB, &0, &3, &PERIOD);
    f.client.join(&id, b);
    f.client.join(&id, c);
    // Cycle 0: a and c pay; b misses and defaults at once (deposit = 0).
    f.client.contribute(&id, a);
    f.client.contribute(&id, c);
    advance_past_deadline(&f, id);
    f.client.settle_cycle(&id);
    // a received the pot; b defaulted without receiving.
    assert!(f.client.get_member(&id, a).received);
    assert!(f.client.get_member(&id, b).defaulted);
    assert!(!f.client.get_member(&id, b).received);
    // next_recipient must skip b and return c.
    assert_eq!(f.client.next_recipient(&id), Some(c.clone()));
}

#[test]
fn next_recipient_is_none_when_all_active_members_received() {
    let f = setup();
    let id = create_full_circle(&f);
    for _ in 0..3 {
        for m in f.members.iter() {
            f.client.contribute(&id, m);
        }
        f.client.settle_cycle(&id);
    }
    // Circle completed; every member has received.
    assert_eq!(f.client.next_recipient(&id), None);
}

// ---- is_due ----------------------------------------------------------------

#[test]
fn is_due_false_before_deadline_with_unpaid_member() {
    let f = setup();
    let [a, b, _c] = &f.members;
    let id = create_full_circle(&f);
    // a and b have paid; c has not — not yet due.
    f.client.contribute(&id, a);
    f.client.contribute(&id, b);
    assert!(!f.client.is_due(&id));
}

#[test]
fn is_due_true_when_all_active_members_have_paid() {
    let f = setup();
    let id = create_full_circle(&f);
    for m in f.members.iter() {
        f.client.contribute(&id, m);
    }
    assert!(f.client.is_due(&id));
}

#[test]
fn is_due_true_after_deadline_even_with_unpaid_members() {
    let f = setup();
    let [a, b, _c] = &f.members;
    let id = create_full_circle(&f);
    f.client.contribute(&id, a);
    f.client.contribute(&id, b);
    advance_past_deadline(&f, id);
    assert!(f.client.is_due(&id));
}

#[test]
fn is_due_false_for_open_circle() {
    let f = setup();
    let [a, _b, _c] = &f.members;
    let id = f
        .client
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &3, &PERIOD);
    assert!(!f.client.is_due(&id));
}

#[test]
fn is_due_false_for_completed_circle() {
    let f = setup();
    let id = create_full_circle(&f);
    for _ in 0..3 {
        for m in f.members.iter() {
            f.client.contribute(&id, m);
        }
        f.client.settle_cycle(&id);
    }
    // Well past the last deadline, but Completed is not Active.
    advance_past_deadline(&f, id);
    assert!(!f.client.is_due(&id));
}

#[test]
fn is_due_ignores_defaulted_members_when_all_active_paid() {
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
    assert!(f.client.get_member(&id, c).defaulted);
    // Cycle 2: only a and b are active. Both pay → due before the deadline.
    f.client.contribute(&id, a);
    f.client.contribute(&id, b);
    assert!(f.client.is_due(&id));
}

// ---- pot_preview -----------------------------------------------------------

#[test]
fn pot_preview_full_payment_equals_size_times_contribution() {
    let f = setup();
    let id = create_full_circle(&f);
    for m in f.members.iter() {
        f.client.contribute(&id, m);
    }
    // 3 × 100 = 300.
    assert_eq!(f.client.pot_preview(&id), 3 * CONTRIB);
}

#[test]
fn pot_preview_includes_deposit_slashes_for_unpaid_members() {
    let f = setup();
    let [a, b, _c] = &f.members;
    let id = create_full_circle(&f);
    // a and b pay; c does not. If settled now, c's slash = min(150, 100) = 100.
    f.client.contribute(&id, a);
    f.client.contribute(&id, b);
    // pot = 100 (a) + 100 (b) + 100 (c slash) = 300.
    assert_eq!(f.client.pot_preview(&id), 3 * CONTRIB);
}

#[test]
fn pot_preview_caps_slash_at_deposit_when_deposit_is_less_than_contribution() {
    let f = setup();
    let [a, b, c] = &f.members;
    // Zero-deposit circle: an unpaid member contributes 0 to the pot.
    let id = f
        .client
        .create_circle(a, &f.token.address, &CONTRIB, &0, &3, &PERIOD);
    f.client.join(&id, b);
    f.client.join(&id, c);
    // Only a and b pay; c's slash = min(0, 100) = 0.
    f.client.contribute(&id, a);
    f.client.contribute(&id, b);
    // pot = 100 + 100 + 0 = 200.
    assert_eq!(f.client.pot_preview(&id), 2 * CONTRIB);
}

#[test]
fn pot_preview_partial_deposit_slash() {
    // After c misses one cycle (deposit 150 → 50), pot_preview for cycle 1
    // should use the remaining 50 as c's slash (50 < 100).
    let f = setup();
    let [a, b, c] = &f.members;
    let id = create_full_circle(&f);
    // Cycle 0: a and b pay; c misses. Deadline triggers; c loses 100.
    f.client.contribute(&id, a);
    f.client.contribute(&id, b);
    advance_past_deadline(&f, id);
    f.client.settle_cycle(&id);
    assert_eq!(
        f.client.get_member(&id, c).deposit_remaining,
        DEPOSIT - CONTRIB
    );
    // Cycle 1: nobody has paid yet; c's remaining deposit is 50.
    // pot_preview = min(150, 100) [a] + min(150, 100) [b] + min(50, 100) [c]
    //             = 100 + 100 + 50 = 250.
    assert_eq!(f.client.pot_preview(&id), 2 * CONTRIB + (DEPOSIT - CONTRIB));
}

#[test]
fn pot_preview_excludes_defaulted_members() {
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
    assert!(f.client.get_member(&id, c).defaulted);
    // Cycle 2: neither a nor b has paid yet; c is excluded.
    // pot_preview = min(150, 100) [a] + min(150, 100) [b] = 200.
    assert_eq!(f.client.pot_preview(&id), 2 * CONTRIB);
}

#[test]
fn pot_preview_zero_when_no_active_members_remain() {
    // Edge: all members defaulted → pot is zero (no one contributes or slashes).
    let f = setup();
    let [a, b, c] = &f.members;
    // Use a zero-deposit circle so every miss is an immediate default.
    let id = f
        .client
        .create_circle(a, &f.token.address, &CONTRIB, &0, &3, &PERIOD);
    f.client.join(&id, b);
    f.client.join(&id, c);
    // Cycle 0: nobody pays. All three default at the deadline.
    advance_past_deadline(&f, id);
    f.client.settle_cycle(&id);
    // All defaulted; contract holds nothing.
    assert!(f.client.get_member(&id, a).defaulted);
    assert!(f.client.get_member(&id, b).defaulted);
    assert!(f.client.get_member(&id, c).defaulted);
    // pot_preview on the now-completed circle returns 0.
    assert_eq!(f.client.pot_preview(&id), 0);
}
