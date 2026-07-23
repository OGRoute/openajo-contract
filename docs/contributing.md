# Contributing

This is the smart-contract repo. It holds every escrowed fund in OpenAjo, so the
bar here is deliberately higher than in the app repo, and the review is stricter.

Contributing requires **Rust**, the **`wasm32v1-none`** target, and the **Stellar
CLI**. If you would rather contribute without a Soroban toolchain, the
[app repo](https://github.com/OGRoute/openajo-app) has work that needs only Node.

## Picking work

[Open issues](https://github.com/OGRoute/openajo-contract/issues). Comment to
claim one first.

* `good first issue` — self-contained, clear acceptance criteria
* `needs design` — **agree the approach in the issue before writing code**

## Before opening a pull request

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --target wasm32v1-none --release
```

CI runs exactly these as the **Test, lint, wasm build** job. `main` is protected:
pull request required, one approving review, green CI, linear history. A red PR is
not reviewed until it is green.

## Standards

* `#![no_std]`. No `std`.
* **No `unwrap()` or `expect()` outside tests.** Fail with a typed
  `panic_with_error!` and a `contracterror` variant, so clients get a code they
  can handle.
* **No floating point.** All money is `i128` in raw token units.
* **Every state-changing function calls `require_auth()`** on the acting address —
  the sole intentional exception is `settle_cycle`, which is permissionless by
  design. If you add a function, document who may call it.
* **Every persistent storage write extends TTL.** Route writes through the
  existing `write_*` helpers rather than touching storage directly. See
  [Storage & TTL](reference/storage.md).
* **Every new function ships with tests**, including `#[should_panic]` paths for
  each error it can raise. Balance-changing logic asserts **exact** balances, not
  just "greater than zero".

## Error codes are an API

Error codes are stable and consumed by the SDK, the indexer, and integrators.
Append new variants with the next free number; never renumber or insert in the
middle. See [Errors](reference/errors.md).

## Event shapes are an API

Event topics and data tuples are a compatibility contract with the app repo's
indexer. Changing one breaks every consumer silently. Such a change needs
**coordinated issues in both repos**, and the contract change ships and deploys
first. See [Events](reference/events.md).

## Two things not to "fix" by accident

Both of these look like weaknesses and are actually design decisions. Read the
linked page before touching either.

* **The deposit does not fully collateralize a member.** It prices default;
  reputation handles repeat default. Requiring `deposit >= size × contribution`
  would break the product. → [Deposits, slashing & default](design/deposits-and-default.md)
* **`settle_cycle` has no caller authorization.** It is permissionless on
  purpose; that is what removes the trusted operator. Adding a required caller
  would reintroduce the problem the protocol exists to solve. →
  [Settlement](design/settlement.md)

## Security

Do not open a public issue for a vulnerability. Use GitHub's private vulnerability
reporting on this repository.

Highest priority: authorization bypasses, settlement manipulation, and anything
that strands or leaks escrowed value. OpenAjo is **unaudited and testnet-only** —
do not deploy it with mainnet funds.
