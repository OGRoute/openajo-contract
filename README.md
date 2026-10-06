# OpenAjo — contracts

[![CI](https://github.com/OGRoute/openajo-contract/actions/workflows/ci.yml/badge.svg)](https://github.com/OGRoute/openajo-contract/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Good first issues](https://img.shields.io/github/issues/OGRoute/openajo-contract/good%20first%20issue?label=good%20first%20issues)](https://github.com/OGRoute/openajo-contract/issues?q=is%3Aissue+is%3Aopen+label%3A%22good+first+issue%22)

Rotating savings (ajo / esusu / adashe) on Stellar, enforced by Soroban smart
contracts instead of a human collector.

A group escrows a security deposit and contributes a fixed amount of a Stellar
asset each cycle; the whole pot pays out to each member in join order. Missed
contributions are slashed from the member's deposit. A deposit that cannot
cover a miss marks the member **defaulted**: skipped in rotation, blocked from
contributing, and recorded permanently in the on-chain reputation registry.
Deposits *price* default; reputation makes repeat defaulting visible to every
future circle.

📖 **Contract docs — [`docs/`](docs/)**: design (lifecycle, deposits, settlement,
reputation), a function-by-function reference for both contracts, and
build/deploy guides.
**Protocol docs — [ogroute.gitbook.io](https://ogroute.gitbook.io/ogroute-docs)**:
the product-level view and user guides.

## Contracts

| Contract | Responsibility |
|---|---|
| `contracts/circle` | Full ROSCA lifecycle for many circles: create → join → contribute → settle → complete. Holds all funds. |
| `contracts/reputation` | Permanent cross-circle history per address (completions, defaults), written only by authorized reporter contracts. |

`settle_cycle` is a permissionless crank: anyone can settle a due cycle, so no
trusted operator is required to keep circles moving.

## Deployed (Stellar testnet)

```
CIRCLE_CONTRACT_ID     = CA6NVGUC5LOZPOR3B266YXCA2TKXF4SH3362S4HRS5RQU53IDIM5F7FU
REPUTATION_CONTRACT_ID = CD465NGKMGF2E6RGGL5DDMG3RZZFDINUR755FH3XRUZLMSQHTEBJWFD6
```

Explorer: <https://stellar.expert/explorer/testnet/contract/CA6NVGUC5LOZPOR3B266YXCA2TKXF4SH3362S4HRS5RQU53IDIM5F7FU>

Deployed 2026-10-06 from soroban-sdk 28. The sha256 of each deployed wasm, which
`stellar contract build` reproduces from this tree:

```
circle       e3fad637e375f1ddb3360bee239d3a61ce23e8d22a46daf1e592f73f7fb1efd6
reputation   117d37f0a95925ef2405444858301016a5ffafe47266b6209a5b34f4e83c133d
```

The earlier soroban-sdk 22 deployment (`CCLVOHGH…` / `CDXPH2PY…`) is retired. It
still exists on testnet, but nothing in this project points at it any more.

### Verify the deployment yourself

`deploy.sh` uploads the unoptimized release build, so the deployed wasm is
byte-identical to what this tree builds. Check it without trusting us:

```bash
./scripts/verify-deployment.sh
#   circle: MATCH   e3fad637e375f1dd…  CA6NVGUC…
#   reputation: MATCH   117d37f0a95925ef…  CD465NGK…
```

It only reads the network — no keys, no funds. A mismatch is expected on a
branch that changes contract code, since the deployed ids still run the
previously deployed build. Stellar testnet is also reset periodically, which
wipes the contracts until they are redeployed; the same workflow is available
manually under **Actions → Verify deployment**.

## Quick start

```bash
rustup target add wasm32v1-none
cargo test                                        # 19 tests
stellar contract build                            # wasm artifacts
./scripts/deploy.sh                               # deploy + wire to testnet
```

Requires Rust stable and the `stellar` CLI v25.2.0+ — soroban-sdk 28 builds
contracts through the CLI rather than plain `cargo build`.

## Application layer

The SDK, indexer/API and web app live in the companion repo **openajo-app**.
Contract events (`circle`/`create`, `join`, `start`, `contrib`, `slash`,
`default`, `payout`, `complete`, `cancel`) are the integration surface — the
indexer is built by folding them.

## Contributing

This repo holds every escrowed fund in the system, so the review bar is high
and the issues are scoped for it: each lists acceptance criteria, the functions
involved, and the tests expected. Start with a
[`good first issue`](https://github.com/OGRoute/openajo-contract/issues?q=is%3Aissue+is%3Aopen+label%3A%22good+first+issue%22),
comment to claim it, then follow [CONTRIBUTING.md](CONTRIBUTING.md).

- [Code of Conduct](CODE_OF_CONDUCT.md)
- [Security policy](SECURITY.md) — anything that can move, strand or leak funds is reported privately

## License

MIT — see [LICENSE](LICENSE).
