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
    Reporter(Address),   // instance   — bool, is this contract a reporter
    Rep(Address),        // persistent — a member's permanent record
}
```

A member's `Rep` is persistent because its entire purpose is to outlive any single
circle. The admin and the reporter list are instance data, needed on every write.

## TTL extension

```rust
const TTL_THRESHOLD: u32 = 259_200;   // ~15 days at 5s ledgers
const TTL_EXTEND:    u32 = 518_400;   // ~30 days
```

The pattern throughout: when a persistent (or instance) entry is touched and its
remaining TTL has dropped below `TTL_THRESHOLD`, it is bumped back up to
`TTL_EXTEND`. In plain terms — any entry used at least once a fortnight keeps
itself alive with a month of headroom. An abandoned circle that no one touches
will eventually let its entries expire, which is the correct outcome: the network
should not store dead circles forever.

## Rule for contributors

**Every persistent storage write must extend TTL.** A write that does not is a
latent bug — the entry can expire mid-life and take a circle's state with it,
stranding funds. When you add a code path that writes persistent state, route it
through the existing `write_*` helpers, which handle the extension, rather than
touching storage directly.
