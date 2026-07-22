# OpenAjo — contracts

Rotating savings (ajo / esusu / adashe) on Stellar, enforced by Soroban smart
contracts instead of a human collector.

A group escrows a security deposit and contributes a fixed amount of a Stellar
asset each cycle; the whole pot pays out to each member in join order. Missed
contributions are slashed from the member's deposit. A deposit that cannot
cover a miss marks the member **defaulted**: skipped in rotation, blocked from
contributing, and recorded permanently in the on-chain reputation registry.
Deposits *price* default; reputation makes repeat defaulting visible to every
future circle.

📖 **[Documentation](https://ogroute.gitbook.io/ogroute-docs)** — protocol
mechanics, contract reference, user guides, and developer setup.

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

## License

MIT — see [LICENSE](LICENSE).
