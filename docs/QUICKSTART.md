# Quickstart

> DEVNET / TEST-ONLY. Season Zero is POINTS-ONLY: no token reward, no airdrop promise.

## 0. Requirements

- Python 3.10+ (Windows, Linux; macOS works the same way).
- Optional but needed to *improve* a solver: Rust (`https://rustup.rs`) plus
  `rustup target add wasm32-unknown-unknown`. Without Rust you can still
  join, practice and submit the baseline.
- A Solana **devnet** wallet address (Phantom/Solflare in devnet mode, or
  `solana-keygen new`). You only ever paste its **public** address.

## 1. Install

```
git clone https://github.com/gasangalimov/ares-lite-solana
cd ares-lite-solana/sdk
python -m pip install .
```

## 2a. ARES Miner (recommended)

```
ares-miner
```

1. **Wallet:** paste your devnet public address. You can also give the path of a
   Solana CLI keypair file; only its public half is read, and the secret half is
   never stored or sent.
2. **Season:** paste the operator URL from the season announcement, then
   **Join**. The Miner downloads the season, recomputes the challenge from the
   manifest and Solana beacon, and checks the manifest hash on devnet.
3. **START MINING.** You can watch the iterations, your local best score,
   commits, reveals, rank and finally the verification result. **Stop** at any
   time. Your state is on disk, so pressing START again resumes.

Advanced options:
- choose a solver: `builtin:starter`, `builtin:baseline`, or the path to your own `solver.json`;
- set the number of CPU workers;
- set a pool name (metadata only);
- view the log;
- reveal manually.

Headless (servers/VMs): `ares-miner --headless --server URL --address <PUBKEY> [--solver path/to/solver.json]`.

## 2b. Command line

```
ares-lite join --server URL                       # season files + local challenge check + on-chain manifest check
ares-lite solve --solver builtin:starter          # one solver iteration -> my.wasm + local practice score
ares-lite practice --module my.wasm               # score any module locally
ares-lite submit --module my.wasm --address <PUBKEY>   # commit now (salt stays local)
ares-lite reveal                                  # during the reveal phase
ares-lite status                                  # phase, leaderboard, receipts check
ares-lite verify                                  # after close: recompute the result yourself
```

## 3. Write your own solver (10 minutes, or ask an AI agent)

```
cp -r examples/solvers/my_rust_solver ~/my_solver
ares-lite solve --solver ~/my_solver              # first run copies the starter crate into ~/my_solver/crate
```

Edit `~/my_solver/crate/src/lib.rs`. A brief you can hand to an AI agent:

> "Improve `solve()` in crate/src/lib.rs (ARES Lite GRAPH-ROUTE). It must stay
> correct on every input: a single wrong answer makes it invalid. Score =
> metered WebAssembly fuel, lower is better. Keep the ARES-WASM-V0 rules from
> the file header (no floats, no data sections, no panics, no imports beyond
> memory). Measure with `ares-lite solve --solver ~/my_solver`."

Then point ARES Miner at `~/my_solver/solver.json` (Advanced → Solver).

Any language works: see `examples/solvers/template_any_language` and
[SOLVER_CONTRACT.md](../sdk/SOLVER_CONTRACT.md).
