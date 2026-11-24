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

### Dev container

Instead of installing both locally, open the repository in the dev container
(`.devcontainer/`): VS Code's **Dev Containers: Reopen in Container**, a GitHub
Codespace, or `devcontainer up --workspace-folder .` with the Dev Container CLI.
It runs on Linux, macOS and Windows hosts with Docker, natively on amd64 and
arm64, and comes with:

| Tool | Version |
| --- | --- |
| Rust | 1.100.0 (`RUSTUP_TOOLCHAIN`), with `wasm32v1-none`, `rustfmt` and `clippy` |
| Stellar CLI | v28.1.0, the CI version, checked against the release sha256 |

`cargo test`, `cargo clippy --all-targets -- -D warnings` and
`stellar contract build` work in it as soon as it is created. It is a
development environment: the reproducible build below is still what verifies a
deployed wasm.

## Test

```bash
git clone https://github.com/OGRoute/openajo-contract
cd openajo-contract
cargo test
```

34 tests across the two contracts. They run against Soroban's test host, not a
real network, so they are fast and need no deployment. Every test that touches
funds asserts **exact** balances — for example
`insufficient_deposit_marks_default_and_skips_rotation` checks each member's and
the contract's balance to the token unit after a default.

They are organised by what they protect:

| Module | What it covers |
| --- | --- |
| `circle::test` | the lifecycle, slashing, defaults, refunds and every error path |
| `circle::test_invariants` | value conservation and escrow composition after **every** state change, plus proptest-generated circles |
| `circle::test_utils` | shared fixtures (`setup`, `create_full_circle`, ...) imported by the test modules |
| `circle::test_reputation` | reputation accumulating across circles, through the real contracts |
| `reputation::test` | reporter authorization, and that revocation never rewrites history |

## Property-based tests

`circle::test_invariants::generated` uses [proptest](https://docs.rs/proptest)
to generate whole circles (2 to 6 members, contribution, deposit, and a random
pay/miss decision per member per cycle), run them to completion and assert that
token supply is conserved after every call and that no escrow remains at the
end. It runs as part of `cargo test` (48 cases by default).

```bash
PROPTEST_CASES=1000 cargo test -p circle random_circles   # deeper local run
```

A failing case is shrunk to a minimal input and saved to
`contracts/circle/proptest-regressions/`; it is replayed first on every later
run, so the failure stays reproducible. Commit that file with the fix.

## Fuzzing

`fuzz/` is a [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) crate (kept
out of the root workspace, so `cargo test` and `clippy` never build it). The
`circle_sequence` target drives arbitrary sequences of `join`, `leave`,
`cancel`, `contribute` and `settle_cycle` calls and ledger time jumps through
the Soroban test host. It fails on any error that is not a declared contract
`Error`, on any change in total token supply, and on a terminal circle that
still holds escrow.

```bash
cargo install cargo-fuzz        # once; needs a nightly toolchain
cargo +nightly fuzz run circle_sequence -- -max_total_time=60
```

Crashing inputs are written to `fuzz/artifacts/circle_sequence/`; replay one
with `cargo +nightly fuzz run circle_sequence <path>`. A short run
(`-max_total_time=60`) is cheap enough to add as an optional CI job; it is not
enabled by default.

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

## Reproducible build (Docker)

Verifying a deployment means rebuilding the exact wasm that was uploaded.
`rust-toolchain.toml` tracks the `stable` channel, so two machines building at
different times can use different compilers. The `Dockerfile` at the repo root
pins every input in one place:

| Input | Pinned to |
| --- | --- |
| Rust | `rust:1.100.0-slim-bookworm`, by image digest, with `RUSTUP_TOOLCHAIN=1.100.0` overriding the `stable` channel |
| Stellar CLI | v28.1.0 (the CI version), checked against the release tarball's sha256 |
| Platform | `linux/amd64`, also on Apple Silicon (Docker emulates it) |
| Dependencies | `Cargo.lock`, enforced with `stellar contract build --locked` |
| Source path | `/src` inside the container |

Build through it with BuildKit (the default builder since Docker 23):

```bash
docker build --output type=local,dest=out .
cat out/wasm.sha256
```

`out/` then holds `circle.wasm`, `reputation.wasm` and `wasm.sha256`, the
`sha256sum` of both files:

```
<64 hex chars>  circle.wasm
<64 hex chars>  reputation.wasm
```

Two machines that build the same commit this way must print the same two
hashes. To check a deployed contract against them, compare with the hash of the
fetched wasm:

```bash
stellar contract fetch --id <CONTRACT_ID> --network testnet --out-file deployed.wasm
sha256sum deployed.wasm
```

`stellar contract build` also prints each file's `Wasm Hash`, so the hash from
a CI run's **Wasm release build** step can be compared with `out/wasm.sha256`
as well, as long as the runner's stable Rust is the pinned version.

To move to a newer Rust or CLI, update the `FROM` line (tag and digest),
`RUSTUP_TOOLCHAIN`, `STELLAR_CLI_VERSION` and `STELLAR_CLI_SHA256` together in
one commit.

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
