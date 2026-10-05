"""The whole solver contract in ~25 lines, with no SDK import, so it ports to any language.

ARES Miner runs:   <command...> <path/to/request.json>     (no shell; cwd = a fresh iteration dir)
request.json:      {"contract": "ares-lite-solver/1", "iteration": 3, "seed": "<hex32>",
                    "deadline_unix": 1791200000, "limits": {"max_module_bytes": 131072, "timeout_seconds": 600},
                    "challenge_path": ".../challenge.json", "season_rs_path": ".../season.rs",
                    "baseline_path": ".../baseline.wasm", "manifest_path": ".../manifest.json",
                    "out_dir": "<iteration dir>", "solution_file": "solution.wasm", "response_file": "response.json"}
You write:         <out_dir>/solution.wasm   (an ARES-WASM-V0 module exporting `solve`)
                   <out_dir>/response.json  {"contract": "ares-lite-solver/1", "status": "ok"|"no_solution",
                                             "solution": "solution.wasm", "notes": "..."}
The Miner re-encodes, validates and scores the module itself; nothing you claim is trusted.
"""

import json
import sys
from pathlib import Path

request = json.loads(Path(sys.argv[-1]).read_text(encoding="utf-8"))
assert request["contract"] == "ares-lite-solver/1"
out = Path(request["out_dir"])

module = Path(request["baseline_path"]).read_bytes()   # <- replace with your own generator / compiler / search

(out / request["solution_file"]).write_bytes(module)
(out / request["response_file"]).write_text(json.dumps({
    "contract": "ares-lite-solver/1", "status": "ok", "solution": request["solution_file"],
    "notes": f"template iteration {request['iteration']}"}), encoding="utf-8")
