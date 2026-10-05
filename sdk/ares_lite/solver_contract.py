"""Solver contract `ares-lite-solver/1` (language-agnostic; see SOLVER_CONTRACT.md).

A solver is any program described by a `solver.json`:

    {"contract": "ares-lite-solver/1", "name": "my-solver", "version": "1",
     "command": ["python3", "solver.py"], "timeout_seconds": 600}

The Miner runs `command + [<request.json path>]` WITHOUT a shell, inside a
fresh per-iteration directory, and reads the response from `response.json`
in that directory. The solver writes its candidate module there. The Miner
never trusts anything else a solver says: it re-encodes, validates and scores
the module itself with the SDK.
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path

CONTRACT = "ares-lite-solver/1"
MAX_MODULE_BYTES = 131_072
MAX_RESPONSE_BYTES = 65_536
MAX_LOG_BYTES = 65_536
DEFAULT_TIMEOUT = 600
MAX_TIMEOUT = 24 * 3600
BUILTIN = {
    "baseline": {"name": "baseline", "version": "1", "command": [sys.executable, "-m", "ares_lite.solvers.baseline"],
                 "timeout_seconds": 60},
    "starter": {"name": "starter-feature-search", "version": "1", "command": [sys.executable, "-m", "ares_lite.solvers.starter"],
                "timeout_seconds": 900},
}


class SolverError(ValueError):
    pass


@dataclass(frozen=True)
class SolverSpec:
    name: str
    version: str
    command: tuple[str, ...]
    timeout_seconds: int = DEFAULT_TIMEOUT
    base_dir: Path | None = None


@dataclass
class SolverResult:
    module: bytes | None
    status: str
    solver_name: str
    solver_version: str
    seconds: int
    notes: str = ""
    claimed: dict = field(default_factory=dict)  # informational only, never trusted
    log_tail: str = ""


def load_solver(ref: str) -> SolverSpec:
    """`builtin:baseline`, `builtin:starter`, or a path to a solver.json."""

    if ref.startswith("builtin:"):
        data, base = BUILTIN.get(ref.split(":", 1)[1]), None
        if data is None:
            raise SolverError(f"unknown builtin solver {ref}")
    else:
        path = Path(ref).expanduser().resolve()
        if path.is_dir():
            path = path / "solver.json"
        if not path.is_file() or path.stat().st_size > 16_384:
            raise SolverError(f"solver.json not found or too large: {path}")
        data, base = json.loads(path.read_text(encoding="utf-8")), path.parent
        if data.get("contract") != CONTRACT:
            raise SolverError(f"solver.json must declare contract {CONTRACT}")
    command = data.get("command")
    if not (isinstance(command, list) and command and all(isinstance(c, str) and c and "\x00" not in c for c in command)):
        raise SolverError("command must be a non-empty list of strings (no shell)")
    timeout = int(data.get("timeout_seconds", DEFAULT_TIMEOUT))
    if not 1 <= timeout <= MAX_TIMEOUT:
        raise SolverError("timeout_seconds out of range")
    name, version = str(data.get("name", "solver"))[:64], str(data.get("version", "0"))[:32]
    return SolverSpec(name, version, tuple(command), timeout, base)


def _resolve_command(spec: SolverSpec) -> list[str]:
    command = list(spec.command)
    if command[0] in ("python", "python3", "py"):
        command[0] = sys.executable  # portable: Windows has no `python3`; use the Miner's own interpreter
    if spec.base_dir is not None:
        # Relative script paths (e.g. "solver.py", "./solve") are resolved next to solver.json.
        command = [str(spec.base_dir / c) if (spec.base_dir / c).is_file() and not os.path.isabs(c) else c for c in command]
    return command


def run_solver(spec: SolverSpec, season_dir: Path, work_dir: Path, iteration: int, seed: bytes,
               budget_seconds: int | None = None) -> SolverResult:
    """Run one solver iteration; returns the candidate module bytes (unvalidated)."""

    season_dir = Path(season_dir).resolve()
    work_dir = Path(work_dir).resolve()
    if work_dir.exists():
        shutil.rmtree(work_dir)
    work_dir.mkdir(parents=True)
    timeout = min(spec.timeout_seconds, budget_seconds or spec.timeout_seconds)
    request = {
        "contract": CONTRACT,
        "iteration": int(iteration),
        "seed": seed.hex(),
        "deadline_unix": int(time.time()) + timeout,
        "limits": {"max_module_bytes": MAX_MODULE_BYTES, "timeout_seconds": timeout},
        "challenge_path": str(season_dir / "challenge.json"),
        "season_rs_path": str(season_dir / "season.rs"),
        "baseline_path": str(season_dir / "baseline.wasm"),
        "manifest_path": str(season_dir / "manifest.json"),
        "out_dir": str(work_dir),
        "solution_file": "solution.wasm",
        "response_file": "response.json",
    }
    request_path = work_dir / "request.json"
    request_path.write_text(json.dumps(request, indent=2), encoding="utf-8")
    env = {k: v for k, v in os.environ.items() if k in ("PATH", "HOME", "USERPROFILE", "SYSTEMROOT", "TEMP", "TMP", "TMPDIR",
                                                        "CARGO_HOME", "RUSTUP_HOME", "LANG", "PYTHONPATH", "ARES_LITE_CACHE",
                                                        "APPDATA", "LOCALAPPDATA", "COMSPEC", "PATHEXT")}
    # The SDK itself is importable by Python solvers even when it is not pip-installed.
    sdk_root = str(Path(__file__).resolve().parents[1])
    env["PYTHONPATH"] = sdk_root + (os.pathsep + env["PYTHONPATH"] if env.get("PYTHONPATH") else "")
    started = time.monotonic()
    log_path = work_dir / "solver.log"
    with open(log_path, "wb") as log:
        try:
            proc = subprocess.run(_resolve_command(spec) + [str(request_path)], cwd=work_dir, stdin=subprocess.DEVNULL,
                                  stdout=log, stderr=subprocess.STDOUT, timeout=timeout, env=env, shell=False)
            code = proc.returncode
        except subprocess.TimeoutExpired:
            code = "timeout"
        except OSError as exc:
            code = f"cannot start: {exc}"
    seconds = int(time.monotonic() - started)
    tail = _tail(log_path)
    result = SolverResult(None, "error", spec.name, spec.version, seconds, log_tail=tail)
    if code != 0:
        result.notes = f"solver exited with {code}"
        return result
    response_path = work_dir / "response.json"
    if not response_path.is_file() or response_path.stat().st_size > MAX_RESPONSE_BYTES:
        result.notes = "missing or oversized response.json"
        return result
    try:
        response = json.loads(response_path.read_text(encoding="utf-8"))
    except (ValueError, UnicodeDecodeError):
        result.notes = "response.json is not valid JSON"
        return result
    if not isinstance(response, dict) or response.get("contract") != CONTRACT:
        result.notes = "response.json does not declare the contract"
        return result
    result.status = str(response.get("status", "error"))[:32]
    result.notes = str(response.get("notes", ""))[:500]
    result.claimed = {k: response[k] for k in ("claimed_score", "solver_name", "solver_version") if k in response}
    if result.status != "ok":
        return result
    solution = (work_dir / str(response.get("solution", "solution.wasm"))).resolve()
    if work_dir not in solution.parents:
        result.status, result.notes = "error", "solution path escapes the iteration directory"
        return result
    if not solution.is_file() or solution.is_symlink() or solution.stat().st_size > MAX_MODULE_BYTES:
        result.status, result.notes = "error", "solution file missing, a symlink, or larger than the module limit"
        return result
    result.module = solution.read_bytes()
    return result


def _tail(path: Path) -> str:
    size = path.stat().st_size
    with open(path, "rb") as handle:
        handle.seek(max(0, size - MAX_LOG_BYTES))
        return handle.read().decode("utf-8", errors="replace")


def read_request(argv: list[str] | None = None) -> dict:
    """Helper for Python solvers: load and minimally check the request."""

    argv = sys.argv[1:] if argv is None else argv
    request = json.loads(Path(argv[-1]).read_text(encoding="utf-8"))
    if request.get("contract") != CONTRACT:
        raise SystemExit(f"unsupported contract {request.get('contract')}")
    return request


def write_response(request: dict, module: bytes | None, notes: str = "", **extra) -> None:
    out = Path(request["out_dir"])
    response = {"contract": CONTRACT, "status": "ok" if module else "no_solution", "notes": notes, **extra}
    if module:
        (out / request["solution_file"]).write_bytes(module)
        response["solution"] = request["solution_file"]
    (out / request["response_file"]).write_text(json.dumps(response), encoding="utf-8")
