# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Crate versions in `contracts/*/Cargo.toml` (`circle`, `reputation`) bump only on an
**interface change** (function signatures, storage layout, error codes, event
shapes, authorization rules). Internal changes — refactors, dependency upgrades,
CI/docs changes, or redeployments that ship the same interface — are recorded
here without a version bump.

## [Unreleased]

## [0.1.0] - 2026-10-06

### Added

- Rotating savings (ajo / esusu / adashe) smart contracts on Stellar Soroban:
  `contracts/circle` (full ROSCA lifecycle) and `contracts/reputation`
  (permanent cross-circle history per address).
- Permissionless `settle_cycle` crank; deposit slashing and default handling;
  reporter allow-list wiring between the two contracts.
- Contract events for `circle`/`create`, `join`, `start`, `contrib`, `slash`,
  `default`, `payout`, `complete`, `cancel` as the indexer integration surface.
- Test suite: 30 tests across `circle` and `reputation`, including
  `#[should_panic]` error paths and exact-balance assertions.

### Changed

- Upgraded the workspace to **soroban-sdk 28** (from 22) and redeployed to
  Stellar testnet (protocol 29). The upgrade pulls in `stellar-xdr`/`soroban-env-host`
  28, which fix two open GitHub advisories present in the 22 line. No change to
  contract logic, storage layout, function signatures or event shapes.
- Contracts now build through the Stellar CLI (`stellar contract build`) rather
  than plain `cargo build`; CI installs the CLI and builds with it.
- Token client `to` argument no longer takes a borrow (clippy
  `needless_borrows_for_generic_args`); the `from` argument still does.
- `Events::publish` stays (deprecated in favour of `#[contractevent]`) because
  the macro emits a different wire format that would break the indexer decoder;
  clippy `-D warnings` is satisfied with a narrow module-level allow.

### Deployments

- **soroban-sdk 28 redeploy (testnet)** — 2026-10-06:
  - `CIRCLE_CONTRACT_ID` = `CA6NVGUC5LOZPOR3B266YXCA2TKXF4SH3362S4HRS5RQU53IDIM5F7FU`
  - `REPUTATION_CONTRACT_ID` = `CD465NGKMGF2E6RGGL5DDMG3RZZFDINUR755FH3XRUZLMSQHTEBJWFD6`
  - wasm sha256: `circle` `e3fad637e375f1ddb3360bee239d3a61ce23e8d22a46daf1e592f73f7fb1efd6`,
    `reputation` `117d37f0a95925ef2405444858301016a5ffafe47266b6209a5b34f4e83c133d`
  - Verified byte-identical to this tree via `scripts/verify-deployment.sh`.
- The earlier soroban-sdk 22 deployment (`CCLVOHGH…` / `CDXPH2PY…`) is retired:
  it still exists on testnet but nothing in this project points at it any more.

### Security

- Resolved two GitHub advisories carried by `soroban-sdk` 22's transitive
  `stellar-xdr` 22 / `soroban-env-host` 22 by moving to soroban-sdk 28. Neither
  advisory reached the deployed wasm; see `SECURITY.md`.

## [0.0.1] - 2026-07-21

### Added

- Initial workspace scaffold (`Cargo.toml`, `contracts/circle`, `contracts/reputation`).
- Storage types and error enums for both contracts.
- Reputation: `initialize`, reporter management, outcome reporting with events.
- Circle: storage types, error enum, event helpers matching the indexer contract.
- Circle: full lifecycle — create, join, leave, cancel, contribute, `settle_cycle`,
  and view functions.
- Soroban test snapshots checked in.

### Changed

- First testnet deployment on soroban-sdk 22
  (`CCLVOHGH…` / `CDXPH2PY…`), later retired in 0.1.0.

[Unreleased]: https://github.com/OGRoute/openajo-contract/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/OGRoute/openajo-contract/releases/tag/v0.1.0
[0.0.1]: https://github.com/OGRoute/openajo-contract/releases/tag/v0.0.1
