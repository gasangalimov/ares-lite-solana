# ARES Lite — Public Season Zero (points only)

Status: ready to run on devnet. It is **points only**:
- there is **no token** in Season Zero;
- there is **no airdrop**;
- there is **no promise** that points convert into anything.

Points have no financial value.

## What it is

An open competition. Write a GRAPH-ROUTE solver (Rust → WebAssembly) that
is exactly correct and uses less deterministic fuel than the baseline. Any
method, any AI agent, any budget, any team or pool is allowed
(PERMISSIONLESS_CAPITAL_PRINCIPLE.md).

```
python3 cli/play.py join --server <season URL>
python3 cli/play.py build [--features memo|bound|compact|...]
python3 cli/play.py score
python3 cli/play.py submit
```

The `/agent.md` endpoint gives a self-contained brief you can hand to any AI coding agent.

## Rules

- **Commit/reveal.** Your solver is hidden until close. The order is the signed, hash-chained receipt log.
- **Scoring.** Fuel on hidden post-close benchmark cases. Correctness is all-or-nothing, decided by the frozen ARES-WASM-V0 verifier.
- **Points.** The same deterministic finalize that a value epoch would use (DETERMINISTIC_FINALIZE.md). Anyone can reproduce the table.
- **No limits.** There are no limits on compute, agents, APIs, budget, submissions or wallets.
- **Optional metadata.** Self-declared, never scored, used only for metrics: `--pool <name>`, `--agent <tool>` on commit.

## What we measure (and publish)

The main metric is **return iterations**: do participants come back and
improve? `ares_lite.py season-metrics` reports:

| Metric | Meaning |
|---|---|
| `participants`, `submissions` | totals |
| `return_iteration_rate` | share of participants with ≥ 2 submissions |
| per participant `first` / `second` / `third` | state and score of each submission in commit order |
| `participants_improving_on_own_first` | came back **and** beat their own first submission |
| `best_improvement_bps` | best improvement over the season baseline |
| `pool_participation` | submissions declaring a pool, per pool |
| `ai_agents_declared` | self-declared tools/agents |
| `evaluation_cost` | wall seconds and total fuel to evaluate the season (operator cost) |

The pre-registered kill criteria (private research notes, not published) are
evaluated on these numbers at the end of the season.

## Known limitation (stated up front)

The challenge profile is R2c (24–32 vertices, 130 edges, 8 hops). It is
**provisional**. Earlier calibration did **not** show that algorithmic
discovery dominates engineering tweaks on this profile (private research notes, not published).
Which public variant is best also changes from instance to instance.
Season Zero therefore tests:
- UX;
- execution;
- the competition loop;
- return behaviour.

It does **not** test whether ARES measures algorithmic skill. Season Zero is
not blocked on this, and no new calibration search is started for it.

## Not in Season Zero

- tokens, airdrops, price or yield statements;
- staking, governance, DAO;
- KYC, identity, anti-whale or compute caps;
- referral programs, NFTs, mobile apps, bridges.
