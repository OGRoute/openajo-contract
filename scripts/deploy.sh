#!/usr/bin/env bash
# Deploy OpenAjo to Stellar in dependency order:
#   reputation -> circle -> set_reporter wiring.
# Usage: ./scripts/deploy.sh   (override IDENT/NETWORK via env)
#
# Testnet (default): funds the identity via friendbot and is safe to run
# unattended.
# Mainnet: friendbot does not exist, so this script never funds on mainnet —
#   the deployer must fund the identity out of band first, then opt in
#   explicitly with CONFIRM_MAINNET=1. A summary is printed before any
#   transaction is sent, so nothing is deployed by accident.
#
# Resuming a partial deploy: pass the ids that were already created so those
# steps are skipped instead of deploying duplicates, e.g. after step 4 failed:
#   REPUTATION_CONTRACT_ID=C... ./scripts/deploy.sh
#   REPUTATION_CONTRACT_ID=C... CIRCLE_CONTRACT_ID=C... ./scripts/deploy.sh
set -euo pipefail
cd "$(dirname "$0")/.."

IDENT="${STELLAR_IDENTITY:-openajo}"
NET="${STELLAR_NETWORK:-testnet}"
CONFIRM_MAINNET="${CONFIRM_MAINNET:-0}"

# Existing contract ids (optional). When provided, the corresponding deploy +
# initialize step is skipped, so a rerun after a partial failure reuses the
# already-created contract instead of deploying a second one.
REP_ID="${REPUTATION_CONTRACT_ID:-${REPUTATION_ID:-}}"
CIRCLE_ID="${CIRCLE_CONTRACT_ID:-${CIRCLE_ID:-}}"

# Mainnet requires an explicit opt-in: friendbot cannot fund a mainnet
# identity, so the deployer funds it out of band and confirms they want to
# proceed on a network where mistakes are irreversible.
if [ "$NET" = "mainnet" ] && [ "$CONFIRM_MAINNET" != "1" ]; then
  cat >&2 <<EOF
error: refusing to deploy to mainnet without an explicit confirmation.

  CONFIRM_MAINNET=1 STELLAR_NETWORK=mainnet ./scripts/deploy.sh

Fund the identity '$IDENT' on mainnet first — this script skips funding on
mainnet because friendbot does not exist there. A summary is printed before
any transaction is sent; set CONFIRM_MAINNET=1 to proceed.
EOF
  exit 1
fi

echo "[1/5] identity ($IDENT on $NET)"
if [ "$NET" = "mainnet" ]; then
  # friendbot is testnet-only: the identity must already be funded on mainnet.
  stellar keys generate "$IDENT" --network "$NET" 2>/dev/null || true
else
  stellar keys generate "$IDENT" --network "$NET" --fund 2>/dev/null ||
    stellar keys fund "$IDENT" --network "$NET" 2>/dev/null || true
fi
ADMIN=$(stellar keys address "$IDENT")
echo "      admin: $ADMIN"

# ---- Summary before anything is sent ---------------------------------------
cat <<SUMMARY

About to deploy OpenAjo:
  network   : $NET
  identity  : $IDENT ($ADMIN)
  reputation: ${REP_ID:-<will be deployed>}
  circle    : ${CIRCLE_ID:-<will be deployed>}

No transaction has been sent yet.
SUMMARY

if [ "$NET" = "mainnet" ]; then
  echo "      CONFIRM_MAINNET=$CONFIRM_MAINNET — proceeding on mainnet."
fi

echo "[2/5] build wasm"
# soroban-sdk 28 requires building through the CLI (25.2.0+), not `cargo build`.
stellar contract build

if [ -n "$REP_ID" ]; then
  echo "[3/5] reusing existing reputation: $REP_ID"
else
  echo "[3/5] deploy reputation"
  REP_ID=$(stellar contract deploy \
    --wasm target/wasm32v1-none/release/reputation.wasm \
    --source "$IDENT" --network "$NET")
  echo "      REPUTATION_CONTRACT_ID=$REP_ID"
  stellar contract invoke --id "$REP_ID" --source "$IDENT" --network "$NET" --send=yes \
    -- initialize --admin "$ADMIN"
fi

if [ -n "$CIRCLE_ID" ]; then
  echo "[4/5] reusing existing circle: $CIRCLE_ID"
else
  echo "[4/5] deploy circle"
  CIRCLE_ID=$(stellar contract deploy \
    --wasm target/wasm32v1-none/release/circle.wasm \
    --source "$IDENT" --network "$NET")
  echo "      CIRCLE_CONTRACT_ID=$CIRCLE_ID"
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
