#!/usr/bin/env bash
# Integration test: run scripts/deploy.sh against a local Stellar network
# (quickstart container) and drive a small circle end to end with the CLI.
# Skips cleanly (exit 0) when docker or the stellar CLI is unavailable.
# Set REQUIRE_LOCAL_NETWORK=1 (CI does) to fail instead of skip.
set -euo pipefail
cd "$(dirname "$0")/.."

skip() {
  echo "SKIP: $1"
  [ "${REQUIRE_LOCAL_NETWORK:-0}" = 1 ] && exit 1
  exit 0
}
command -v stellar >/dev/null || skip "stellar CLI not installed"
command -v docker >/dev/null && docker info >/dev/null 2>&1 || skip "docker not available"

NET=local
ADMIN_ID="deploy-test-admin-$$"
B_ID="deploy-test-b-$$"
DEPLOYMENTS_DIR=$(mktemp -d)
export DEPLOYMENTS_DIR
trap 'stellar container stop "$NET" >/dev/null 2>&1 || true; rm -rf "$DEPLOYMENTS_DIR"' EXIT

stellar container start "$NET" >/dev/null
echo "waiting for the local network..."
# The proxy on :8000 answers "/" with 200 well before friendbot and RPC are
# up, so only a successful funding counts as ready. Funding the admin here
# also means deploy.sh finds it already funded.
stellar keys generate "$ADMIN_ID" --network "$NET" >/dev/null
ready=0
for _ in $(seq 1 60); do
  if stellar keys fund "$ADMIN_ID" --network "$NET" >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 5
done
[ "$ready" = 1 ] || { echo "FAIL: local network friendbot never became ready"; exit 1; }

export STELLAR_IDENTITY="$ADMIN_ID" STELLAR_NETWORK="$NET"
OUT=$(./scripts/deploy.sh)
echo "$OUT"
CIRCLE=$(echo "$OUT" | sed -n "s/^ *CIRCLE_CONTRACT_ID=//p")
REP=$(echo "$OUT" | sed -n "s/^ *REPUTATION_CONTRACT_ID=//p")
A=$(stellar keys address "$ADMIN_ID")
test -n "$CIRCLE" && test -n "$REP"

# deploy.sh records the deployment, and verify-deployment.sh reads it back.
RECORD="$DEPLOYMENTS_DIR/$NET.json"
test -f "$RECORD" || { echo "FAIL: deploy.sh did not write $RECORD"; exit 1; }
cat "$RECORD"
for want in "\"circle_contract_id\": \"$CIRCLE\"" "\"reputation_contract_id\": \"$REP\"" \
  "\"admin\": \"$A\"" \
  "\"circle_wasm_hash\": \"$(sha256sum target/wasm32v1-none/release/circle.wasm | cut -d' ' -f1)\"" \
  "\"reputation_wasm_hash\": \"$(sha256sum target/wasm32v1-none/release/reputation.wasm | cut -d' ' -f1)\""; do
  grep -qF "$want" "$RECORD" || { echo "FAIL: $RECORD lacks $want"; exit 1; }
done
VERIFY=$(./scripts/verify-deployment.sh)
echo "$VERIFY"
echo "$VERIFY" | grep -q "reading deployed ids from $RECORD" || { echo "FAIL: verify-deployment.sh ignored $RECORD"; exit 1; }
echo "$VERIFY" | grep -q "circle: MATCH .*$CIRCLE" || { echo "FAIL: circle not verified"; exit 1; }
echo "$VERIFY" | grep -q "reputation: MATCH .*$REP" || { echo "FAIL: reputation not verified"; exit 1; }

stellar keys generate "$B_ID" --network "$NET" --fund
B=$(stellar keys address "$B_ID")
TOKEN=$(stellar contract asset deploy --asset native --source "$ADMIN_ID" --network "$NET")

inv() { # source id fn args...
  local src=$1 id=$2; shift 2
  stellar contract invoke --id "$id" --source "$src" --network "$NET" --send=yes -- "$@"
}

CID=$(inv "$ADMIN_ID" "$CIRCLE" create_circle --creator "$A" --token "$TOKEN" \
  --contribution 100 --deposit 0 --size 2 --period_secs 3600 | tr -d "\"")
inv "$B_ID" "$CIRCLE" join --circle_id "$CID" --member "$B"
inv "$ADMIN_ID" "$CIRCLE" contribute --circle_id "$CID" --member "$A"
inv "$B_ID" "$CIRCLE" contribute --circle_id "$CID" --member "$B"
inv "$ADMIN_ID" "$CIRCLE" settle_cycle --circle_id "$CID"

MEMBER=$(stellar contract invoke --id "$CIRCLE" --source "$ADMIN_ID" --network "$NET" \
  -- get_member --circle_id "$CID" --member "$A")
echo "member A state: $MEMBER"
echo "$MEMBER" | grep -q "\"received\":true" || { echo "FAIL: first recipient not paid"; exit 1; }
echo "PASS: deploy.sh + circle lifecycle on local network"
