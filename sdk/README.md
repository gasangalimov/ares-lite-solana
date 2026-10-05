# ares-lite-sdk — ARES Lite participant SDK + ARES Miner

> DEVNET / TEST-ONLY · NO MAINNET · NO TOKEN SALE · NO AIRDROP PROMISE · SEASON ZERO IS POINTS-ONLY

```
pip install .            # Python 3.10+; dependencies: httpx, pycryptodome
ares-miner               # the Miner window: connect wallet (public address) -> join -> START MINING
ares-lite --help         # participant CLI
python -m ares_lite.conformance   # reproduce the conformance vectors byte for byte
```

| File | |
|---|---|
| [SPEC.md](SPEC.md) | every security-relevant encoding, explicit and versioned (`ares-lite-sdk/1`) |
| [SOLVER_CONTRACT.md](SOLVER_CONTRACT.md) | how to plug in your own solver in any language (`ares-lite-solver/1`) |
| `ares_lite/vectors/` | conformance vectors (`ares-lite-conformance/1`) including negative cases |
| `ares_lite/_core/` | the frozen generator, verifier, encoding and transcripts (the operator runs the same code) |
| `ares_lite/client.py`, `cli.py` | participant client: join (with checks), practice, commit, reveal, status, verify |
| `ares_lite/miner/` | ARES Miner engine and local window |
| `ares_lite/operator_server.py`, `operator_cli.py` | reference operator (run your own local practice season) |
| `examples/solvers/` | `my_rust_solver` (edit the starter crate) and `template_any_language` |
| `tests/` | `python -m unittest discover -s tests -t .` (the end-to-end test needs Rust + wasm32) |

Windows and Linux are supported; macOS works the same way. Rust (`rustup`
plus `rustup target add wasm32-unknown-unknown`) is needed only to build
Rust solvers.
