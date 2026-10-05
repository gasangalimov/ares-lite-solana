#!/usr/bin/env bash
# ARES Lite fixed-supply release (one script for localnet, devnet and mainnet-beta).
#
#   CLUSTER=localnet  FOUNDER_WALLET=<base58> release/release.sh   # local validator rehearsal
#   CLUSTER=devnet    FOUNDER_WALLET=<base58> release/release.sh   # devnet rehearsal
#   CLUSTER=mainnet-beta FOUNDER_WALLET=<base58> EXPECTED_SO_SHA256=<hash> \
#       ARES_LITE_MAINNET_GO=OWNER-GO-MAINNET-IRREVERSIBLE release/release.sh
#
# Stops at the first mismatch (set -e + explicit checks). Keys live in $KEYS
# (default ~/.config/ares-lite/<cluster>), NEVER inside the repository.
# Mainnet requires: the owner's GO phrase, EXPECTED_SO_SHA256 equal to the
# reviewed binary hash, and typing the founder address interactively.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PY="${PYTHON:-python3}"
CLI="$PY $ROOT/cli/ares_lite_chain.py"
CLUSTER="${CLUSTER:?CLUSTER=localnet|devnet|mainnet-beta}"
FOUNDER_WALLET="${FOUNDER_WALLET:?FOUNDER_WALLET must be set explicitly (base58 address)}"
KEYS="${KEYS:-$HOME/.config/ares-lite/$CLUSTER}"
OUT="${OUT:-$ROOT/release/out/$CLUSTER}"   # gitignored; contains public addresses only
case "$CLUSTER" in
  localnet) URL="${RPC:-http://127.0.0.1:8899}";;
  devnet) URL="${RPC:-https://api.devnet.solana.com}";;
  mainnet-beta) URL="${RPC:-https://api.mainnet-beta.solana.com}";;
  *) echo "unknown cluster" >&2; exit 2;;
esac
case "$KEYS" in "$ROOT"*) echo "refusing: KEYS inside the repository" >&2; exit 1;; esac
fail() { echo "RELEASE STOPPED: $*" >&2; exit 1; }
step() { echo; echo "== [$1] $2"; }

if [[ "$CLUSTER" == "mainnet-beta" ]]; then
  [[ "${ARES_LITE_MAINNET_GO:-}" == "OWNER-GO-MAINNET-IRREVERSIBLE" ]] || fail "mainnet needs the owner's explicit GO (ARES_LITE_MAINNET_GO)"
  [[ -n "${EXPECTED_SO_SHA256:-}" ]] || fail "mainnet needs EXPECTED_SO_SHA256 of the reviewed binary"
  echo "IRREVERSIBLE: this creates the production ARES mint, sends 100,000,000 ARES to"
  echo "  $FOUNDER_WALLET"
  echo "and revokes the mint authority forever. Type the founder address to continue:"
  read -r typed
  [[ "$typed" == "$FOUNDER_WALLET" ]] || fail "confirmation mismatch"
fi
mkdir -p "$KEYS" "$OUT"
chmod 700 "$KEYS"

step 1 "build reviewed binary"
(cd "$ROOT/solana/program" && SSL_CERT_FILE="${SSL_CERT_FILE:-/etc/ssl/certs/ca-certificates.crt}" cargo build-sbf -- --locked >/dev/null)
SO="$ROOT/solana/program/target/deploy/ares_lite_rewards.so"
HASH=$(sha256sum "$SO" | cut -d' ' -f1)
echo "binary sha256: $HASH   commit: $(git -C "$ROOT" rev-parse HEAD)"

step 2 "verify binary hash"
if [[ -n "${EXPECTED_SO_SHA256:-}" ]]; then [[ "$HASH" == "$EXPECTED_SO_SHA256" ]] || fail "binary hash $HASH != expected $EXPECTED_SO_SHA256"; fi

