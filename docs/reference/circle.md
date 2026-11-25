# `circle`

The lifecycle contract. One instance manages many circles, each a `u32` id.

Every function that moves funds or changes state requires `require_auth()` from
the acting address, except `settle_cycle`, which is permissionless by design.

## Admin

### `initialize(admin, reputation)`

Callable once, by anyone. Stores the admin address and the `reputation` contract
id this instance reports to, and emits a `circle`/`init` event with both. Calling
it twice reverts with `AlreadyInitialized`.

Deploy wiring calls this immediately after deployment — see
[Deploy to testnet](../building/deploy.md).

## Creating and joining

### `create_circle(creator, token, contribution, deposit, size, period_secs) -> u32`

Creates an `Open` circle, adds the creator as member 0, and escrows the creator's
deposit in the same transaction. Uses a zero-second grace period and returns the
new circle id.

### `create_circle_with_grace(creator, token, contribution, deposit, size, period_secs, grace_secs) -> u32`

Creates a circle as above, with a per-circle grace period in seconds. Unpaid
members cannot be slashed until the ledger timestamp is strictly later than
`cycle_deadline + grace_secs`. Fully paid cycles may still settle immediately.
Members may contribute during the grace period until settlement occurs.

Requires `creator` auth. Reverts `BadParams` unless:

| Parameter | Rule |
| --- | --- |
| `contribution` | `> 0`, raw token units |
| `deposit` | `>= 0` (may be `0`; see [deposits](../design/deposits-and-default.md)) |
| `size` | `>= 2` |
| `period_secs` | `>= MIN_PERIOD_SECS` (`3600`, one hour) |
| `grace_secs` | Any `u64` value for `create_circle_with_grace`; `0` for `create_circle` |

`token` is the SAC address of the asset the circle runs in.

