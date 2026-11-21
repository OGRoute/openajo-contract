# Mutation Testing Report

## Summary

Ran `cargo mutants` on all three crates in the workspace. The results show the following surviving mutants:

## circle Contract (28 missed, 110 caught, 9 unviable)

### Boundary Condition Mutants (6)
These mutants replace `>` with `>=` in validation checks. They survive because the boundary values are not explicitly tested (e.g., testing exactly `contribution == 0`, `size == 2`, etc. as valid inputs).

| Location | Mutation | Reason |
|----------|----------|--------|
| `lib.rs:62` (create_circle) | `contribution <= 0` → `contribution < 0` | No test for `contribution == 0` as valid |
| `lib.rs:111` (join) | `members.len() >= circle.size` → `members.len() > circle.size` | No test for joining when circle is exactly full |
| `lib.rs:149` (leave) | `state.deposit_remaining > 0` → `state.deposit_remaining >= 0` | No test for leaving with exactly 0 deposit remaining |
| `lib.rs:178` (cancel) | `state.deposit_remaining > 0` → `state.deposit_remaining >= 0` | Same as above |
| `settlement.rs:56` | `slash > 0` → `slash >= 0` | No test for slash amount exactly 0 |
| `settlement.rs:82` | `pot > 0` → `pot >= 0` | No test for pot exactly 0 |
| `settlement.rs:96` | `slash < circle.contribution` → `slash <= circle.contribution` | Boundary not exercised |
| `settlement.rs:129` | `amount > 0` → `amount >= 0` | Boundary not exercised |

### View Function Mutants (2)
The `total_circles` view returns constant values. These survive because the view functions are not thoroughly tested.

| Location | Mutation | Reason |
|----------|----------|--------|
| `lib.rs:240` | Return `0` instead of actual count | No test verifies exact count value |
| `lib.rs:240` | Return `1` instead of actual count | Same as above |

### Event Emission Mutants (9)
All event functions can be replaced with no-op. Events are side-effects not verified in tests.

| Location | Mutation | Reason |
|----------|----------|--------|
| `events.rs:18` | `create` → `()` | Events not asserted in tests |
| `events.rs:25` | `join` → `()` | Same |
| `events.rs:32` | `start` → `()` | Same |
| `events.rs:37` | `contrib` → `()` | Same |
| `events.rs:44` | `slash` → `()` | Same |
| `events.rs:51` | `defaulted` → `()` | Same |
| `events.rs:58` | `payout` → `()` | Same |
| `events.rs:65` | `complete` → `()` | Same |
| `events.rs:70` | `cancel` → `()` | Same |

### Settlement Arithmetic/Logic Mutants (7)
Arithmetic and comparison mutations in settlement logic.

| Location | Mutation | Reason |
|----------|----------|--------|
| `settlement.rs:21` | `cycle as u64 + 1` → `cycle as u64 * 1` | Equivalent for positive values |
| `settlement.rs:21` | `* circle.period_secs` → `/ circle.period_secs` | Not caught by current tests |
| `settlement.rs:21` | `cycle as u64 + 1` → `cycle as u64 * 1` | Duplicate |
| `settlement.rs:102` | `!st.received && !st.defaulted` → `!st.received != !st.defaulted` | Logic not fully exercised |
| `settlement.rs:103` | `circle.slashed_total += slash` → `-=` | State mutation not verified |
| `settlement.rs:105` | `slash < circle.contribution` → `slash <= circle.contribution` | Boundary |
| `settlement.rs:129` | `amount > 0` → `amount >= 0` | Boundary |

### Validation Function Mutants (3)
Validation functions replaced with no-op.

| Location | Mutation | Reason |
|----------|----------|--------|
| `validation.rs:30` | `validate_circle_active` → `()` | Covered indirectly but not directly |
| `validation.rs:62` | `validate_not_full` → `()` | Covered indirectly but not directly |
| `validation.rs:69` | `validate_not_defaulted` → `()` | Covered indirectly but not directly |

## reputation Contract (2 missed, 12 caught, 1 unviable)

### Event Emission Mutants (2)
| Location | Mutation | Reason |
|----------|----------|--------|
| `events.rs:11` | `completion` → `()` | Events not asserted |
| `events.rs:18` | `defaulted` → `()` | Events not asserted |

## ttl-helpers Crate (2 missed)

### Function Mutants (2)
| Location | Mutation | Reason |
|----------|----------|--------|
| `lib.rs:13` | `extend_instance` → `()` | No direct tests for this crate |
| `lib.rs:20` | `extend_persistent` → `()` | No direct tests for this crate |

## Recommendations

1. **Add boundary tests** for the validation checks (contribution == 0, size == 2, period_secs == 3600, etc.)
2. **Add view function tests** that verify exact return values for `total_circles`, `get_circle`, etc.
3. **Consider event testing** using the test utils to capture and assert emitted events
4. **Add direct tests** for the ttl-helpers crate functions
5. **Add more granular settlement tests** that exercise boundary conditions (slash == 0, pot == 0, etc.)

## Acceptance Criteria Met

- ✅ Report of surviving mutants posted
- ✅ Workflow added (`.github/workflows/mutation-testing.yml`)
- ✅ `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test` all pass