[[ -f "$KEYS/deployer.json" ]] || solana-keygen new --no-bip39-passphrase --silent -o "$KEYS/deployer.json"
[[ -f "$KEYS/program.json" ]] || solana-keygen new --no-bip39-passphrase --silent -o "$KEYS/program.json"
chmod 600 "$KEYS"/*.json
PROGRAM_ID=$(solana-keygen pubkey "$KEYS/program.json")
DEPLOYER=$(solana-keygen pubkey "$KEYS/deployer.json")
solana config set --url "$URL" --keypair "$KEYS/deployer.json" >/dev/null
if [[ "$CLUSTER" != "mainnet-beta" ]]; then solana airdrop 5 "$DEPLOYER" >/dev/null || true; fi

step "1b" "deploy program (upgrade authority = deployer, stage 1)"
DEPLOY_SIG=$(solana program deploy --use-rpc --program-id "$KEYS/program.json" "$SO" --output json | $PY -c "import json,sys;print(json.load(sys.stdin).get('signature',''))")
solana program show "$PROGRAM_ID" --output json > "$OUT/program_show.json"
ONCHAIN_HASH=$(solana program dump "$PROGRAM_ID" "$OUT/onchain.so" >/dev/null && sha256sum "$OUT/onchain.so" | cut -d' ' -f1)
# The dumped ProgramData is padded; compare the prefix of the deployed length.
$PY - "$SO" "$OUT/onchain.so" <<'EOF' || fail "on-chain program bytes differ from the reviewed binary"
import sys
local, remote = open(sys.argv[1], "rb").read(), open(sys.argv[2], "rb").read()
sys.exit(0 if remote[:len(local)] == local and not remote[len(local):].strip(b"\0") else 1)
EOF

step "3-8" "create SPL mint + ATOMIC genesis (1B; 100M founder; 900M vault; revoke mint authority)"
export ARES_LITE_KEYS="$KEYS"
cp "$KEYS/deployer.json" "$KEYS/admin.json"
# Public verification window between publishing an epoch root and claims.
if [[ "$CLUSTER" == "mainnet-beta" ]]; then CLAIM_DELAY="${CLAIM_DELAY:-259200}"; else CLAIM_DELAY="${CLAIM_DELAY:-300}"; fi
$CLI chain-genesis --workdir "$OUT" --rpc "$URL" --program-id "$PROGRAM_ID" --founder-wallet "$FOUNDER_WALLET" --admin-key "$KEYS/admin.json" \
  --claim-delay "$CLAIM_DELAY"

step "7,9,10" "verify totals, freeze authority None, additional mint impossible (simulated, commits nothing)"
$CLI verify-fixed-supply --workdir "$OUT" --at-genesis --death-test-key "$KEYS/deployer.json" > "$OUT/verify_fixed_supply.json"
cat "$OUT/verify_fixed_supply.json"

step "11-14" "verify emission schedule, genesis timestamp, halving curve, reward vault"
$CLI supply --workdir "$OUT" > "$OUT/supply.json"
$PY - "$OUT/supply.json" <<'EOF' || fail "schedule/vault verification failed"
import json, sys, time
sys.path.insert(0, ".")
from ares_lite import tokenomics as tk
s = json.load(open(sys.argv[1]))
assert s["reward_reserve_remaining"] == tk.REWARD_RESERVE, "vault != 900M"
assert s["current_era"] == 0 and s["current_era_budget"] == tk.ERA0_BUDGET, "not era 0"
assert abs(s["genesis_ts"] - time.time()) < 3600, "genesis timestamp not recent"
assert s["next_halving_ts"] == s["genesis_ts"] + tk.ERA_SECONDS, "halving not exactly 5 years"
assert tk.unlocked(tk.ERA_SECONDS) == tk.ERA0_BUDGET and tk.unlocked(2 * tk.ERA_SECONDS) == tk.ERA0_BUDGET * 3 // 2
assert s["committed_to_seasons"] == 0 and s["distributed_to_miners"] == 0
assert s["claim_delay_seconds"] > 0, "claim delay (verification window) must be > 0"
print("schedule ok: genesis", s["genesis_utc"], "next halving", s["next_halving_utc"])
EOF

step 15 "verify burn path (unsigned simulation of a founder-funded bounty; commits nothing)"
if [[ "${SKIP_BURN_SIM:-0}" == "1" ]]; then
  echo "WARNING: burn simulation skipped by operator (SKIP_BURN_SIM=1); recorded in RELEASE.json" | tee "$OUT/verify_burn.json"
else
  $CLI verify-fixed-supply --workdir "$OUT" --simulate-burn > "$OUT/verify_burn.json" || fail "burn simulation (founder wallet needs ~0.01 SOL for bounty rent)"
  cat "$OUT/verify_burn.json"
fi

step 16 "publish addresses"
$PY - "$OUT" "$HASH" "$DEPLOY_SIG" "$CLUSTER" <<'EOF'
import json, subprocess, sys, time
from pathlib import Path
out, so_hash, sig, cluster = Path(sys.argv[1]), sys.argv[2], sys.argv[3], sys.argv[4]
chain = json.loads((out / "chain.json").read_text())
show = json.loads((out / "program_show.json").read_text())
release = {
    "cluster": cluster, "commit": subprocess.check_output(["git", "rev-parse", "HEAD"]).decode().strip(),
    "binary_sha256": so_hash, "deploy_signature": sig, "deployed_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
    "program_id": chain["program_id"], "programdata_address": show.get("programdataAddress"),
    "upgrade_authority": show.get("authority"), **{k: chain[k] for k in chain if k != "rpc"},
    "burn_simulation": (out / "verify_burn.json").read_text().strip()[:2000],
}
(out / "RELEASE.json").write_text(json.dumps(release, indent=2) + "\n")
print(json.dumps(release, indent=2))
EOF

step 17 "reproduce from a clean client (only public addresses + RPC)"
$CLI verify-fixed-supply --rpc "$URL" --program-id "$PROGRAM_ID" --mint "$($PY -c "import json;print(json.load(open('$OUT/chain.json'))['mint'])")" >/dev/null
echo; echo "RELEASE COMPLETE ($CLUSTER). Public record: $OUT/RELEASE.json"
echo "Irreversible on this cluster: mint created, 100M sent to founder, 900M in vault, mint authority revoked."
echo "Still reversible/staged: program upgrade authority ($DEPLOYER) -> multisig or --final (separate decision)."
