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

GitHub's advisory database is wider than RustSec's and sometimes flags crates in
`Cargo.lock` that `cargo audit` says nothing about. When that happens, the first
question is whether the crate reaches the **deployed wasm** at all — much of
`Cargo.lock` is the test harness and build-time macros, which never ship:

```bash
cargo tree --target wasm32v1-none -i <crate>   # nothing to print = not in the contract
```

A crate reachable only through `soroban-sdk-macros` is a proc-macro dependency:
it runs on the build host and is not linked into the guest wasm. A crate like
`soroban-env-host` is the environment the contract runs *inside*, pulled in by
`testutils` for tests. Neither can affect a deployed contract, though both still
matter for anyone running the tests or the build.

Advisories that are only fixed in a newer `soroban-sdk` major are a maintainer
decision, not a routine bump: a major upgrade means rebuilding, redeploying to
new contract ids, and updating every repository and document that points at
them. Dependabot is configured not to open those automatically.

## Status

Unaudited. Deployed to **testnet only**. Do not use with mainnet funds until a
professional audit has been completed.
