# Devnet evidence

> DEVNET / TEST-ONLY. Verify any entry at `https://explorer.solana.com/address/<address>?cluster=devnet`
> or `https://explorer.solana.com/tx/<signature>?cluster=devnet`.

## v4 frozen economics: final adversarial run, 26/26 PASS

| Item | Value |
|---|---|
| Program | `9Tzp3MQFQR9d2VRfreJJgtq3cdYEdaDujRMHVMhpxBoV` |
| Mint | `BcdSq76FgstSMAyJgYJ4wx6LvBebw5NCReKRwQ6UzfTV` (100,000,000; mint + freeze authority None) |
| Genesis | `F69cHmzGvkYTmgqiP2UfbfKJi5R9bHaFTs5FeYwzsW7DmnXDK3Hfnw8v6eQbuM6PFvQXR1pZ1FRo55WC5bXubC4` |
| Founder vesting vault | `9nU3yjynF8LtYDqqSAuskCNwWqJUY74wkaY55eRUrBu3` |
| Mining vault | `5kcs73EFbVNWnrcAFiRJVU1urx6GRxS1s1AxEKLHTWsX` |
| Config | `FEPcZTbJMB4Xcr8uYBRkTtJR1BeouwgzKyXnx4XrPfhq` |

The run covered:
- genesis exactness and mint-death attempts;
- a full season, with commit/reveal, a pinned log head, scoring, publish and claim;
- parallel epochs, no-winner and expired epochs;
- the single correction (R2), its deadline and second-correction rejection;
- outstanding claims, cap guards, reserve-drain and vesting-bypass rejections;
- the 95/5 bounty split;
- the vesting cliff, with an exact release after it;
- the hand-off to Squads 2-of-3 vaults, with the old deployer rejected;
- clean-client reproduction.

All 27 recorded transaction signatures were re-checked as finalized.
The devnet build uses a scaled 36 s epoch so that multi-day flows and the
1-year vesting cliff (365 epochs) could run for real. Production accepts
only 24 h epochs.

## History

The v0.1 deployment (program v3, superseded economics) remains unchanged as
historical evidence: program `7Xeon6BKCnAf8tNxuM7ZbaQjXH7AyPcjxtSFtxTxvHDc`,
mint `J79qQp757mrFA4Jn3SY3SvA1MFsQRNCW8CTxgJzakmQA`.
