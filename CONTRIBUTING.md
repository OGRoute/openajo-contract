# Contributing to openajo-contract

## Setup

```bash
rustup target add wasm32v1-none
cargo test
```

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
cargo build --target wasm32v1-none --release
```

All four must pass — CI enforces them.

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

## Commits

Conventional format: `type(scope): description` (scopes: `circle`,
`reputation`, `ci`, `docs`). One logical unit per commit.
