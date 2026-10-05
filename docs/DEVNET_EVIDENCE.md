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

## Season Zero end-to-end on devnet (clean external participant)

- result: `PASS`
- network: `solana-devnet (DEVNET / TEST-ONLY, points only)`
- program: `9Tzp3MQFQR9d2VRfreJJgtq3cdYEdaDujRMHVMhpxBoV`
- epoch: `567`
- epoch_account: `66XEvMJKYEzmi1ChoueBX1E8KNKi822iJFLpfkyXpd1m`
- manifest_hash: `71705f08b63b19b5529ec757ffb43c75dc9187fb34a92156bce53314723ea83e`
- reward_policy: `season_zero_points_v0 (total 0 on-chain)`
- create_epoch_signature: `4YdoBpe31eznD9WHFabbkipdXh7wGXxBwhPrczVFC73QQu7JnkNj3u6WdnkksQMzSte3AU9tnDLS8ay6jTe85Ghh`
- close_epoch_signature: `3pJKezNX23DoLEYzutJ9qyCQTZpD7X4PXPuynH88FPSR41ZqEXtBuqMjfxhenabcB8TD4SVvwMzG1gqEZhzuarzw`
- publish_signature: `KBrNPCbJSkbCqSNbGAdAEoQPF5n7iVKrkUYkf9dsNMA9KUoFCnZHQ81Yd37nbYKK7kqMRfJeK5E5Y9pH4f2QpYz`
- finalize_epoch_signature: `5GSbR1HcRDTocCxMcGsJcyChJscezKb1txgRgdnXkWs3EETZ48px9FAVTna9mkuv9mXm3emLivzhrSkLdk49XTt5`
- onchain_status: `FINAL`
- reward_total: `0`
- winners: `0`
- results_digest: `0f57af8da75953e9b9903c01482bf8a78bb274a206e43abf51162265b17cfcd2`
- log_head: `0831b7f8cf979296ddbcdf13bd92fa5c96ec2129024d6cfb1097b772680d0963`
- participants: `2 (ARES Miner window + ares-lite CLI), installed from the public repo in a clean HOME`
- commits_reveals: `3 / 3 (Miner auto-committed 2 improvements during OPEN and auto-revealed at CLOSE_COMMITS)`
- independent_verify: `all_ok=true for both participants (9 checks incl. on-chain manifest hash, log head, results digest, root, total<=cap, reward beacon)`
- leaderboard: `rank 1 = 3a8QWX…LLVZ 45.19% (final hidden cases); identical module from 4QHMbH…gfuR marked duplicate_of commit #1, no rank`
- return_iteration_rate: `{'cohort_first_valid_submission': 2, 'made_2nd': {'participants': 1, 'rate': 0.5}, 'made_3rd': {'participants': 0, 'rate': 0.0}, 'made_5th': {'participants': 0, 'rate': 0.0}}`

## Season Zero launch rehearsal: scheduled round, unattended close

- result: `PASS (operator on HTTPS behind Caddy in a container; NOT yet on a public host)`
- flow: `fresh public clone (8e9385b) -> pip install -> conformance -> ares-miner -> connect public address -> join https operator -> challenge recomputed + on-chain manifest check -> starter solver -> 2 commits -> scheduled commit close -> Miner auto-revealed within ~1 s -> unattended on-chain close/finalize/publish -> verify 9/9 (both participants) -> leaderboard`
- devnet_epoch: `772`
- epoch_account: `J67CStgketDHEA5NhyKA8UV9nhr2JEGH2EmWx8GU97Sv`
- manifest_hash: `d2d6bae06636a9f0d531dc14499883a816d3218b1f5eb156f58c53809f2b1d5d`
- schedule: `{'opens_at': '2026-10-05T17:28:44Z', 'commits_close_at': '2026-10-05T17:50:00Z', 'reveals_close_at': '2026-10-05T18:01:03Z', 'onchain_close_window': ['2026-10-05T18:02:33Z', '2026-10-05T18:02:45Z'], 'results_expected_by': '2026-10-05T18:17:33Z'}`
- round_closed_at: `2026-10-05T18:04:46Z`
- onchain_status: `FINAL`
- reward_total: `0`
- results_digest: `04dd4f8746939bdc12db9dc763795b1a64236edafc32b5e2a3eff1c623dbb934`
- signatures: `{'create_epoch': 'BDtJqQKABT1Z8ehvTxG8tKCCWGgbDxedzNabm29RXV7YT9cPcxnrsAzQRSvtqi14xxLXe5kWqq1acJb9F58khAy', 'close_epoch': '2hfT77kHBkvPzfWzzJycXPX8f59GsmGqbQis47N9ZDTsdUEGwAFnDum7sMv1jkdhriwa31pExdkaUCaCS7mfbdDD', 'publish_result': '5UAYHNycFHwLBwoGNX4cEQ963RCan4QvVnLG1nB1rbMUhTzMY3z15oPwYLRME7VMWKg6ABJk28FBdaaenqmV7VG6', 'finalize_epoch': 'BML5KonMYVe3sZksMLXWE75bXrVPQoqdC3sRrh5c4BuvTtmFbwmawopVeawxUaEz41qCTLmzJxg2SVEusrEoQte'}`
- signature_status: `{'create_epoch': 'finalized', 'close_epoch': 'finalized', 'publish_result': 'finalized', 'finalize_epoch': 'finalized'}`
- admin_path: `create/close/publish via Squads admin multisig, members 1+2 (member 3 free for an independent holder)`
- windows: `GitHub Actions run 37350490335: windows-latest py3.10 + py3.12 and ubuntu: install, conformance, 17 SDK tests, 13-step Miner smoke all PASS`

## History

The v0.1 deployment (program v3, superseded economics) remains unchanged as
historical evidence: program `7Xeon6BKCnAf8tNxuM7ZbaQjXH7AyPcjxtSFtxTxvHDc`,
mint `J79qQp757mrFA4Jn3SY3SvA1MFsQRNCW8CTxgJzakmQA`.
