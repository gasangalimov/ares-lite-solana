# Solver contract `ares-lite-solver/1`

A solver is any program, in any language, that writes one candidate
ARES-WASM-V0 module per run. ARES Miner and `ares-lite solve` run it, then
**validate and score the module themselves**. Nothing a solver claims is trusted.

## solver.json

```json
{"contract": "ares-lite-solver/1", "name": "my-solver", "version": "1",
 "command": ["python3", "solver.py"], "timeout_seconds": 900}
```

- `command`: an argument list run **without a shell**. Relative script names are
  resolved next to `solver.json`. `python3`/`python` maps to the Miner's own
  interpreter, so the same file works on Windows.
- `timeout_seconds`: 1 … 86,400. The process is killed after it.

## Invocation

The Miner runs `command + [<absolute path to request.json>]` in a fresh
per-iteration directory, with stdin closed and a minimal environment.

`request.json`:

```json
{"contract": "ares-lite-solver/1", "iteration": 3, "seed": "<64 hex>", "deadline_unix": 1791200000,
 "limits": {"max_module_bytes": 131072, "timeout_seconds": 900},
 "challenge_path": ".../challenge.json", "season_rs_path": ".../season.rs",
 "baseline_path": ".../baseline.wasm", "manifest_path": ".../manifest.json",
 "out_dir": "<the iteration directory>", "solution_file": "solution.wasm", "response_file": "response.json"}
```

- `iteration` increases by one per run, so you can use it to explore variants.
  `seed` is fresh randomness for your own search. It doesn't enter scoring.
- `challenge.json` holds the task spec and public examples. `season.rs`
  holds the same constants as Rust (`const` items) for the starter crate.

## Output

Write `<out_dir>/solution.wasm` and `<out_dir>/response.json`:

```json
{"contract": "ares-lite-solver/1", "status": "ok", "solution": "solution.wasm",
 "notes": "free text", "claimed_score": 0}
```

`status` is `ok` or `no_solution`. Exit code 0 means the run happened; any
other exit code, a timeout, or a missing or invalid response counts as no
solution.

## What the Miner enforces

- The solution file must be inside the iteration directory (no `..`, no
  symlink) and at most 131,072 bytes. `response.json` must be at most 64 KiB.
- The module is re-encoded with the frozen codec, then run by the deterministic
  verifier: all-or-nothing correctness, then metered fuel on the practice cases.
- Only modules that beat the best score you have already committed are committed.

## Solvers in this SDK

| Ref | What |
|---|---|
| `builtin:baseline` | the season baseline unchanged (no toolchain needed; scores 0%) |
| `builtin:starter` | the starter Rust crate, searching its shipped variants (`compact`, `memo`, `bound` …); needs Rust + wasm32 |
| `examples/solvers/my_rust_solver` | **your** copy of the starter crate: edit `crate/src/lib.rs` |
| `examples/solvers/template_any_language` | the whole contract in ~25 lines with no SDK import; port it to any language |

GPU, remote and AI-agent workers fit the same contract. Your `command` can
call a GPU job, a remote build service, or an agent loop that edits code and
builds it. The Miner only sees the module it returns.
