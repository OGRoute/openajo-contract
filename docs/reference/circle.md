# `circle`

The lifecycle contract. One instance manages many circles, each a `u32` id.

Every function that moves funds or changes state requires `require_auth()` from
the acting address, except `settle_cycle`, which is permissionless by design.

## Admin

### `initialize(admin, reputation)`

Callable once, by anyone. Stores the admin address and the `reputation` contract
id this instance reports to. Calling it twice reverts with `AlreadyInitialized`.

Deploy wiring calls this immediately after deployment — see
[Deploy to testnet](../building/deploy.md).

## Creating and joining

### `create_circle(creator, token, contribution, deposit, size, period_secs) -> u32`

Creates an `Open` circle, adds the creator as member 0, and escrows the creator's
deposit in the same transaction. Returns the new circle id.

Requires `creator` auth. Reverts `BadParams` unless:

| Parameter | Rule |
| --- | --- |
| `contribution` | `> 0`, raw token units |
| `deposit` | `>= 0` (may be `0`; see [deposits](../design/deposits-and-default.md)) |
| `size` | `>= 2` |
| `period_secs` | `>= 3600` (one hour) |

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

### `cancel(circle_id, creator)`

The creator cancels an `Open` circle, refunding every member's deposit. Moves the
circle to `Cancelled`. Requires `creator` auth.

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
pays the fee and nothing more.

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
    period_secs: u64,      // >= 3600
    status: CircleStatus,
    started_at: u64,       // 0 while Open
    current_cycle: u32,    // 0-based
}

struct MemberState {
    deposit_remaining: i128,
    received: bool,
    defaulted: bool,
}
```

All amounts are `i128` raw token units. There is no floating point anywhere in the
contract. See [Errors](errors.md) and [Events](events.md) for the full tables.
