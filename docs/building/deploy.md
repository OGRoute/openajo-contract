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
