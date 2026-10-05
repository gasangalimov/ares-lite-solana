"""Example custom solver: YOUR copy of the starter Rust crate.

First run: copies the starter crate into ./crate next to this file. Then edit
./crate/src/lib.rs (by hand, or ask an AI coding agent: "improve the
`solve` function in crate/src/lib.rs; keep the ARES-WASM-V0 rules in the
header comment; lower fuel is better; correctness first"). Every Miner
iteration rebuilds YOUR crate with the season constants and hands the module
to the Miner, which validates and scores it locally.

Needs Rust + `rustup target add wasm32-unknown-unknown`.
"""

from __future__ import annotations

import json
import shutil
from pathlib import Path

from ares_lite import LITE_ROOT
from ares_lite.solver_contract import read_request, write_response
from ares_lite.tooling import build_solver

HERE = Path(__file__).resolve().parent
CRATE = HERE / "crate"
FEATURES: list[str] = []          # e.g. ["memo"]: start from one of the shipped variants


def main() -> None:
    request = read_request()
    if not CRATE.exists():
        shutil.copytree(LITE_ROOT / "solvers" / "graph_route", CRATE, ignore=shutil.ignore_patterns("target"))
    if shutil.which("cargo") is None:
        write_response(request, None, notes="cargo not found")
        return
    challenge = json.loads(Path(request["challenge_path"]).read_text(encoding="utf-8"))
    scaled = str(challenge.get("profile", "")).startswith("lite-L1")
    target, implied = ("L1", ["scaled"]) if scaled else (str(challenge["difficulty_index"]), [])
    module = build_solver(target, Path(request["season_rs_path"]).read_text(encoding="utf-8"), sorted(set(implied + FEATURES)),
                          crate=CRATE, target_tag="-my-rust-solver")
    write_response(request, module, notes="built from crate/src/lib.rs", solver_name="my-rust-solver", solver_version="1")


if __name__ == "__main__":
    main()
