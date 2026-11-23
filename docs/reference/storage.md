# Storage & TTL

Soroban has three storage durations — instance, persistent, and temporary — and
this contract uses all three, matched to how long each piece of data must
outlive the ledger's automatic expiry.

## `circle` keys

```rust
enum DataKey {
    Config,                     // instance   — GlobalConfig (admin, reputation)
    Count,                      // instance   — u32, total circles ever created
    Circle(u32),                // persistent — one Circle record
    Members(u32),               // persistent — Vec<Address> in join order
    Member(u32, Address),       // persistent — MemberState
    Paid(u32, u32, Address),    // temporary  — bool, contributed for (circle, cycle, member)
}
```

**Instance** holds the two things every call needs — the config and the circle
counter. Instance storage shares the contract's own TTL, so it lives as long as
the contract is in use.

**Persistent** holds everything a circle needs to survive between transactions:
the circle itself, its member list, and each member's state. These must not
expire while a circle is live, so every write extends their TTL.

**Temporary** holds the per-cycle "has this member paid?" flags. This is the right
choice: a `Paid(circle, cycle, member)` fact only matters until that cycle
settles, and settlement reads it during the same active window it was written in.
Letting it expire afterwards is free cleanup rather than a data loss — the pot
has already been collected on the strength of it.

## `reputation` keys

```rust
enum DataKey {
    Admin,               // instance   — the allow-list manager
    Reporter(Address),   // persistent — bool, is this contract a reporter
    Rep(Address),        // persistent — a member's permanent record
}
```

A member's `Rep` is persistent because its entire purpose is to outlive any single
circle. Reporter flags are persistent so a growing allow-list is not loaded on
every invocation; the admin stays in instance storage, where it is needed on the
admin calls. Each `set_reporter` write extends the reporter flag's TTL, and each
accepted report refreshes the flag on read.

> ⚠️ **Upgrading an existing deployment**
>
> Reporter flags previously lived in instance storage. Moving them to persistent
> storage is not an automatic migration: the admin must call
> `set_reporter(admin, reporter, true)` again for every reporter after the upgrade
> (normally only `circle`) before settlement can report. See
> [reputation](reputation.md#storage-and-upgrading-an-existing-deployment).

## TTL extension

```rust
const TTL_THRESHOLD:      u32 = 259_200;   // ~15 days at 5s ledgers
const TTL_EXTEND:         u32 = 518_400;   // ~30 days
const LONG_TTL_THRESHOLD: u32 = 2_592_000; // ~150 days
const LONG_TTL_EXTEND:    u32 = 3_024_000; // ~175 days (under the network max)
```

The pattern throughout: when a persistent (or instance) entry is touched and its
remaining TTL has dropped below its threshold, it is bumped back up to its extend
value. In plain terms — any entry used at least once a fortnight keeps itself
alive with a month of headroom. An abandoned circle that no one touches will
eventually let its entries expire, which is the correct outcome: the network
should not store dead circles forever.

Circle state uses the short window. Reputation records use the **long** window
because they must outlive the activity that produced them, and `get_reputation`
extends an existing record on read. A long TTL is still finite: an entry untouched
for ~175 days is archived (not deleted) and must be restored by the client before
reading. See
[Permanence, TTL and restoration](../design/reputation.md#permanence-ttl-and-restoration).

## Rule for contributors

**Every persistent storage write must extend TTL.** A write that does not is a
latent bug — the entry can expire mid-life and take a circle's state with it,
stranding funds. When you add a code path that writes persistent state, route it
through the existing `write_*` helpers, which handle the extension, rather than
touching storage directly.
