#!/usr/bin/env bash
# Verify that the contracts deployed on-chain are the code in this tree.
#
# deploy.sh uploads the unoptimized release build, so the deployed wasm should
# be byte-identical to `cargo build --target wasm32v1-none --release` output.
# Anyone can run this: it only reads the network.
#
# Usage: ./scripts/verify-deployment.sh
#   STELLAR_NETWORK         network name (default: testnet)
#   CIRCLE_CONTRACT_ID      override the circle id
#   REPUTATION_CONTRACT_ID  override the reputation id
set -euo pipefail
cd "$(dirname "$0")/.."

NET="${STELLAR_NETWORK:-testnet}"
CIRCLE_ID="${CIRCLE_CONTRACT_ID:-CCLVOHGHDH32GWFAMCEMVHLNJSF6ENVHERYWU2OHUYWWLAOKLVR3HGKS}"
REP_ID="${REPUTATION_CONTRACT_ID:-CDXPH2PYUTRW7GV57X6CJH3E3JOPROSC23NXPMAXOO3EOBI5UTCB2GTQ}"

command -v stellar >/dev/null || {
  echo "stellar CLI not found: https://developers.stellar.org/docs/tools/developer-tools/cli" >&2
  exit 127
}

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

echo "[1/2] building release wasm"
cargo build --target wasm32v1-none --release >/dev/null

echo "[2/2] comparing against $NET"
status=0

check() {
  local name=$1 id=$2
  local built="target/wasm32v1-none/release/$name.wasm"
  local want have

  want=$(sha256sum "$built" | cut -d' ' -f1)

  if ! stellar contract fetch --id "$id" --network "$NET" \
    --out-file "$tmp/$name.wasm" >/dev/null 2>"$tmp/$name.err"; then
    echo "  $name: FETCH FAILED ($id)"
    sed 's/^/    /' "$tmp/$name.err" >&2
    status=1
    return
  fi

  have=$(sha256sum "$tmp/$name.wasm" | cut -d' ' -f1)

  if [ "$want" = "$have" ]; then
    echo "  $name: MATCH   ${want:0:16}…  $id"
  else
    echo "  $name: DIFFERS $id"
    echo "    built    $want"
    echo "    deployed $have"
    status=1
  fi
}

check circle "$CIRCLE_ID"
check reputation "$REP_ID"

if [ "$status" -eq 0 ]; then
  echo
  echo "Deployed contracts match this working tree."
else
  echo
  echo "A mismatch is expected on a branch that changes contract code: the" >&2
  echo "deployed ids still run the previously deployed build. Redeploy, or" >&2
  echo "check out the commit that was deployed." >&2
fi
exit "$status"
