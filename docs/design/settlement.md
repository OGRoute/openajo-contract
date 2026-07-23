# Settlement

`settle_cycle` is the heart of the contract. It collects one cycle's pot, pays one
member, handles anyone who missed, and either advances the circle or completes it.
It is **permissionless** — anyone can call it — which is what removes the trusted
operator from the protocol.

## When a cycle is due

Settlement is allowed once either condition holds:

* **Every active member has paid** this cycle, or
* the **deadline has passed**: `started_at + (cycle + 1) × period_secs`.

If neither is true, the call reverts with `NotDue`. This is what lets an
honest circle settle early the instant everyone has contributed, while still
guaranteeing that a stalling member cannot hold the circle hostage past the
deadline.

## The three passes

Settlement walks the members three times. This is simple and correct, and it is
also the contract's main scaling limit — see the note at the end.

### 1. Collect the pot

For each active (non-defaulted) member:

* **Paid this cycle** → their escrowed contribution joins the pot, and they are
  remembered as a payer.
* **Did not pay** → the contract slashes `min(deposit_remaining, contribution)`
  into the pot, emits `slash`, and if the slash fell short of the contribution,
  marks them `defaulted`, emits `default`, and reports the default to
  `reputation`. See [Deposits, slashing & default](deposits-and-default.md).

### 2. Pay the recipient

The pot goes to the **first member in join order who is active and has not yet
received**:

```rust
for m in members.iter() {
    let st = read_member(&env, circle_id, &m);
    if !st.received && !st.defaulted {
        recipient = Some(m);
        break;
    }
}
```

That member is marked `received` and a `payout` event is emitted. Because the
search always starts from the front of the join order, payouts follow the order
members joined, skipping anyone who has already been paid or has defaulted.

### 3. Advance or complete

If any active member still has not received, the cycle counter increments and the
circle stays `Active`. If every non-defaulted member has now received, the circle
**completes**: each non-defaulted member's remaining deposit is refunded, a
completion is reported to `reputation` for each, and a `complete` event fires.

## The no-recipient edge case

There is one subtle branch. What if this settlement's own defaults eliminate
every remaining unpaid member — so after collecting the pot, there is no one left
to pay?

The contract does not let those funds strand. It refunds each payer their
contribution and splits the slashed remainder equally among them, with any
rounding remainder going to the first payer:

```rust
let slash_total = pot - circle.contribution * (n as i128);
let share = slash_total / (n as i128);
let remainder = slash_total - share * (n as i128);
```

This guarantees the contract never keeps money it cannot assign to someone. It is
a rare path, and — as of now — the one settlement branch the test suite does not
yet exercise. Writing that test is
[issue #3](https://github.com/OGRoute/openajo-contract/issues/3).

## Permissionless, and why it matters

`settle_cycle` takes only a `circle_id`. It has no caller argument and performs no
`require_auth` on who invoked it — whoever submits the transaction pays the fee
and nothing more. Any funded account can settle any due circle.

This is a deliberate and load-bearing property. It means there is no privileged
"operator" account that has to stay online for circles to progress, and nothing
that can stall or censor a payout. The app repo runs an optional crank that calls
this for convenience, but if that crank vanishes, any member can settle their own
circle. Removing this property — making settlement require a specific caller —
would reintroduce exactly the trusted third party the protocol is built to
eliminate.

## Scaling

The three passes each do one storage read per member, so settlement cost grows
linearly with `size`, and Soroban transactions have a hard resource budget. A
large enough circle cannot settle at all, which would strand its funds. Finding
the real maximum and bounding the cost is
[issue #2](https://github.com/OGRoute/openajo-contract/issues/2), labelled
`needs design` because the fix (fewer passes, cached state, or paginated
settlement) needs agreeing before implementation.
