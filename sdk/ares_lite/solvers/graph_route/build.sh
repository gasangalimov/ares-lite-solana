#!/usr/bin/env bash
# Build an ARES-WASM-V0 module from this crate.
#   build.sh <0|1|2|L1> [cargo features...] -> prints module path
# (L1 = ARES Lite scaled profile, 16 pages; build with the `scaled` feature)
# Memory must equal the frozen profile page count (2/4/8 pages). The shadow
# stack is placed at the top of linear memory (global-base + no stack-first) so
# it never overlaps the host input/output region that starts at address 0.
set -euo pipefail
cd "$(dirname "$0")"
difficulty="${1:?difficulty index required}"; shift || true
case "$difficulty" in 0) pages=2;; 1) pages=4;; 2) pages=8;; L1) pages=16;; *) echo "bad difficulty" >&2; exit 2;; esac
memory=$((pages * 65536))
stack=32768
features="${*:-}"
target_dir="${CARGO_TARGET_DIR_OVERRIDE:-target}/d${difficulty}${features:+-${features// /-}}"
RUSTFLAGS="-C target-cpu=mvp -C target-feature=-bulk-memory,-sign-ext,-multivalue,-reference-types,-nontrapping-fptoint,-mutable-globals \
 -C link-arg=--import-memory=ares,memory -C link-arg=--initial-memory=${memory} -C link-arg=--max-memory=${memory} \
 -C link-arg=--no-entry -C link-arg=--export=solve -C link-arg=-zstack-size=${stack} \
 -C link-arg=--no-stack-first -C link-arg=--global-base=$((memory - stack - 4096)) -C link-arg=--strip-all" \
cargo build --quiet --release --lib --target wasm32-unknown-unknown --target-dir "$target_dir" \
  ${features:+--features "$features"}
case "$target_dir" in /*) ;; *) target_dir="$PWD/$target_dir";; esac
echo "$target_dir/wasm32-unknown-unknown/release/ares_lite_graph_route_solver.wasm"
