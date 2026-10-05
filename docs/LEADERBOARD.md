# Leaderboard and metrics

Operator endpoints (public, read-only). The same code ships in the SDK
(`ares_lite/leaderboard.py`), so anyone can recompute both outputs.

## `GET /leaderboard`

One row per reward address, sorted by rank:

| Field | Meaning |
|---|---|
| `rank` | 1 = best. Ordered by best score, then by the earliest commit reaching it. `null` until a valid reveal |
| `address`, `address_b58` | reward address (wallet, pool or multisig), hex and base58 |
| `best_score_bps` | best improvement over the baseline in basis points (integer) |
| `best_commit_seq` | receipt-log position of that submission |
| `submissions`, `revealed`, `valid` | counts |
| `history[]` | `commit_seq`, `revealed`, `valid`, `score_bps`, `unix` (acceptance time, non-consensus), `solver`, `duplicate_of` |
| `first_commit_unix`, `last_commit_unix` | timestamps (non-consensus) |
| `metadata` | optional self-declared `pool`, `solver`, `ai_agents` (never scored) |
| `score_source` | `practice (public cases, live)` during the season; `final (hidden cases)` after results |

**Copies.** A revealed solution identical to an *earlier* commit from a
*different* address gets `duplicate_of: <earlier commit_seq>`, no score and
no rank. Re-submitting your own solution is not a copy.

## `GET /metrics`

Season Zero metrics (`ares_lite.metrics`):
- `return_iteration_rate`: within the cohort of participants with a first valid
  submission, the number and rate that made a **2nd**, **3rd** and **5th** submission;
- `any_return_rate`, `participants_with_2plus_submissions`, `participants_improving_on_own_first`;
- `best_improvement_bps`, pool participation, declared AI agents, evaluation cost;
- `per_participant` first/second/third attempt details.

## `GET /status`

Returns `phase` (`OPEN` / `CLOSE_COMMITS` / `CLOSE_REVEALS`), log `head`,
counts, `manifest_hash`, `reward_policy` and `points_only`.

## Final results

`GET /results.json` holds the full score table, the baseline and the
`score_table_digest`. The digest and the result root are pinned in the
epoch's Solana account. `ares-lite verify` recomputes everything and compares
it with the chain.
