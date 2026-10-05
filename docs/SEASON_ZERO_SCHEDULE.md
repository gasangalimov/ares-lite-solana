# Season Zero schedule (first 20–30 participants)

> DEVNET / TEST-ONLY · POINTS ONLY · all times **UTC**.

Season Zero runs in **weekly rounds**. One round = one challenge = one devnet epoch.

| When (UTC) | What happens | What you do |
|---|---|---|
| **Monday 16:00** | The round opens. The challenge is fixed from a Solana devnet blockhash taken *after* the round was announced. | Open ARES Miner → **Join** → **START MINING** |
| Monday 16:00 → **Saturday 16:00** | **Commit phase.** Each improvement is committed: a hash that binds your solver and your address. The solver itself stays secret. | Keep mining whenever you like. Stopping and resuming is safe. |
| **Saturday 16:00** | **Commits close.** Nothing new is accepted. | Nothing. A running Miner **reveals by itself within seconds**. |
| Saturday 16:00 → **Sunday 15:58:30** | **Reveal phase (≈ 24 h).** | If your computer was off: open ARES Miner and press START, or run `ares-lite reveal`, before Sunday 15:58. |
| **Sunday ≈ 16:00** | The round is closed on Solana devnet. The final scores use hidden test cases chosen from a devnet blockhash *after* reveals close. The result digest is published on-chain. | Nothing |
| **by Sunday 16:15** | Final leaderboard. | The Miner shows **verified ✓** after recomputing everything itself. Or run `ares-lite verify`. |

**Round 1:** opens **Monday 2026-10-12 16:00 UTC**, commits close **Saturday 2026-10-17 16:00 UTC**, reveals close **Sunday 2026-10-18 15:58:30 UTC**, results by **Sunday 2026-10-18 16:15 UTC**.

Round 1 opens only once the public operator URL is announced.

## Where the exact times live
- The operator publishes the machine-readable schedule at `<operator URL>/schedule.json`. The same data is inside `/status`.
- ARES Miner shows the **next deadline** in your local time, with UTC alongside.

These published times are authoritative. The devnet clock runs in 36-second epochs, so the on-chain close moment is rounded to the next 36-second boundary (at most 36 s later than announced).

## What a missed step costs
| Missed | Effect |
|---|---|
| Commit after Saturday 16:00 | Rejected. Commit earlier: there is no limit on commits, and only your best counts. |
| Reveal after Sunday 15:58 | That commitment is not scored. Your other revealed commitments still count. |
| Miner closed during the reveal phase | Nothing is lost if you reveal before the deadline. The secret is stored on your disk. |

## Why reveals wait
If revealed solvers were visible while commits were still open, anyone could copy the best one. Your commitment fixes your solver and your address from the start. After the reveal, an identical solver committed later by another address is marked as a copy and earns no points.
