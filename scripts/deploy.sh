#!/usr/bin/env bash
# Deploy OpenAjo to Stellar testnet in dependency order:
#   reputation -> circle -> set_reporter wiring.
# Usage: ./scripts/deploy.sh   (override IDENT/NETWORK via env)
set -euo pipefail
cd "$(dirname "$0")/.."

IDENT="${STELLAR_IDENTITY:-openajo}"
NET="${STELLAR_NETWORK:-testnet}"

echo "[1/5] identity ($IDENT on $NET)"
stellar keys generate "$IDENT" --network "$NET" --fund 2>/dev/null ||
  stellar keys fund "$IDENT" --network "$NET" 2>/dev/null || true
ADMIN=$(stellar keys address "$IDENT")
echo "      admin: $ADMIN"

echo "[2/5] build wasm"
cargo build --target wasm32-unknown-unknown --release

echo "[3/5] deploy reputation"
REP_ID=$(stellar contract deploy \
  --wasm target/wasm32-unknown-unknown/release/reputation.wasm \
  --source "$IDENT" --network "$NET")
stellar contract invoke --id "$REP_ID" --source "$IDENT" --network "$NET" --send=yes \
  -- initialize --admin "$ADMIN"

echo "[4/5] deploy circle"
CIRCLE_ID=$(stellar contract deploy \
  --wasm target/wasm32-unknown-unknown/release/circle.wasm \
  --source "$IDENT" --network "$NET")
stellar contract invoke --id "$CIRCLE_ID" --source "$IDENT" --network "$NET" --send=yes \
  -- initialize --admin "$ADMIN" --reputation "$REP_ID"

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
