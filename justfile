# Local mirror of CI. Run `just check` before opening a PR.

default: check

fmt:
    cargo fmt --all -- --check

clippy:
    cargo clippy --all-targets -- -D warnings

test:
    cargo test

# Needs the stellar CLI v25.2.0+ (soroban-sdk 28 refuses plain `cargo build`).
build:
    stellar contract build

# Licences and duplicate dependencies (needs cargo-deny).
deny:
    cargo deny check licenses bans

# fmt, clippy, test and wasm build, as in CI.
check: fmt clippy test build
