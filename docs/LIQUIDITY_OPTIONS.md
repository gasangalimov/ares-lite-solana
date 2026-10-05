# ARES Lite — Liquidity options (release option only; nothing created)

Status: **options for an owner decision.** No pool is created by any script
in this repository. No tokens are taken from the 90% miner reserve.

Rules:
- the 90% reward reserve is only reachable through proven reward claims;
- there are no hidden market-maker allocations;
- liquidity is not price support, and no price is promised.

| Option | Source of ARES | Pros | Cons / risks |
|---|---|---|---|
| L0 No initial pool | — | simplest; no creator market activity | no on-chain market until holders create one |
| L1 Creator seeds a small pool from the 10% | creator allocation (public wallet) | transparent; the reserve is untouched | the creator controls the LP position; disclose the amount and LP-token handling (e.g. lock or burn LP tokens) |
| L2 Holders create pools later | earned/claimed ARES | fully organic | thin early markets |

If L1 is chosen later:
1. Publish the amount and the wallet in advance.
2. Use a standard AMM.
3. Disclose LP-token custody (locked or burned).
4. Make no promises about price or volume.

Pool creation is a manual, separate step. It is not part of `release.sh`.
