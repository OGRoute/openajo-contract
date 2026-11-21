# Settling cycles: running a keeper

`settle_cycle` is permissionless — anyone can call it — but "anyone *can*" is not
the same as "anyone *does*". A circle only moves when somebody actually submits
the transaction. This page is the operational guide for doing that reliably: when
a cycle is due, how to detect it, what it costs, and what can go wrong.

> ℹ️ **Scope**
>
> This is contract-level guidance. The application layer (SDK, indexer, web app)
> lives in the companion repo [openajo-app](https://github.com/OGRoute/openajo-app).
> The on-chain rules themselves are in [Settlement](design/settlement.md); this
> page is about driving them from the outside.

## When a cycle is due

A circle is `Active` from the moment its last member joins. From then on,
settlement is allowed once **either** condition holds:

1. **Every active (non-defaulted) member has paid** this cycle's contribution, or
2. The **deadline has passed**:
   ```
   deadline = started_at + (current_cycle + 1) * period_secs
   ```

If neither is true, `settle_cycle` reverts with `NotDue` (`#10`). This lets an
honest circle settle the instant everyone has contributed, while guaranteeing a
stalling member cannot hold the circle hostage past the deadline.

Notes that matter for a keeper:

* `started_at` is the ledger timestamp captured when the circle activated, and
  `current_cycle` is 0-based. The deadline above is exactly what the contract's
  `cycle_deadline(circle_id)` view returns — prefer calling that over
  recomputing it.
* "Active member" excludes anyone already defaulted. A defaulted member neither
  has to pay nor counts toward "everyone paid".
* A cycle can become due *early* only through contributions. The deadline is the
  only time-based trigger; there is no other clock.

## How to detect a due cycle

The contract does not push notifications, so a keeper has to poll. A reliable
loop looks like this:

1. **Enumerate circles.** `total_circles()` returns the count; ids are
   `0 .. total-1`. New circles appear as the count grows, so re-read it each
   pass rather than caching it.
2. **Filter to `Active`.** `get_circle(id).status` must be `Active`. Skip
   `Open`, `Completed` and `Cancelled` circles — settling them reverts with
   `BadStatus` (`#4`).
3. **Check the deadline.** Compare the current ledger time to
   `cycle_deadline(id)`. If `now >= deadline`, the cycle is due.
4. **Detect early due (optional).** There is no on-chain "paid count" view: paid
   state is temporary storage keyed by `(circle_id, cycle, member)` and is not
   exposed. Two workable approaches:
   * Fold `contrib` events for the circle and count distinct payers for the
     current cycle; when that equals the number of active members, settle.
   * Just attempt `settle_cycle` and treat `NotDue` as "not ready yet". This is
     simpler and is what most keepers do, but see [Cost](#cost) — simulate
     first so you are not paying for failed transactions.
5. **Simulate, then submit.** Run the call as an RPC simulation first. Only
   submit the real transaction if simulation succeeds. This avoids spending fees
   on `NotDue`/`BadStatus` and catches resource-budget failures before they hit
   the network.
6. **Record the outcome.** On success the circle either advances to the next
   cycle or completes; re-read `get_circle` to know which. On failure, log the
   error code and continue to the next circle — never let one bad circle stall
   the whole loop.

A minimal pseudo-loop:

```text
loop:
  n = total_circles()
  for id in 0..n:
    circle = get_circle(id)
    if circle.status != Active: continue
    if now < cycle_deadline(id):
        # may still be early-due via all-paid; try via simulation
    simulate settle_cycle(id)
    if simulation ok: submit settle_cycle(id)
  sleep(period)
```

## Cost

Settlement is **not** subsidized. The caller pays the Stellar transaction fee and
receives nothing in return — there is no keeper reward, bounty or token. Treat
running a keeper as a volunteer/civic function (or as part of operating the app).

Concrete cost drivers:

* **Linear in circle size.** `settle_cycle` walks all members three times, one
  storage read per member per pass. CPU and read-bytes scale with `size`.
* **Hard resource budget.** Soroban transactions have a fixed resource cap. A
  circle large enough to exceed it **cannot settle at all**, which would strand
  its funds. The real maximum size and how to bound the cost are open work
  ([issue #2](https://github.com/OGRoute/openajo-contract/issues/2)). Until that
  is resolved, keep circles modest.
* **Failed transactions still cost fees.** A submitted transaction that reverts
  (e.g. `NotDue`) consumes its fee. Simulate before submitting.
* **Concurrency.** If two keepers settle the same circle in the same ledger, one
  succeeds and the other's transaction fails. Both paid the fee for the attempt.
  Coordinate or just accept occasional wasted fees.

## Failure modes

| Symptom | Cause | What to do |
| --- | --- | --- |
| `NotDue` (`#10`) | Cycle not yet due: not all paid and deadline not passed | Wait; retry after deadline or after more contributions |
| `BadStatus` (`#4`) | Circle is not `Active` (still `Open`, or already `Completed`/`Cancelled`) | Skip it; remove from active set |
| `NotFound` (`#3`) | Unknown circle id | Re-read `total_circles()`; ids never shrink |
| Simulation fails on resource budget | Circle too large for one transaction | Cannot settle as-is; tracked in issue #2 |
| `NotReporter` (reputation, `#4`) | `circle` not on reputation's allow-list | Deployment wiring missing — see [Deploy to testnet](building/deploy.md) step 5 |
| Transaction rejected / fee errors | Underfunded keeper account, stale sequence number, RPC hiccup | Fund the account, retry with fresh sequence, back off |
| State unchanged after a "success" that wasn't | Simulation passed but submission failed or was dropped | Re-simulate and resubmit; settlement is idempotent |
| No recipient after defaults | This settlement defaulted every remaining unpaid member | Contract handles it: contributions refunded, slashed remainder split among payers. No keeper action needed |

Settlement is **atomic**: if it fails, no state changes and no funds move. The
only thing you lose on a failed attempt is the transaction fee. This makes
retrying safe — a keeper can be as aggressive as it likes about retrying without
risk of double-paying, because a successful settle advances `current_cycle` and
the next attempt on the same circle simply returns `NotDue`.

## Idempotency and concurrency

* After a successful settle, `current_cycle` increments (or the circle becomes
  `Completed`). A repeat call on the same circle in the same state returns
  `NotDue` until the new cycle is due.
* Multiple keepers can run against the same network safely. The contract's rules,
  not the caller's identity, determine the outcome.
* A completed circle holds no funds and never re-activates. Stop polling it.

## Reference keeper script

A reference keeper implementation will be published as
[`scripts/keeper.sh`](../scripts/keeper.sh) (or its successor) once it exists;
this guide will link to it directly at that point. Until then, the loop above plus
the contract views in [`circle` reference](reference/circle.md) are sufficient
to build your own.

## See also

* [Settlement](design/settlement.md) — the full three-pass algorithm
* [Deposits, slashing & default](design/deposits-and-default.md) — why misses
  and defaults happen during a settle
* [Events](reference/events.md) — `slash`, `default`, `payout`, `complete`
* [Errors](reference/errors.md) — `NotDue`, `BadStatus`, `NotFound`
