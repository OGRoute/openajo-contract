# Build and test

## Prerequisites

* **Rust** (stable) with the `wasm32v1-none` target
* **Stellar CLI** (`stellar`) **v25.2.0 or newer**, for building and deploying

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

30 tests across the two contracts. They run against Soroban's test host, not a
real network, so they are fast and need no deployment. Every test that touches
funds asserts **exact** balances — for example
`insufficient_deposit_marks_default_and_skips_rotation` checks each member's and
the contract's balance to the token unit after a default.

They are organised by what they protect:

| Module | What it covers |
| --- | --- |
| `circle::test` | the lifecycle, slashing, defaults, refunds and every error path |
| `circle::test_invariants` | value conservation and escrow composition after **every** state change |
| `circle::test_reputation` | reputation accumulating across circles, through the real contracts |
| `reputation::test` | reporter authorization, and that revocation never rewrites history |

## Build the wasm

```bash
stellar contract build
```

Outputs `target/wasm32v1-none/release/circle.wasm` and `reputation.wasm`, ready to
deploy.

soroban-sdk 28 builds contracts **through the CLI**, not through `cargo build`.
Its build script stops a plain `cargo build --target wasm32v1-none` with:

```
error: soroban-sdk requires stellar-cli v25.2.0+ to build a contract
```

`stellar contract build` wraps the same cargo invocation and sets what the SDK
needs, then prints the exported function list — a quick check that the contract
surface is what you expect. `cargo test`, `clippy` and `fmt` are unaffected and
still run directly.

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
exact wasm subset Soroban accepts. The repo pins it, and `stellar contract build`
uses it, so you only hit this if you override the target by hand.

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
stellar contract build
```

`-D warnings` means clippy warnings fail the build. Run the same four locally
before opening a PR and CI holds no surprises.
