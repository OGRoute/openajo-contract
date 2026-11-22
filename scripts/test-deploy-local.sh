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
trap "stellar container stop $NET >/dev/null 2>&1 || true" EXIT

stellar container start "$NET" >/dev/null
echo "waiting for the local network..."
for _ in $(seq 1 60); do
  curl -sf http://localhost:8000/friendbot?addr=GAAZI4TCR3TY5OJHCTJC2A4QSY6CJWJH5IAJTGKIN2ER7LBNVKOCCWN7 >/dev/null 2>&1 && break
  curl -sf http://localhost:8000/ >/dev/null 2>&1 && sleep 5 && break
  sleep 5
done

export STELLAR_IDENTITY="$ADMIN_ID" STELLAR_NETWORK="$NET"
OUT=$(./scripts/deploy.sh)
echo "$OUT"
CIRCLE=$(echo "$OUT" | sed -n "s/^ *CIRCLE_CONTRACT_ID=//p")
REP=$(echo "$OUT" | sed -n "s/^ *REPUTATION_CONTRACT_ID=//p")
A=$(stellar keys address "$ADMIN_ID")
test -n "$CIRCLE" && test -n "$REP"

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
