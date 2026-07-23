# Deploy to testnet

`scripts/deploy.sh` deploys both contracts to Stellar testnet in the correct
dependency order and wires them together. Run it as-is or read it as the
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

The script only targets testnet networks. **Do not point it at mainnet** — these
contracts are unaudited, and the [security posture](../contributing.md#security)
is testnet-only.

## Deriving the token contract

A circle runs in a Stellar Asset Contract. For native XLM on testnet:

```bash
stellar contract id asset --asset native --network testnet
```

That id is what you pass as `token` to `create_circle`, and what the app uses as
`NEXT_PUBLIC_TOKEN_ID`. A production deployment would use a stablecoin SAC such as
USDC instead — a savings circle denominated in a volatile asset undermines the
saving.
