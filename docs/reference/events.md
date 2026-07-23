# Events

Both contracts publish events on every state change. These are consumed by the
app repo's indexer, and their shapes are a **compatibility contract** between the
two repositories.

{% hint style="warning" %}
Changing a topic pair or a data tuple silently breaks every consumer — the
indexer's decoder returns `null` for a shape it does not recognise rather than
erroring. Any such change requires coordinated issues in both repos, and the
contract change must ship and deploy first. The source carries this warning too.
{% endhint %}

Every event uses a two-symbol topic: a namespace and a name.

## `circle` events

Topic namespace: `circle`.

| Name | Data tuple | Emitted when |
| --- | --- | --- |
| `create` | `(id: u32, creator: Address)` | A circle is created |
| `join` | `(id: u32, member: Address)` | A member joins |
| `start` | `id: u32` | The circle activates (fills to `size`) |
| `contrib` | `(id: u32, member: Address, cycle: u32)` | A member contributes |
| `slash` | `(id: u32, member: Address, amount: i128)` | A deposit is slashed to cover a miss |
| `default` | `(id: u32, member: Address)` | A member's bond ran short and they defaulted |
| `payout` | `(id: u32, recipient: Address, amount: i128, cycle: u32)` | The pot is paid out |
| `complete` | `id: u32` | The circle completes |
| `cancel` | `id: u32` | An open circle is cancelled |

Note that `start`, `complete`, and `cancel` publish a bare `u32`, not a tuple —
they carry only the circle id. The others publish tuples.

## `reputation` events

Topic namespace: `rep`.

| Name | Data | Emitted when |
| --- | --- | --- |
| `complete` | `member: Address` | A completion is recorded |
| `default` | `member: Address` | A default is recorded |

## How the indexer uses them

The indexer does **not** treat these events as the source of truth for state. It
uses them only as a signal for *which* circles changed, then re-reads those
circles from the contract by simulation. So the important thing about an event is
its identity and which circle it names — not any running total you might try to
derive from the data field. A missed event costs the indexer a timeline entry,
never a wrong balance. See the
[indexer documentation](https://github.com/OGRoute/openajo-app/blob/main/docs/architecture/indexer.md).

## Slash and payout amounts

`slash.amount` and `payout.amount` are `i128` raw token units, like every other
amount in the system. A consumer should format them with the token's decimals (7
for the SACs used here) only at display time.
