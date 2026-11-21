# Contributing to openajo-contract

Please read the [Code of Conduct](CODE_OF_CONDUCT.md) first. Comment on an
issue to claim it before you start, so two people do not do the same work.

## Setup

```bash
rustup target add wasm32v1-none
cargo test
```

Building the wasm needs the **`stellar` CLI v25.2.0+** as well: soroban-sdk 28
refuses to build a contract through plain `cargo build`.

If `cargo test` fails compiling `soroban-env-host` with an `ed25519-dalek`
trait error, pin it and commit the lockfile:

```bash
cargo update -p ed25519-dalek@3.0.0 --precise 2.2.0
```

## Before opening a PR

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
stellar contract build
```

All four must pass — CI enforces them. With [`just`](https://just.systems)
installed, `just check` runs fmt, clippy, test and the wasm build in one go
(`just --list` shows the rest, including `just deny` for `cargo deny check
licenses bans`).

## Resource cost baseline

`contracts/circle/src/test_cost.rs` runs each entry point at a fixed size (a
three member circle) and compares the CPU instructions and memory the host
metered with `contracts/circle/cost_baseline.txt`. `cargo test` fails when a call
costs more than 10% over its baseline. After an intended change, regenerate the
baseline and commit it with the change:

```bash
UPDATE_COST_BASELINE=1 cargo test -p circle cost_baseline
```

Review the diff of the baseline file: a large rise is worth a note in the PR.

## Deploy script test

`scripts/test-deploy-local.sh` runs `scripts/deploy.sh` against a local
quickstart network and drives a 2-member circle through the CLI. It needs
Docker and the `stellar` CLI and skips (exit 0) without them; CI runs it
with `REQUIRE_LOCAL_NETWORK=1` on PRs touching `scripts/` or `contracts/`.

## Changelog

Update `CHANGELOG.md` in the same PR: add an entry under `[Unreleased]`, and
bump the crate version only on an interface change (see **Versioning**).

## Rules of the codebase

- `#![no_std]`; no `unwrap()`/`expect()` outside tests; no floats — all
  amounts are `i128` raw token units.
- Every public function documents who may call it; every user action calls
  `require_auth()`.
- Every persistent write extends TTL.
- Event shapes are the API contract for the app repo's indexer — changing
  topics or data tuples is a breaking change and needs a coordinated issue in
  `openajo-app`.
- Every new function ships with tests, including `#[should_panic]` error
  paths. Balance-changing logic asserts exact balances.

## Versioning

The crates in `contracts/*/Cargo.toml` (`circle`, `reputation`) carry a version
that tracks the **on-chain interface**, not the implementation:

- **Bump the crate version** (and record the change in `CHANGELOG.md`) when an
  interface changes: function signatures, storage layout/keys, error codes,
  event topics or data tuples, or authorization rules. These are breaking for
  the app repo's SDK, indexer and integrators.
- **Do not bump** for internal changes: refactors, dependency upgrades (such as
  a soroban-sdk bump that changes no interface), test-only changes, CI/docs
  changes, or a redeploy that ships the same interface. Record those under the
  appropriate `CHANGELOG.md` section without a version bump.
- While the crates are pre-1.0 (`0.x`), a minor bump (`0.x` → `0.(x+1)`) may
  be breaking; a patch bump is for non-breaking interface additions only.

Keep the two crates' versions in sync: they are deployed and upgraded together.

## Commits

Conventional format: `type(scope): description` (scopes: `circle`,
`reputation`, `ci`, `docs`). One logical unit per commit.
