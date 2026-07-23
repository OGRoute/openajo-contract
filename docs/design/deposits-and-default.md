# Deposits, slashing & default

This is the mechanism that makes the whole protocol work without trust. Read it
carefully — it is subtle, and the subtlety is intentional.

## The deposit is a price, not full collateral

When a member joins, they escrow a flat `deposit`. It is **not** sized to cover
everything they could owe over the life of the circle. From the source, with its
comment preserved:

```rust
/// Flat security bond escrowed at join. >= 0. Deliberately does NOT fully
/// collateralize a member — it prices default; reputation makes repeat
/// defaulting visible. Do not "fix" by requiring deposit >= size*contribution.
pub deposit: i128,
```

Why not require full collateral? Because a bond equal to `size × contribution`
would lock up as much money as the entire circle is worth, which defeats the
point of a savings circle — you would need the whole sum up front to join a group
whose purpose is to help you reach that sum. A smaller flat bond is enough to make
defaulting *cost* something. Making repeat defaulting genuinely expensive is
[reputation](reputation.md)'s job, not the deposit's.

If you take one thing from this page: **do not raise the deposit to full
collateralization to "harden" the contract.** It would break the product while
appearing to improve security.

## Slashing

Every settlement, the contract looks at each active member. If they contributed
this cycle, their contribution goes into the pot. If they did not, the contract
takes it from their deposit instead:

```rust
let slash = min(st.deposit_remaining, circle.contribution);
```

`min` matters. The contract slashes what the deposit can cover, up to the
contribution owed — it never drives a deposit negative. A `slash` event is
emitted for the amount actually taken. The recipient is paid the full pot
regardless of who missed, so a member late in the rotation does not suffer for
someone else's default.

## Becoming defaulted

A member is marked `defaulted` the moment their deposit **cannot** fully cover a
missed contribution:

```rust
if slash < circle.contribution {
    st.defaulted = true;
    events::defaulted(&env, circle_id, &m);
    rep.report_default(&this, &m);
}
```

So default is not "missed a payment" — it is "missed a payment and the bond ran
short." A member with enough deposit to absorb a miss stays in good standing that
cycle; the miss simply eats into their bond. Default happens when the bond is
exhausted.

Once defaulted, a member is:

* **skipped in the payout rotation** — they never receive a pot,
* **blocked from contributing** (`Defaulted` error), and
* **reported to the `reputation` contract**, permanently and across all circles.

Their remaining deposit, if any, is not refunded — it has already been consumed
covering their shortfall.

## What a member ends with

Three outcomes, and the tests assert exact balances for each:

| Outcome | Deposit | Payout | Reputation |
| --- | --- | --- | --- |
| Completed cleanly | refunded in full | received once | `completed += 1` |
| Missed but bond covered it | reduced by the slashes | still received | unchanged |
| Defaulted | consumed, nothing refunded | never received | `defaulted += 1` |

## A deposit below the contribution

`create_circle` permits any `deposit >= 0`, including a deposit smaller than
`contribution` — or zero. Such a circle cannot absorb even a single miss: the
first missed contribution exhausts the bond and defaults the member immediately.

That is a legitimate configuration for a high-trust group that wants the escrow
and rotation guarantees without tying up collateral, but it is a sharp edge.
Whether to enforce a minimum is an open question tracked in
[issue #1](https://github.com/OGRoute/openajo-contract/issues/1).