> ℹ️ **Note**
>
> There is currently no upper bound on `size`. A very large circle can exceed the
> settlement resource budget — [issue #1](https://github.com/OGRoute/openajo-contract/issues/1)
> and [issue #2](https://github.com/OGRoute/openajo-contract/issues/2).

### `join(circle_id, member)`

Escrows `member`'s deposit and appends them to the join order. Requires `member`
auth. **Activates the circle automatically** when it reaches `size`.

Reverts: `NotFound`, `BadStatus` (not `Open`), `AlreadyMember`, `CircleFull`.

## Leaving and cancelling (Open circles only)

### `leave(circle_id, member)`

A non-creator member leaves an `Open` circle and is refunded in full. Requires
`member` auth.

Reverts: `NotMember`, `IsCreator` (the creator cancels instead), `BadStatus`.

### `remove_member(circle_id, creator, member)`

The creator removes a member from an `Open` circle. The member's remaining
deposit is refunded in full and their record is deleted (like `leave`), so they
may join again later as a new member at the end of the join order. The remaining
members keep their relative order, so payout order is unchanged. Requires
`creator` auth.

The creator cannot remove themselves — that would orphan the circle, so they must
`cancel` instead.

Reverts: `NotCreator`, `BadStatus` (not `Open`), `IsCreator`, `NotMember`.

### `cancel(circle_id, creator)`

The creator cancels an `Open` circle, refunding every member's deposit. Each
refunded member's `deposit_remaining` is set to `0`, so `get_member` reflects the
refund. Moves the circle to `Cancelled`. Requires `creator` auth.

Reverts: `NotCreator`, `BadStatus`.

## Running the circle (Active)

### `contribute(circle_id, member)`

Pays this cycle's `contribution` into escrow. Requires `member` auth.

Reverts: `BadStatus` (not `Active`), `NotMember`, `Defaulted`, `AlreadyPaid`.

### `settle_cycle(circle_id)`

**Permissionless.** Collects the pot, slashes and defaults members who missed,
pays the next recipient in join order, and either advances the cycle or completes
the circle. See [Settlement](../design/settlement.md) for the full algorithm.

Reverts: `BadStatus` (not `Active`), `NotDue` (cycle not yet due).

Takes no caller address and performs no auth on the invoker — whoever submits it
pays the fee and nothing more. When it reports outcomes to `reputation` it signs
as the circle contract itself and passes the circle id as the report `context`
(see [reputation](reputation.md)).

### `set_open_circle_cap(cap)`

Admin only: sets the cap on the number of `Open` circles a single creator may
have at once. `u32::MAX` means no cap (the initialization default). Requires
`config.admin` auth.

## Authorization, precisely

Every state-changing function authenticates the acting address with
`require_auth()` — the sole intentional exception is permissionless
`settle_cycle`. Each auth entry has the shape
`[signer, { function: { contract_fn { contract_address, function_name, args } }, sub_invocations: [...] }]`;
inner calls (token transfers, reports to `reputation`) appear as nested
sub-invocations under the signer's entry.

| Function | Signs with `require_auth` | Auth tree shape |
| --- | --- | --- |
| `initialize(admin, reputation)` | nobody — open to anyone | none |
| `create_circle(creator, …)` | `creator` | `[creator, { contract_fn { this, `create_circle`, [...] } }]`; plus a token `transfer` sub-invocation under `creator` when `deposit > 0` |
| `create_circle_with_grace(creator, …)` | `creator` | Same as `create_circle`, with `grace_secs` included in the function arguments |
| `join(circle_id, member)` | `member` | `[member, { contract_fn { this, `join`, [...] } }]`; plus a token `transfer` sub-invocation under `member` when `deposit > 0` |
| `leave(circle_id, member)` | `member` (not the creator) | `[member, { contract_fn { this, `leave`, [...] } }]`; plus a refund transfer sub-invocation |
| `remove_member(circle_id, creator, member)` | `creator` | `[creator, { contract_fn { this, `remove_member`, [...] } }]`; plus a refund transfer sub-invocation |
| `cancel(circle_id, creator)` | `creator` | `[creator, { contract_fn { this, `cancel`, [...] } }]`; plus one refund transfer sub-invocation per member |
| `contribute(circle_id, member)` | `member` | `[member, { contract_fn { this, `contribute`, [...] } }]`; plus a token `transfer` sub-invocation |
| `settle_cycle(circle_id)` | nobody — permissionless | none; the invoker only pays the fee |
| `set_open_circle_cap(cap)` | `config.admin` | `[admin, { contract_fn { this, `set_open_circle_cap`, [cap] } }]` |
| `get_*` views | none — read-only simulations | none |

`settle_cycle` also invokes `reputation::report_completion` /
`reputation::report_default` as `this` (the circle contract), so those
cross-contract calls appear as
`[circle_id, { contract_fn { reputation, ... } }]` entries; `reputation`
verifies `reporter.require_auth()` — satisfied by contract auth — and its
reporter allow-list.

The committed `test_snapshots/` files record the exact auth trees the test host
emits for each test; the auth assertion tests
(`unauthorized_reporter_panics`, `revoked_reporter_panics`,
`non_admin_set_reporter_panics`, `non_creator_cancel_panics` and the
`#should_panic` guard tests) are the source of truth for who must sign.

## Views

Read-only. In practice these are called as RPC simulations by the SDK and
indexer.

| Function | Returns |
| --- | --- |
| `get_circle(circle_id) -> Circle` | Full circle record. Reverts `NotFound`. |
| `get_members(circle_id) -> Vec<Address>` | Members in join order (= payout order). |
| `get_member(circle_id, member) -> MemberState` | `deposit_remaining`, `received`, `defaulted`. |
| `cycle_deadline(circle_id) -> u64` | Unix seconds by which the current cycle must settle. |
| `total_circles() -> u32` | Count of circles ever created. Ids are `0 .. total-1`. |

## Types

```rust
enum CircleStatus { Open, Active, Completed, Cancelled }

struct Circle {
    creator: Address,
    token: Address,        // SAC
    contribution: i128,    // raw units, > 0
    deposit: i128,         // raw units, >= 0
    size: u32,             // >= 2
    period_secs: u64,      // >= MIN_PERIOD_SECS (3600)
    status: CircleStatus,
    started_at: u64,       // 0 while Open
    current_cycle: u32,    // 0-based
    slashed_total: i128,   // raw units, sum of all slashes in settle_cycle
    ended_at: u64,         // set on completion or cancel; 0 until then
}

struct MemberState {
    deposit_remaining: i128,
    received: bool,
    defaulted: bool,
}
```

All amounts are `i128` raw token units. There is no floating point anywhere in the
contract. See [Errors](errors.md) and [Events](events.md) for the full tables.
