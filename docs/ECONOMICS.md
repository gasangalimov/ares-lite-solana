# Economics (frozen, devnet v4)

> DEVNET / TEST-ONLY. NO MAINNET. NO TOKEN SALE. NO AIRDROP PROMISE.
> Season Zero is points-only: Season Zero epochs publish **total 0**.

These are the frozen parameters of the on-chain program. They are stated as facts about code, not as value or price claims.

| Parameter | Value |
|---|---|
| Max supply | 100,000,000 ARES (6 decimals), minted once at genesis; mint authority `None`, freeze authority `None` |
| Founder | 10,000,000 (10%) in a program vesting vault: 1-year cliff, then linear until year 5; non-revocable; released only to the founder beneficiary |
| Mining reserve | 90,000,000 in the mining vault |
| Epoch | 24 h in production (the devnet rehearsal build uses a scaled clock) |
| Epoch reward cap | `cap(e) = floor(A × 379,735 / 1,000,000,000)`, where `A` = vault − held caps − awarded-but-unclaimed |
| Half-life | 1,825 successful (full-cap) epochs ≈ 5 years of daily winners; failed/no-winner/expired epochs deplete nothing |
| Correction | one withdraw → republish per epoch, only inside the 72 h verification window, total ≤ the pinned cap |
| Bounties | 95% to the solver, 5% back into the mining reserve, 0% burn |
| Transfer tax | none |

Full specification: `ARES_LITE_FINAL_ECONOMICS_SPEC_v1` (in the main project), implemented by `solana/program` and tested in `solana/tests-svm` (golden vectors G1–G12, R1, R2, invariants 1–13).
