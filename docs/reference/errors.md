# Errors

Both contracts use `#[contracterror]` enums with explicit `u32` codes. A failed
call surfaces to clients as `Error(Contract, #N)`, where `N` is the code below.
The [SDK](https://github.com/OGRoute/openajo-app) decodes these into typed values
and human messages — branch on the code, never on the message text.

Codes are **stable**. Do not renumber them; the SDK, indexer, and any integrator
map against these numbers.

## `circle`

| Code | Name | Raised when |
| --- | --- | --- |
| 1 | `NotInitialized` | A call is made before `initialize` |
| 2 | `AlreadyInitialized` | `initialize` is called a second time |
| 3 | `NotFound` | The circle id does not exist |
| 4 | `BadStatus` | The circle is in the wrong state for the action (e.g. joining a non-`Open` circle, contributing to a non-`Active` one) |
| 5 | `AlreadyMember` | The address is already in the circle |
| 6 | `NotMember` | The address is not a member |
| 7 | `CircleFull` | The circle already has `size` members |
| 8 | `AlreadyPaid` | The member already contributed this cycle |
| 9 | `Defaulted` | The member has defaulted and cannot contribute |
| 10 | `NotDue` | `settle_cycle` called before the cycle is due |
| 11 | `IsCreator` | The creator tried to `leave` (must `cancel`) |
| 12 | `BadParams` | Invalid `create_circle` parameters |
| 13 | `NotCreator` | A non-creator tried a creator-only action |

## `reputation`

| Code | Name | Raised when |
| --- | --- | --- |
| 1 | `NotInitialized` | A call is made before `initialize` |
| 2 | `AlreadyInitialized` | `initialize` is called a second time |
| 3 | `NotAdmin` | A non-admin called `set_reporter` |
| 4 | `NotReporter` | A non-allow-listed contract tried to report |

## A note for contributors

`NotInitialized` and `AlreadyInitialized` share codes 1 and 2 across both
contracts, which is fine — a client always knows which contract it called. When
adding a new error, append it with the next free number rather than inserting one
in the middle, because inserting shifts every code after it and silently breaks
every consumer. Adding rustdoc that documents which error each function can panic
with is [issue #4](https://github.com/OGRoute/openajo-contract/issues/4).
