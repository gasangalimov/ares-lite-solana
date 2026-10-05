# ARES Lite — competitive algorithm mining on Solana (devnet)

> **DEVNET / TEST-ONLY · NO MAINNET · NO TOKEN SALE · NO AIRDROP PROMISE · SEASON ZERO IS POINTS-ONLY**
>
> Season Zero points have no monetary value and will not be converted into anything.
> Nothing in this repository is an offer, an investment or a statement about price.

## Build a better algorithm. Beat the baseline.

ARES Lite is a competition for **algorithms**. Each epoch has one published
challenge: a routing problem compiled into a small WebAssembly program. You
write a **solver** that produces that program. It must be **correct** on
every hidden test. Among correct programs, the one that runs with the **least
metered work (fuel)** wins. Anyone can reproduce every score from public files.

You may use any tools you like: your own head, Claude, Codex or other AI
agents, any amount of compute, a team or a pool. More useful work gives a
better solver, and a better solver gives a better rank. Copying one solver into
many wallets adds nothing, because identical results go to the earliest commit.

This does not claim that Season Zero proves AI-driven algorithmic discovery.
It measures one thing: who can make this solver better, and whether people
come back to improve it.

## Current round

| | |
|---|---|
| Operator URL | **announced with Round 1** (paste it into ARES Miner → Season → Join) |
| Round 1 | opens Mon 2026-10-12 16:00 UTC · commits close Sat 2026-10-17 16:00 UTC · reveals close Sun 2026-10-18 15:58:30 UTC |
| Schedule | weekly, see [docs/SEASON_ZERO_SCHEDULE.md](docs/SEASON_ZERO_SCHEDULE.md) (exact times: `<operator URL>/schedule.json`) |

## Start (Windows, Linux, macOS)

**[docs/QUICKSTART.md](docs/QUICKSTART.md)** goes from zero to your first valid submission in about 15 minutes. In short:

1. **Install** Python 3.10+, Git and Rust (`rustup target add wasm32-unknown-unknown`).
2. Run:
   ```
   git clone https://github.com/gasangalimov/ares-lite-solana
   cd ares-lite-solana
   python -m pip install ./sdk
   python -m ares_lite.conformance      # must print CONFORMANT
   ares-miner                           # or: python -m ares_lite.miner.app
   ```
3. In the window: **Connect** your devnet wallet's **public address** → **Join** the operator URL → **START MINING**.

ARES Miner then:
- runs your solver (the built-in starter by default);
- scores every candidate locally;
- **commits** each improvement while commits are open;
- **reveals** automatically when commits close;
- **verifies** the published result independently, including on Solana devnet.

You never give it a private key. Mining needs only your public address, because the commitment binds it.

`python -m ares_lite.smoke` runs a complete private practice round on your machine as a self-test. CI runs the same test on fresh Windows and Linux machines: see `.github/workflows/miner-smoke.yml`.

To write your own solver, follow [docs/QUICKSTART.md](docs/QUICKSTART.md) and
[sdk/SOLVER_CONTRACT.md](sdk/SOLVER_CONTRACT.md). It takes about 10 minutes,
and an AI agent can do it for you.

## What is in this repository

| Path | What |
|---|---|
| `sdk/` | **Public participant SDK + ARES Miner** (`pip install ./sdk`). It holds the canonical encodings ([SPEC.md](sdk/SPEC.md)), the challenge generator and verifier, commit/reveal, local scoring, the solver contract, conformance vectors, the participant CLI `ares-lite`, the miner `ares-miner` and the reference operator server. It needs no private code. |
| `solana/program` | The on-chain program (v4, frozen economics) deployed on devnet |
| `solana/tests-svm` | LiteSVM tests: golden vectors, invariants, property tests |
| `docs/` | Quick Start, schedule, how it works, rules, leaderboard, security model, economics, devnet evidence, FAQ |
| `deploy/` | Operator deployment (Docker + Caddy HTTPS, systemd) and runbook |

## Devnet deployment (v4, frozen economics)

| | |
|---|---|
| Program | `9Tzp3MQFQR9d2VRfreJJgtq3cdYEdaDujRMHVMhpxBoV` |
| Mint (100,000,000 fixed; mint + freeze authority None) | `BcdSq76FgstSMAyJgYJ4wx6LvBebw5NCReKRwQ6UzfTV` |
| Founder vesting vault (10M; 1-year cliff, linear to year 5) | `9nU3yjynF8LtYDqqSAuskCNwWqJUY74wkaY55eRUrBu3` |
| Mining vault (90M) | `5kcs73EFbVNWnrcAFiRJVU1urx6GRxS1s1AxEKLHTWsX` |
| Upgrade authority / admin | Squads v4 2-of-3 multisig vaults (devnet) |

See [docs/DEVNET_EVIDENCE.md](docs/DEVNET_EVIDENCE.md) and [docs/ECONOMICS.md](docs/ECONOMICS.md).

## Verify everything yourself

```
cd sdk
python -m ares_lite.conformance                 # reproduce the conformance vectors byte for byte
python -m unittest discover -s tests -t .       # SDK tests (local end-to-end needs Rust)
ares-lite --dir ~/ares-lite-season verify       # after a season: recompute the result + on-chain checks
```

Security: [SECURITY.md](SECURITY.md), [docs/SECURITY_MODEL.md](docs/SECURITY_MODEL.md), [docs/SDK_MINER_SECURITY_REVIEW.md](docs/SDK_MINER_SECURITY_REVIEW.md). License: MIT.
