# Reputation

The `reputation` contract is small and does one thing: it keeps a permanent,
cross-circle tally of how each address's memberships have ended.

```rust
pub struct Reputation {
    /// Circles finished cleanly (deposit intact through completion).
    pub completed: u32,
    /// Circles defaulted on (deposit could not cover a missed contribution).
    pub defaulted: u32,
}
```

That is the whole state per member. No scores, no decay, no weighting — just two
counts. Anything more opinionated belongs in the application layer, which can read
these numbers and present them however it likes.

## Why it is a separate contract

Reputation is deliberately not baked into `circle`. Keeping it standalone means:

* **It outlives any one circle.** A circle completes and its state can eventually
  expire; a member's record persists independently.
* **It is portable.** A future second product — a different kind of circle, a
  lending pool — can report to and read from the same reputation contract, so a
  member's standing follows them across products, not just across circles.
* **Its trust boundary is explicit.** Only contracts on an allow-list can write
  to it, and that list is visible and managed on its own.

This portability is the answer to the second half of the trust problem. The
deposit prices a *single* default; reputation is what makes *repeat* defaulting
visible to circles full of strangers who have no other way to know.

## Who may write to it

Two layers of authorization, both required:

```rust
reporter.require_auth();                       // 1: caller proved they are the reporter
if !allowed { panic_with_error!(NotReporter) } // 2: reporter is on the allow-list
```

A report is accepted only if the calling contract both **authenticates as itself**
and **appears on the allow-list**. Being on the list is not enough without the
auth, and presenting auth is not enough without being listed. The `circle`
contract is added to this list once, at deploy time, by the admin calling
`set_reporter`.

The admin can revoke a reporter at any time by setting `allowed` to `false`, after
which that contract's reports are rejected. This is covered by the
`revoked_reporter_panics` test.

Reporter flags live in **persistent** storage (the admin stays in instance
storage). Keeping them out of instance means a growing allow-list is not loaded on
every invocation. Each `set_reporter` write extends the flag's TTL, and every
accepted report refreshes the reporting flag too, so an active reporter does not
silently expire. An existing deployment upgrading to this layout must call
`set_reporter` again for each reporter after the upgrade — the old instance
entries are not read by the new code. See [Storage & TTL](../reference/storage.md).

## What gets reported, and when

`circle` calls into `reputation` at exactly two moments, both inside
`settle_cycle`:

| Event | Reported |
| --- | --- |
| A member's deposit cannot cover a missed contribution | `report_default` → `defaulted += 1` |
| A circle completes | `report_completion` for each non-defaulted member → `completed += 1` |

A member who merely missed a contribution their bond could absorb is **not**
reported — only genuine default is. And an unknown address reads back as
`{ completed: 0, defaulted: 0 }` rather than erroring, so callers never have to
special-case "never seen before" (`unknown_member_reads_zero`).

Both report functions take a `context` — the reporting circle's id — and put it
in the outcome event. Counters stay additive, so an outcome can be traced back to
a circle for disputes and audits without changing the aggregate record.

## Permanence, TTL, and restoration

Soroban persistent entries are not infinite: every entry has a TTL, and rent is
paid by extending it. A record that is written once and then never touched would
otherwise be archived after the default ~30 day window, which is not what
"permanent history" should mean.

The contract takes three measures:

1. **A long TTL for records.** `Rep(Address)` entries are extended to the long
   (~175 day) TTL window on every write, not the short ~30 day workflow window.
   A member can be quiet for months and still read back correctly.
2. **Reads refresh the TTL.** `get_reputation` extends an existing record's TTL
   before returning it. Any flow that submits the read as a transaction keeps the
   record alive. A read-only RPC simulation does not persist TTL changes, so this
   is best-effort, not a substitute for restoration.
3. **Archived is not deleted.** If an entry still passes the long window with no
   activity, Soroban archives it rather than deleting it. The history remains in
   the network's archived state and is restored by including the `Rep(member)`
   persistent ledger entry in a `RestoreFootprintOp` (the app's
   `restoreFootprint` RPC call) before reading. The app should treat a zero read
   for an address it believes has history as "possibly archived", attempt a
   restore, then re-read. This is documented here because restoration is a
   transaction-level operation; the contract cannot restore an entry it cannot
   access.

The `reading_a_record_refreshes_its_long_ttl` test writes a record, advances the
ledger until only one ledger of TTL remains, reads it, and then advances past the
original expiry to prove the refreshed record is still readable.

## What it deliberately does not do

Reputation does not gate anything. It does not stop a defaulted member from
joining a new circle, and the `circle` contract does not consult it before
letting someone join. That is a policy choice left to the application and to circle
creators: the chain records the truth, and humans decide what to do with it. A
future version could add optional on-chain gating, but the base layer stays
descriptive rather than prescriptive.
