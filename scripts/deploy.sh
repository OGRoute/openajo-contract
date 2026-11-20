#!/usr/bin/env bash
# Deploy OpenAjo to Stellar testnet in dependency order:
#   reputation -> circle -> set_reporter wiring.
# Usage: ./scripts/deploy.sh   (override IDENT/NETWORK via env)
#
# Resuming a partial run: if a previous run already deployed a contract, pass
# its id back through the environment (REPUTATION_CONTRACT_ID /
# CIRCLE_CONTRACT_ID) and that step is skipped instead of deploying a duplicate.
set -euo pipefail
cd "$(dirname "$0")/.."

IDENT="${STELLAR_IDENTITY:-openajo}"
NET="${STELLAR_NETWORK:-testnet}"

# Accept already-deployed contract ids so a failed run can be resumed without
# redeploying (and replacing) contracts that already exist on-chain.
REP_ID="${REPUTATION_CONTRACT_ID:-${REP_ID:-}}"
CIRCLE_ID="${CIRCLE_CONTRACT_ID:-${CIRCLE_ID:-}}"

echo "[1/5] identity ($IDENT on $NET)"
stellar keys generate "$IDENT" --network "$NET" --fund 2>/dev/null ||
  stellar keys fund "$IDENT" --network "$NET" 2>/dev/null || true
ADMIN=$(stellar keys address "$IDENT")
echo "      admin: $ADMIN"

echo "[2/5] build wasm"
# soroban-sdk 28 requires building through the CLI (25.2.0+), not `cargo build`.
stellar contract build

if [ -n "$REP_ID" ]; then
  echo "[3/5] reputation already deployed ($REP_ID) — skipping"
else
  echo "[3/5] deploy reputation"
  REP_ID=$(stellar contract deploy \
    --wasm target/wasm32v1-none/release/reputation.wasm \
    --source "$IDENT" --network "$NET")
  stellar contract invoke --id "$REP_ID" --source "$IDENT" --network "$NET" --send=yes \
    -- initialize --admin "$ADMIN"
fi

if [ -n "$CIRCLE_ID" ]; then
  echo "[4/5] circle already deployed ($CIRCLE_ID) — skipping"
else
  echo "[4/5] deploy circle"
  CIRCLE_ID=$(stellar contract deploy \
    --wasm target/wasm32v1-none/release/circle.wasm \
    --source "$IDENT" --network "$NET")
  stellar contract invoke --id "$CIRCLE_ID" --source "$IDENT" --network "$NET" --send=yes \
    -- initialize --admin "$ADMIN" --reputation "$REP_ID"
fi

echo "[5/5] authorize circle as reputation reporter"
stellar contract invoke --id "$REP_ID" --source "$IDENT" --network "$NET" --send=yes \
  -- set_reporter --admin "$ADMIN" --reporter "$CIRCLE_ID" --allowed true

cat <<DONE

==========================================================
 OpenAjo deployed ($NET)
   REPUTATION_CONTRACT_ID=$REP_ID
   CIRCLE_CONTRACT_ID=$CIRCLE_ID
   ADMIN=$ADMIN
==========================================================
DONE
