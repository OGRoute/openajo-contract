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
CIRCLE_CONTRACT_ID     = CCLVOHGHDH32GWFAMCEMVHLNJSF6ENVHERYWU2OHUYWWLAOKLVR3HGKS
REPUTATION_CONTRACT_ID = CDXPH2PYUTRW7GV57X6CJH3E3JOPROSC23NXPMAXOO3EOBI5UTCB2GTQ
```

Explorer: <https://stellar.expert/explorer/testnet/contract/CCLVOHGHDH32GWFAMCEMVHLNJSF6ENVHERYWU2OHUYWWLAOKLVR3HGKS>

## Quick start

```bash
rustup target add wasm32v1-none
cargo test                                        # 19 tests
cargo build --target wasm32v1-none --release      # wasm artifacts
./scripts/deploy.sh                               # deploy + wire to testnet
```

Requires Rust stable and the `stellar` CLI v27+ for deployment.

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
