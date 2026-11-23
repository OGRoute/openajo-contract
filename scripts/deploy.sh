#!/usr/bin/env bash
# Deploy OpenAjo to Stellar in dependency order:
#   reputation -> circle -> set_reporter wiring.
# Usage: ./scripts/deploy.sh   (override IDENT/NETWORK via env)
#
# On success the ids, the admin and the wasm hashes are written to
# deployments/<network>.json (DEPLOYMENTS_DIR overrides the directory), which
# verify-deployment.sh reads.
#
# Resuming a partial run: if a previous run already deployed a contract, pass
# its id back through the environment (REPUTATION_CONTRACT_ID /
# CIRCLE_CONTRACT_ID) and that step is skipped instead of deploying a duplicate.
set -euo pipefail
cd "$(dirname "$0")/.."

IDENT="${STELLAR_IDENTITY:-openajo}"
NET="${STELLAR_NETWORK:-testnet}"
CONFIRM_MAINNET="${CONFIRM_MAINNET:-0}"
DEPLOYMENTS_DIR="${DEPLOYMENTS_DIR:-deployments}"

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

# Accept already-deployed contract ids so a failed run can be resumed without
# redeploying (and replacing) contracts that already exist on-chain.
REP_ID="${REPUTATION_CONTRACT_ID:-${REP_ID:-}}"
CIRCLE_ID="${CIRCLE_CONTRACT_ID:-${CIRCLE_ID:-}}"

echo "[1/5] identity ($IDENT on $NET)"
# A missing account still exits 0 here (with `"entries":[]`), so check for an
# entry rather than the exit code.
account_exists() {
  stellar ledger entry fetch account --account "$IDENT" --network "$NET" 2>/dev/null |
    grep -q '"val"'
}
# Creates the key if it does not exist yet; a no-op when it does.
stellar keys generate "$IDENT" --network "$NET" >/dev/null 2>&1 || true
if [ "$NET" != "mainnet" ]; then
  # friendbot is testnet/local only. It can lag behind the network (a freshly
  # started local container answers before friendbot is up) or fail
  # transiently, so retry until the account exists instead of failing later
  # with "Account not found". Funding an already-funded account is a no-op.
  FUND_ATTEMPTS="${FUND_ATTEMPTS:-24}"
  for attempt in $(seq 1 "$FUND_ATTEMPTS"); do
    account_exists && break
    stellar keys fund "$IDENT" --network "$NET" >/dev/null 2>&1 || true
    account_exists && break
    echo "      waiting for friendbot to fund $IDENT ($attempt/$FUND_ATTEMPTS)..."
    sleep 5
  done
fi
ADMIN=$(stellar keys address "$IDENT")
echo "      admin: $ADMIN"
if ! account_exists; then
  if [ "$NET" = "mainnet" ]; then
    echo "error: account $ADMIN does not exist on mainnet; fund it out of band first." >&2
  else
    echo "error: could not fund $ADMIN on $NET via friendbot." >&2
  fi
  exit 1
fi

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

REP_DEPLOYED=0
CIRCLE_DEPLOYED=0

if [ -n "$REP_ID" ]; then
  echo "[3/5] reputation already deployed ($REP_ID) — skipping"
else
  REP_DEPLOYED=1
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
  CIRCLE_DEPLOYED=1
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

# ---- Record the deployment --------------------------------------------------
sha256() {
  if command -v sha256sum >/dev/null; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

# The wasm hash of a contract this run deployed is the hash of the build it
# uploaded. A contract passed in to resume a run may run an older build, so its
# wasm is fetched from the network instead. Prints "null" if that fails.
wasm_hash() { # name id deployed-by-this-run
  local name=$1 id=$2 fresh=$3 fetched
  if [ "$fresh" = 1 ]; then
    printf '"%s"' "$(sha256 "target/wasm32v1-none/release/$name.wasm")"
    return
  fi
  fetched=$(mktemp)
  if stellar contract fetch --id "$id" --network "$NET" --out-file "$fetched" >/dev/null 2>&1; then
    printf '"%s"' "$(sha256 "$fetched")"
  else
    echo "warning: could not fetch the $name wasm ($id); recording its hash as null" >&2
    printf 'null'
  fi
  rm -f "$fetched"
}

REP_HASH=$(wasm_hash reputation "$REP_ID" "$REP_DEPLOYED")
CIRCLE_HASH=$(wasm_hash circle "$CIRCLE_ID" "$CIRCLE_DEPLOYED")

mkdir -p "$DEPLOYMENTS_DIR"
DEPLOYMENT_FILE="$DEPLOYMENTS_DIR/$NET.json"
# Written to a temporary file and renamed, so a reader never sees half a file.
TMP_FILE=$(mktemp "$DEPLOYMENTS_DIR/.$NET.json.XXXXXX")
cat >"$TMP_FILE" <<JSON
{
  "network": "$NET",
  "circle_contract_id": "$CIRCLE_ID",
  "reputation_contract_id": "$REP_ID",
  "admin": "$ADMIN",
  "circle_wasm_hash": $CIRCLE_HASH,
  "reputation_wasm_hash": $REP_HASH,
  "deployed_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
}
JSON
chmod 644 "$TMP_FILE"
mv "$TMP_FILE" "$DEPLOYMENT_FILE"

cat <<DONE

==========================================================
 OpenAjo deployed ($NET)
   REPUTATION_CONTRACT_ID=$REP_ID
   CIRCLE_CONTRACT_ID=$CIRCLE_ID
   ADMIN=$ADMIN
 Recorded in $DEPLOYMENT_FILE
==========================================================
DONE
