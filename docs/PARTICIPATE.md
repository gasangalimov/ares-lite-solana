# Participating in Season Zero (points only)

> **Status: the participant toolkit is not yet published in this repository.**
> Committing and revealing requires the season's commitment wire format and a
> local practice verifier. Their public release is pending a separate decision.
> Until then, this page describes the flow, and the season will be announced
> here together with the toolkit. No date is promised.

## Flow

1. **Join.** Point the participant client at the season server. It downloads:
   - the public challenge (GRAPH-ROUTE parameters, `season.rs` constants);
   - the baseline module;
   - the agent brief (`/agent.md`), which you can hand to any AI coding agent.
2. **Build.** Compile a solver crate (Rust `no_std` → `wasm32-unknown-unknown`) against the season constants. The starter variants (`memo`, `bound`, `compact`, …) are examples; invent better ones.
3. **Score locally.** Run on the public practice cases with the same deterministic verifier and fuel metering as the official scoring. A solver must be exactly correct.
4. **Submit.**
   - The client computes your commitment *locally*: your code stays private until reveal.
   - It sends the commitment, then the reveal after commits close.
   - You keep the signed receipts.
5. **Iterate.** Submit again whenever you have a better solver. There is no limit on submissions, compute, AI agents, teams or pools.
6. **After close.** Check the final board and verify your receipts against the published log.

## Rules

See [SEASON_ZERO_RULES.md](SEASON_ZERO_RULES.md).
