# Deploy

`scripts/deploy.sh` deploys both contracts to Stellar in the correct
dependency order and wires them together. It targets testnet by default and
supports mainnet with an explicit opt-in. Run it as-is or read it as the
canonical deployment recipe.

```bash
./scripts/deploy.sh
```

## What it does

The order matters, because `circle` needs to know `reputation`'s address, and
`reputation` needs to authorize `circle` as a reporter.

1. **Identity.** Creates and funds a testnet identity (default name `openajo`),
   or funds the existing one. Its address becomes the admin of both contracts.
2. **Build.** `cargo build --target wasm32v1-none --release`.
3. **Deploy `reputation`**, then `initialize` it with the admin.
4. **Deploy `circle`**, then `initialize` it with the admin **and the reputation
   contract id** — this is the link that lets `circle` report outcomes.
5. **Authorize `circle`** as a reporter on `reputation`:
   `set_reporter(admin, circle_id, true)`.

Without step 5, every `report_default` and `report_completion` from `circle`
would revert with `NotReporter`, and settlement would fail the first time a
member defaulted or a circle completed. The wiring is not optional.

## Output

```
==========================================================
 OpenAjo deployed (testnet)
   REPUTATION_CONTRACT_ID=C...
   CIRCLE_CONTRACT_ID=C...
   ADMIN=G...
==========================================================
```

The same values are written to `deployments/<network>.json` (for example
`deployments/testnet.json`), replacing the previous file for that network:

```json
{
  "network": "testnet",
  "circle_contract_id": "C...",
  "reputation_contract_id": "C...",
  "admin": "G...",
  "circle_wasm_hash": "<sha256 of circle.wasm>",
  "reputation_wasm_hash": "<sha256 of reputation.wasm>",
  "deployed_at": "2026-01-01T00:00:00Z"
}
```

The file is only written after the final wiring step succeeds, and it is
renamed into place, so a failed run leaves the previous record untouched. The
wasm hashes are the sha256 of the uploaded bytes, the same hash Stellar uses
for a contract's code. For a contract passed in to resume a run (below), the
script fetches the wasm the network actually runs and hashes that instead; if
the fetch fails it records `null`. Set `DEPLOYMENTS_DIR` to write somewhere
other than `deployments/`.

Commit the file for a shared network such as testnet, so the ids in it are
the source for the README and the app. `scripts/verify-deployment.sh` reads
the ids from `deployments/<network>.json` when `CIRCLE_CONTRACT_ID` and
`REPUTATION_CONTRACT_ID` are not set. Records for the throwaway `local`
network are ignored by git.

Put the two contract ids into the app repo's configuration —
`apps/web/.env.local` and `indexer/.env` — as documented in the
[app configuration guide](https://github.com/OGRoute/openajo-app/blob/main/docs/getting-started/configuration.md).

## Overriding defaults

```bash
STELLAR_IDENTITY=mykey STELLAR_NETWORK=testnet ./scripts/deploy.sh
```

## Resuming a partial deploy

If a run fails partway through, rerunning from scratch would deploy a second
reputation contract. Instead, pass the ids that were already created so those
steps are skipped:

```bash
REPUTATION_CONTRACT_ID=C... ./scripts/deploy.sh
REPUTATION_CONTRACT_ID=C... CIRCLE_CONTRACT_ID=C... ./scripts/deploy.sh
```

## Mainnet

Mainnet is supported, but friendbot does not exist there, so the script never
funds an identity on mainnet — fund `STELLAR_IDENTITY` out of band first.
Because mistakes on mainnet are irreversible, an explicit confirmation is
required and a summary is printed before any transaction is sent:

```bash
CONFIRM_MAINNET=1 STELLAR_IDENTITY=mykey STELLAR_NETWORK=mainnet ./scripts/deploy.sh
```

Run without `CONFIRM_MAINNET=1` and the script exits before sending anything.
These contracts are unaudited — see the [security posture](../contributing.md#security)
before deploying mainnet funds.

## Deriving the token contract

A circle runs in a Stellar Asset Contract. For native XLM on testnet:

```bash
stellar contract id asset --asset native --network testnet
```

That id is what you pass as `token` to `create_circle`, and what the app uses as
`NEXT_PUBLIC_TOKEN_ID`. A production deployment would use a stablecoin SAC such as
USDC instead — a savings circle denominated in a volatile asset undermines the
saving.
