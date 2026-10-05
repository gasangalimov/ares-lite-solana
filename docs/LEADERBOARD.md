# Leaderboard format

There are two boards. Only the final board decides the season.

## Live practice board (during the season, feedback only)

`GET /board` returns a JSON array sorted by `practice_fuel` ascending (valid first):

| Field | Meaning |
|---|---|
| `commit_seq` | position in the hash-chained receipt log |
| `address` | reward address (base58 or hex) of the submission |
| `reason` | `OK` or the first failing gate (e.g. `GATE_WRONG_OUTPUT`) |
| `practice_fuel` | fuel on the **public** practice cases. Everyone can reproduce and overfit it, so it is not the score |

## Final board (after close)

Published as `results.json`. The season's on-chain record holds its
`score_table_digest` and Merkle root, so anyone can recompute both from the
public files.

| Column | Meaning |
|---|---|
| `rank` | by `score` ascending among valid submissions; ties: earliest `commit_seq` |
| `address` | reward address (wallet, pool or multisig) |
| `commit_seq` | receipt-log position |
| `solution_hash` | hash of the revealed solution |
| `valid` | all-or-nothing correctness on the solution-bound gate tests |
| `score` | fuel on the hidden post-close benchmark cases (lower is better) |
| `improvement_bps` | `(baseline − score) / baseline × 10,000` |
| `points` | the epoch's points for the winner (`winner_takes_epoch_v0`); 0 otherwise |

Example row:

```json
{"rank": 1, "address": "5gzQP5u5nMGUJeq4JDzW55GuftUdd75S7iwcYtrfx5QF", "commit_seq": 6,
 "valid": true, "score": 1826385, "improvement_bps": 3699, "points": 100}
```

This example is taken from the devnet evidence run: baseline 2,898,592, best
1,826,385, −36.99%.

## Season metrics (published with the final board)

`return_iteration_rate` counts the cohort of participants with a first valid
submission, and how many of them made a 2nd, 3rd and 5th submission. Also
published: `any_return_rate`, best improvement, pool participation, declared
agents and evaluation cost. These are never used for points.
