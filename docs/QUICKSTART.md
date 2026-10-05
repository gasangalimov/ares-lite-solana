# Quick Start: from zero to your first valid submission

> DEVNET / TEST-ONLY · NO MAINNET · NO TOKEN SALE · NO AIRDROP PROMISE · SEASON ZERO IS POINTS-ONLY.
> You never give anyone a private key. ARES Miner needs only your **public** address.

Time needed: about 15 minutes, most of it installing Python and Rust once.

## 1. Install the tools (once)

**Windows 10/11**
1. **Python 3.10 or newer** from <https://www.python.org/downloads/>. In the installer, tick **"Add python.exe to PATH"**.
2. **Git** from <https://git-scm.com/download/win>, with the defaults. (Instead of Git you can use *Code → Download ZIP* on GitHub.)
3. **Rust** from <https://rustup.rs> (`rustup-init.exe`).
   - If it offers to install the Visual Studio C++ build tools, you may choose **"Proceed without"**: ARES solvers only need the WebAssembly target.
   - Then open a **new** PowerShell window and run:
     ```
     rustup target add wasm32-unknown-unknown
     ```

**Linux / macOS:** install Python 3.10+ and Git from your package manager, then:
```
curl https://sh.rustup.rs -sSf | sh
rustup target add wasm32-unknown-unknown
```

Rust is needed by the built-in **starter** solver, which is the one that finds improvements. Without Rust you can still join and submit the baseline (`builtin:baseline`).

## 2. Get a devnet wallet address

Use any Solana wallet switched to **devnet**: Phantom or Solflare (*Settings → Developer → Devnet*), or `solana-keygen new`.

Copy its **public address** (base58, 32–44 characters). That is all ARES needs. Season Zero needs **no SOL and no transactions** from you.

## 3. Install ARES Lite

```
git clone https://github.com/gasangalimov/ares-lite-solana
cd ares-lite-solana
python -m pip install ./sdk
python -m ares_lite.conformance
```

The last command must print `CONFORMANT`. That proves your install computes every hash exactly like everyone else's.

## 4. Mine

```
ares-miner
```

If your shell cannot find `ares-miner`, run `python -m ares_lite.miner.app` instead.

A local window opens. It runs only on your machine, at `127.0.0.1`.
1. **Wallet:** paste your public address → **Connect**.
2. **Season:** paste the operator URL from the round announcement → **Join**. The Miner downloads the round, recomputes the challenge itself and checks it against Solana devnet.
3. Press **START MINING**.

Then watch:
- **Iterations** and **Local best**: your improvement over the baseline, estimated on public practice cases;
- **Commits / revealed**: each improvement is committed automatically;
- **Next deadline**: when commits or reveals close, in your local time;
- **Rank**, and finally **Verification: verified ✓**.

You can press **STOP** and START again at any time; progress is saved on disk. During the reveal phase, keep the Miner open or start it once. It reveals by itself. See the [schedule](SEASON_ZERO_SCHEDULE.md).

**Your first valid submission** is the first row in *History* with a commit number. It counts once it shows `revealed: yes` after commits close.

## 5. Get better (optional)

The starter solver tries a few known improvements. To go further, write your own:
```
cp -r sdk/examples/solvers/my_rust_solver ~/my_solver
ares-lite --dir ~/ares-lite-season solve --solver ~/my_solver
```

Edit `~/my_solver/crate/src/lib.rs`, then in the Miner choose *Advanced → Solver → `~/my_solver/solver.json`*.

You can hand this brief to an AI coding agent:

> "Improve `solve()` in crate/src/lib.rs (ARES Lite GRAPH-ROUTE). It must stay correct on every input; one wrong answer makes it invalid. Score = metered WebAssembly fuel, lower is better. Keep the ARES-WASM-V0 rules in the file header (no floats, no data sections, no panics, no imports beyond memory). Measure with `ares-lite --dir ~/ares-lite-season solve --solver ~/my_solver`."

Any language works: see `sdk/examples/solvers/template_any_language` and [SOLVER_CONTRACT.md](../sdk/SOLVER_CONTRACT.md).

## Command line instead of the window

```
ares-lite join --server URL                            # season files + local and on-chain checks
ares-lite solve --solver builtin:starter               # one iteration -> my.wasm + local practice score
ares-lite submit --module my.wasm --address <PUBKEY>   # commit (the secret salt stays on your disk)
ares-lite reveal                                       # after commits close
ares-lite status                                       # phase, leaderboard, your receipts
ares-lite verify                                       # after the round: recompute the result yourself
```

For servers without a screen: `ares-miner --headless --server URL --address <PUBKEY>`.

## Troubleshooting

| You see | Do |
|---|---|
| "the starter solver builds Rust: install Rust …" | Do step 1 (Rust and the wasm target), open a **new** terminal, start the Miner again |
| `iteration N: INVALID GATE_FUEL_EXHAUSTED@…` | Normal: that variant is too slow on some case. The Miner keeps searching. |
| "operator rejected /commit: commits are not open" | The commit phase is over. Wait for the next round. |
| `NON-CONFORMANT` | Re-clone with `git clone` (the repository pins byte-exact files; a copy whose line endings were rewritten changes the hashes) |
| Join fails with "Solana RPC check failed" | Devnet RPC hiccup: press Join again. Advanced → RPC lets you use another devnet RPC. |
| Self-test of the whole flow on your machine | `python -m ares_lite.smoke` runs a private practice round and prints PASS/FAIL per step |
