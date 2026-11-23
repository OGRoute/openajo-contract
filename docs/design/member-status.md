# Decision: keep `received` / `defaulted` bools (no `MemberStatus` enum)

Status: decided, not implemented. Issue #124.

## Question

`MemberState { deposit_remaining, received: bool, defaulted: bool }` allows four
combinations. Should it become an enum
(`Waiting`, `Received`, `Defaulted`, `ReceivedThenDefaulted`)?

## Findings

- All four combinations are reachable and meaningful today. A member who
  received the pot and later defaults is `received && defaulted`
  (`recipient_defaults_after_receiving_loses_deposit`), so there is no invalid
  combination to rule out; the enum would encode the same four states.
- `MemberState` is a `#[contracttype]` struct stored in persistent storage under
  `DataKey::Member(circle, address)` and returned by `get_member`. A struct
  serializes as a map of field names; an enum serializes as a vec of
  discriminant and payload. Replacing the fields changes the XDR of every stored
  member, so already-deployed circles would fail to decode (`get_member`,
  `contribute`, `settle_cycle` all read it). Keeping old data readable would
  need a migration entrypoint walking every circle's members, or a dual-read
  path, in a contract with no upgrade/migration machinery.
- `get_member`'s return type changes, which is a breaking change for the indexer
  in `OGRoute/openajo-app` and needs a coordinated issue there first (not
  opened here).
- The readability gain is small: the loops test `!received && !defaulted` or
  `defaulted`, which `matches!` helpers on the struct would express equally
  well.

## Decision

Do not change the stored type or `get_member`'s return type. If the loops are
judged noisy, add non-storage helpers (`MemberState::is_active()`,
`is_owed_payout()`) in a later change: source-compatible, wire-compatible and
needing no indexer change.

Revisit only together with a planned storage migration and a linked
`openajo-app` issue.
