# Season Zero — rules

## Build a better algorithm. Beat the baseline.

> **Current release: DEVNET / TEST-ONLY.** Season Zero is **points only**. It has:
> - **no ARES rewards**;
> - **no airdrop promise**;
> - **no retrospective token allocation**;
> - **no mainnet**.
>
> Points have no monetary value and will not be converted into anything.
> Nothing here is an offer, an investment, or a statement about price.

## The task

Write a solver for the season's GRAPH-ROUTE challenge as a Rust → WebAssembly
module.
- **Correctness is all-or-nothing.** A solver that is wrong on any test is invalid.
- **Score.** Among valid solvers, the score is the deterministic execution fuel (instructions) on hidden benchmark cases. These cases are fixed only after submissions close. **Lower is better.**
- **Baseline.** You must beat the season baseline by at least the published threshold (1% in the reference profile).

## Equality of rules, not equality of resources

All of these are allowed, without caps:
- any number of GPUs and CPUs;
- Claude, Codex or any other AI agents, and any API budget;
- teams, companies and labs;
- your own algorithms and tools;
- **mining pools**: submit under the pool's address and split as you like.

**More resources → stronger solver → more points.** That is legitimate and
intended. Nobody is capped for being well-resourced.

What does **not** work:
- **Identical copies.** 1 identical solver × 1,000 wallets creates *one* result, not 1,000. Exact ties go to the earliest commit, so copies and re-submissions of the same solver earn nothing.
- **Identity games.** Identity does not matter at all: no KYC, no one-person-one-wallet. The only rule is that extra wallets add nothing without extra useful work.

## Mechanics

These are fixed for Season Zero and identical to the frozen devnet baseline.

1. **Commit.** Submit a hash commitment that binds your solver and your reward address. The order is a hash-chained receipt log, and you get a signed receipt.
2. **Reveal** your module after commits close.
3. **Close.** The operator pins the receipt-log head on-chain (Solana devnet) before any scoring.
4. **Score** deterministically on hidden cases derived from a post-close beacon. Anyone can reproduce the score table and the result from the public files.
5. **Epoch result (`winner_takes_epoch_v0`).** The best valid solver that beats the baseline by the threshold takes the epoch's points. Ties go to the earliest commit.
6. **Verification window.** The result is published on-chain with its digest. Anyone can re-run the verification before it becomes final.

## Leaderboard

See [LEADERBOARD.md](LEADERBOARD.md). The live board during the season uses
public practice cases and is for feedback only. The final ranking uses the
hidden post-close cases.

## What we measure

The headline metric is the **return iteration rate**. Among participants
whose submissions include a first **valid** one, we count how many made a
**2nd**, a **3rd** and a **5th** submission. Registrations and traffic are
not success metrics.

We also publish:
- the best improvement over the baseline;
- pool participation and the AI agents participants chose to declare (optional, never scored);
- the operator's evaluation cost.

## Fair-play notes

- **No limits** on submissions, compute, agents or wallets.
- **Infrastructure attacks.** Attacking the operator's infrastructure (DoS, etc.) is not participation and is out of scope.
- **Known limitation.** The challenge profile is provisional. Which public solver variant is best changes from instance to instance, and Season Zero tests the competition loop, not a final difficulty.
