"""Built-in solver: the starter Rust crate, searching over its algorithm variants.

Iteration i builds the starter crate (`ares_lite/solvers/graph_route`) with the
i-th feature set below, so a Miner run improves step by step. Needs `cargo` and
the `wasm32-unknown-unknown` target (`rustup target add wasm32-unknown-unknown`).
Edit the crate (or copy it, see examples/solvers/my_rust_solver) to compete
with your own algorithm.
"""

from __future__ import annotations

import json
import shutil
from pathlib import Path

from ares_lite.solver_contract import read_request, write_response
from ares_lite.tooling import build_solver

FEATURE_SETS = ([], ["compact"], ["memo"], ["bound"], ["compact", "memo"], ["memo", "bound"], ["compact", "memo", "bound"])


def target_of(challenge: dict) -> tuple[str, list[str]]:
    scaled = str(challenge.get("profile", "")).startswith("lite-L1")
    return ("L1", ["scaled"]) if scaled else (str(challenge["difficulty_index"]), [])


def main() -> None:
    request = read_request()
    if shutil.which("cargo") is None:
        write_response(request, None, notes="cargo not found: install Rust and `rustup target add wasm32-unknown-unknown`")
        return
    challenge = json.loads(Path(request["challenge_path"]).read_text(encoding="utf-8"))
    target, implied = target_of(challenge)
    features = FEATURE_SETS[request["iteration"] % len(FEATURE_SETS)]
    module = build_solver(target, Path(request["season_rs_path"]).read_text(encoding="utf-8"), sorted(set(implied + features)))
    write_response(request, module, notes=f"starter crate, features {features or ['(default)']}",
                   solver_name="starter-feature-search", solver_version="1")


if __name__ == "__main__":
    main()
