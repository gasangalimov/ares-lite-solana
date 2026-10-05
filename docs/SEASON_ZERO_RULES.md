# Season Zero rules

## Build a better algorithm. Beat the baseline.

> **DEVNET / TEST-ONLY.** Season Zero is **POINTS-ONLY**:
> - no ARES rewards;
> - no airdrop promise;
> - no retrospective token allocation;
> - no mainnet.
>
> Points have no monetary value and will not be converted into anything.
> Every Season Zero manifest uses the reward policy `season_zero_points_v0`,
> which allocates **zero tokens** by construction. The policy is part of the
> manifest hash pinned on Solana, so you can check it before you join.

## The task

Produce a WebAssembly solver (ARES-WASM-V0 module) for the epoch's
GRAPH-ROUTE challenge.
- **Correctness is all-or-nothing:** one wrong answer on any test makes the module invalid.
- **Score** is the metered fuel on hidden benchmark cases fixed only after
  submissions close. Lower fuel is better; it is shown as basis points of
  improvement over the baseline.
- **Threshold:** a submission counts as beating the baseline only with at
  least the manifest's improvement threshold (1% in the reference profile).

## Equality of rules, not equality of resources

All of these are allowed, without caps:
- any amount of compute;
- any AI agents (Claude, Codex, others) and any API budget;
- teams, companies and labs;
- pools, by submitting under the pool's address.

More useful work gives a better solver and a better rank.

These do not help:
- **Identical copies.** One solver in 1,000 wallets is one result. Equal
  scores rank by the earliest commit, so copies and re-submissions earn nothing.
- **Identities.** Identity is irrelevant: there is no KYC and no
  one-person-one-wallet rule. Extra wallets help only with genuinely better work.

## Phases of an epoch

1. **OPEN:** commit as often as you like. Each commit binds module, address, epoch and salt.
2. **CLOSE_COMMITS:** reveal your commitments. ARES Miner does this automatically.
3. **CLOSE_REVEALS:** the operator pins the closed receipt-log head on Solana devnet, then scores deterministically with seeds from a post-close beacon.
4. **Published:** the score table digest is pinned on-chain (total 0 tokens). Anyone can recompute and verify it during the public verification window.

## Points and ranking

The final ranking is by best final score per reward address, with ties going
to the earliest commit. Points are the ranking itself. The leaderboard shows
rank, best score in bps, submissions and history
([LEADERBOARD.md](LEADERBOARD.md)).

## What we measure

The headline metric is the **return iteration rate**: of the participants whose
submissions include a first **valid** one, how many made a 2nd, a 3rd and a 5th+
submission. Wallet counts and traffic are not success metrics.

## Fair play

- There are no limits on submissions, compute, agents or wallets.
- Attacking the operator's infrastructure (DoS etc.) is not participation and is out of scope.
- **Known limitation:** the challenge profile is provisional. Season Zero tests the competition loop, not a final difficulty.
