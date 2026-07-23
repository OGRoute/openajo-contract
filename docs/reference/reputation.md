# `reputation`

A permanent, cross-circle record of how each address's memberships ended. Small
by design — see [Reputation](../design/reputation.md) for the rationale.

Writes are gated by a two-check authorization: the caller must authenticate as
itself **and** be on the admin's reporter allow-list.

## Admin

### `initialize(admin)`

Callable once, by anyone. Stores the admin, who manages the reporter allow-list.
Reverts `AlreadyInitialized` on a second call.

### `set_reporter(admin, reporter, allowed)`

Adds (`allowed = true`) or removes (`allowed = false`) a contract from the reporter
allow-list. Requires `admin` auth; reverts `NotAdmin` otherwise.

At deploy time this is called once to authorize the `circle` contract as a
reporter. It can be called again at any point to revoke that authorization.

## Reporting (allow-listed reporters only)

### `report_completion(reporter, member)`

Increments `member.completed`. Requires `reporter` auth, and `reporter` must be
allow-listed — otherwise `NotReporter`.

Called by `circle` for each non-defaulted member when a circle completes.

### `report_default(reporter, member)`

Increments `member.defaulted`. Same authorization as above.

Called by `circle` the moment a member's deposit cannot cover a missed
contribution.

## View

### `get_reputation(member) -> Reputation`

```rust
struct Reputation {
    completed: u32,   // circles finished cleanly
    defaulted: u32,   // circles defaulted on
}
```

An address with no history returns `{ completed: 0, defaulted: 0 }` rather than
reverting, so callers never special-case an unseen member.

## Authorization, precisely

```rust
reporter.require_auth();                        // authenticate as the reporter
let allowed = storage.get(Reporter(reporter));  // look up the allow-list
if !allowed { panic_with_error!(NotReporter); } // must be listed
```

Both checks are required and independent. A contract that is allow-listed but does
not present auth is rejected; a contract that presents auth but is not listed is
rejected. This is what lets `circle` — and only `circle` — write to a member's
permanent record. Tests: `unauthorized_reporter_panics`,
`revoked_reporter_panics`, `non_admin_set_reporter_panics`.

## Errors

| Code | Name | Meaning |
| --- | --- | --- |
| 1 | `NotInitialized` | Called before `initialize` |
| 2 | `AlreadyInitialized` | `initialize` called twice |
| 3 | `NotAdmin` | Non-admin called `set_reporter` |
| 4 | `NotReporter` | Caller is not an allow-listed reporter |
