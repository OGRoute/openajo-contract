# Build and test

## Prerequisites

* **Rust** (stable) with the `wasm32v1-none` target
* **Stellar CLI** (`stellar`), for deploying

The `wasm32v1-none` target is pinned in `rust-toolchain.toml`, so `rustup`
installs it automatically when you build inside the repo:

```toml
[toolchain]
channel = "stable"
targets = ["wasm32v1-none"]
```

## Test

```bash
git clone https://github.com/OGRoute/openajo-contract
cd openajo-contract
cargo test
```

19 tests across the two contracts. They run against Soroban's test host, not a
real network, so they are fast and need no deployment. Every test that touches
funds asserts **exact** balances — for example
`insufficient_deposit_marks_default_and_skips_rotation` checks each member's and
the contract's balance to the token unit after a default.

## Build the wasm

```bash
cargo build --target wasm32v1-none --release
```

Outputs `target/wasm32v1-none/release/circle.wasm` and `reputation.wasm`, ready to
deploy.

## Two toolchain traps

Both of these cost real time if you hit them without warning, so they are
documented here and in the repo README.

### Build target must be `wasm32v1-none`

If you build for the more familiar `wasm32-unknown-unknown`, the contract will
build but **fail at upload** with:

```
reference-types not enabled
```

Current Rust emits post-MVP WebAssembly features (reference types, and others) on
`wasm32-unknown-unknown` that the Soroban VM rejects. `wasm32v1-none` targets the
exact wasm subset Soroban accepts. The repo pins it so a plain `cargo build`
inside the project is correct — you only hit this if you override the target by
hand.

### `ed25519-dalek` version conflict

If `cargo test` fails while compiling `soroban-env-host` with a trait error like:

```
the trait bound `ChaCha20Rng: CryptoRng` is not satisfied
```

pin the crate down one minor version:

```bash
cargo update -p ed25519-dalek@3.0.0 --precise 2.2.0
```

The committed `Cargo.lock` already has this pin, so a normal checkout is fine. You
only need the command if you regenerate the lockfile from scratch.

## CI

Every pull request runs, as one job named **Test, lint, wasm build**:

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --target wasm32v1-none --release
```

`-D warnings` means clippy warnings fail the build. Run the same four locally
before opening a PR and CI holds no surprises.
