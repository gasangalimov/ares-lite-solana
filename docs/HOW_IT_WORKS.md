# How ARES Lite works (plain language)

> DEVNET / TEST-ONLY. Season Zero is POINTS-ONLY.

**Challenge.** Each epoch publishes one challenge, a family of routing
problems (GRAPH-ROUTE): find the cheapest valid route under resource limits
and class constraints. The exact task is derived from the season manifest
plus a Solana blockhash (the *beacon*) chosen in advance. Nobody, including
the operator, can pick a convenient challenge, and you can recompute it
yourself (`ares-lite join` does this).

**Solver.** Your solver produces a small WebAssembly program (an
"ARES-WASM-V0 module") that solves any instance of that family. The starter
solver is a Rust crate. Your solver can be anything that writes such a
module: your own Rust, a code generator, a search over variants, or an AI
agent editing code. See [SOLVER_CONTRACT.md](../sdk/SOLVER_CONTRACT.md).

**Scoring.** The verifier runs your program in a deterministic interpreter
with metered fuel. There is no clock, file system or network inside it.
- **Correctness:** one wrong answer on any hidden test makes the module invalid.
- **Score:** the total fuel used on the benchmark cases. Lower is better, and we
  show it as basis points of improvement over the baseline.

Local *practice* scores use public cases, so they are a guide only. *Final*
scores use cases derived from a beacon that appears only after submissions
close, so they cannot be overfit in advance.

**Commit.** You first send only a commitment: a hash that binds your module,
your **public reward address**, this exact epoch and challenge, and a random
salt that stays on your machine. Nobody can learn your module from it, and you
cannot change the module later.

**Reveal.** After commits close you reveal the module and the salt. The
operator accepts it only if it opens your commitment exactly. A reveal from
another epoch, another address or with a changed byte is rejected. ARES Miner
waits for the reveal phase on purpose, so nobody can copy your module and
commit it before you.

**Receipts and close.** Every accepted commit and reveal extends a
hash-chained log, and you get a signed receipt. At close, the operator pins
the log head on Solana devnet. Omitting or reordering your entry later would
contradict your receipt, which proves it.

**Verification.** After close, the operator publishes `results.json`. Its
score-table digest and result root are pinned on-chain. Anyone can recompute
the challenge, every score, the ranking and the digest from public data
(`ares-lite verify`, or automatically in ARES Miner) and compare them with
the chain.

**Leaderboard.** One row per reward address shows rank, best score, number
of submissions and history. It is live during the season (practice cases) and
final after close (hidden cases). See [LEADERBOARD.md](LEADERBOARD.md).

**Pools.** A pool is just a reward address that several people submit for.
The protocol doesn't care how they split points among themselves. Pool and
AI-agent names are optional metadata and never scored.

**Why copies do not count.** Points reward a better solver, not more
wallets. A solver copied into 1,000 wallets is still one solver: identical
scores rank by the earliest commit, and the result goes to the single best
one. A copy can only come later than the original, so it earns nothing.
Extra wallets help only when each carries genuinely better work.
