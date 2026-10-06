# Security Policy

OpenAjo escrows user funds. Treat every finding accordingly.

## Reporting

Do **not** open a public issue for vulnerabilities. Use GitHub's private
vulnerability reporting (Security → Advisories → Report a vulnerability) on
this repository.

Include: the affected contract/function, reproduction steps, and impact.

## Scope of greatest concern

- Auth bypass on any user action (`create_circle`, `join`, `leave`, `cancel`,
  `contribute`)
- `settle_cycle` manipulation: double payout, wrong recipient, slashing more
  than `deposit_remaining`, settlement before due
- Cross-contract reporter spoofing against `reputation`
- Any path that strands or leaks escrowed funds

## Dependency advisories

CI runs `cargo audit` on every push and pull request, and it fails the build on
any RustSec vulnerability. Two unmaintained-crate warnings (`derivative`,
`paste`) do not fail it: both arrive through `soroban-sdk` and nothing here can
drop them.

GitHub's advisory database also reports two advisories against crates in
`Cargo.lock` that RustSec does not carry. Neither reaches the deployed wasm, and
both are fixed only in `soroban-sdk` 23 or later — a major upgrade that requires
redeploying to new contract ids:

| Advisory | Where it lives | Effect on the deployed contracts |
| --- | --- | --- |
| `stellar-xdr` ≤ 25.0.0 — `StringM::from_str` bypasses max-length validation (medium) | reachable only via `soroban-sdk-macros`, a **proc-macro** | None. Proc-macros run on the build host and are not linked into the guest wasm. The macro parses this repository's own source, not untrusted input. |
| `soroban-env-host` < 26.0.0 — muxed address/`ScVal` conversions may break after a failed conversion (low) | not in the `wasm32v1-none` build graph at all | None. The host is the environment the contract runs *inside*; it reaches `Cargo.lock` through `testutils` for the test harness only. |

Confirm either claim yourself:

```bash
cargo tree --target wasm32v1-none -i soroban-env-host   # nothing to print
cargo tree --target wasm32v1-none -i stellar-xdr        # only via soroban-sdk-macros (proc-macro)
```

A `soroban-sdk` major upgrade is tracked as a maintainer decision rather than a
routine bump, because it means redeploying and changing the contract ids every
other repository points at. Dependabot is configured not to open it
automatically.

## Status

Unaudited. Deployed to **testnet only**. Do not use with mainnet funds until a
professional audit has been completed.
