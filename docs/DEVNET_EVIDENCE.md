# Devnet evidence (v0.1)

> **Current release: DEVNET / TEST-ONLY.**

Everything below is public Solana devnet data. Verify any entry at
`https://explorer.solana.com/address/<address>?cluster=devnet` or
`https://explorer.solana.com/tx/<signature>?cluster=devnet`.

## Deployment (frozen MVP baseline, 22/22 checks passed, 2026-10-05)

| Item | Value |
|---|---|
| Program ID | `7Xeon6BKCnAf8tNxuM7ZbaQjXH7AyPcjxtSFtxTxvHDc` |
| ProgramData | `8iFeM9rwCjttJQST2zVN7pEnuc2dDBZzfGysroq8AdB7` (134,176 bytes) |
| Binary SHA-256 (reproducible from `solana/program`) | `eeac5f03b6713184b209be81a23a8b14916a6238e83f65e6df829d3f408858e7` |
| Deploy | `51aDBjCjDa73oF1PfGgYRuwwjYhVnyYzVbmX8bJuVxHuQ9WoFoR5aKkLdbdt1wKMMJXc5kqYvCKv3WMCEe2vk8Rp` |
| Mint (6 decimals, mint and freeze authority `None`) | `J79qQp757mrFA4Jn3SY3SvA1MFsQRNCW8CTxgJzakmQA` |
| Genesis (1B; 100M founder; 900M vault; revoke) | `2u68YNtPg489mJKtzxUztTzhyTkxNTz7deqnnb7SDNjJFuCZMHueFqr6Tv7KNk3UXaTPFTBYp2sjSirwaDWVkwBP` |
| Founder wallet / token account | `FC1uk1NC3XBWtPMND3iYkPZA9tVWM5ob9XpTn4srakTt` / `9yd5hsHf9fTR5SqV1DcNGEQkxhEKLQp97dYuBa3Ks5iu` |
| Reward vault / vault authority PDA | `ERCk3RQ1VBW4nhLejE9RXRmo6fTwL3EJVzZDSWukVqGJ` / `HVfNmqEjXWRjvQrca5udAxywwk8uJgwheHkTYUAhfNUs` |
| Config | `EQXqnCBtwCY6AWowtfdc2Z7mo5wPEjivtjeUaXJthaxS` |
| Season (close pinned the log head; on-chain epoch cap 145.448323 ARES) | `8gSi5ARkrcrkYoGW23dS4WsqZxFb2PtkbiqashjB7KYc`; close `3JivH7DhC8PzYK8GswXyB5GSGHgv8qQ2fNitsb8eAEofXxJoUtiePKyH2fuGHzj3WR2MbWE5AfaGgK5LiYtMoxWe` |
| Publish (root + digest) | `2joeZs9CtTeTPoHYWoWmy1dDrZpVEn2bgV9afQDT3wEvrTYomS7yiqoRVC7hmCN6ZPMDKGWm6Yn8po9cuWn8tpGy` |
| Claim inside the verification window | rejected: `0x14` ClaimWindowNotOpen |
| Claim (100 ARES, vault → winner) | `35swLZ69rcSSBcHHGi1Zfae7gFRnEdmdNWyeGD34yVMAfjBRQH1a4xJquXfvSXJn3GutMzdGCqffAyWPjhNvdA8Q` |
| Bounty (10,000 ARES; 1,000 burned; supply 1,000,000,000 → 999,999,000) | `4kcPJXkakLVR5y44zAiU1p9k6NFDN2MvA8FwmX654HSE1SRuv8MtrXRHUuVr3kUirh48GcrB7SfkV7XgPjkcZyCU` |
| Bounty award (9,000 ARES escrow) | `3GuTcK97iEte4nnrCrsNXrswLkry8EYaVg3cenJs7qApvK2oHusQ6AXePY5giFQmdenxq5Nwz33R9r71DoPEC3ku` |
| Mint-death attempts | 6/6 rejected by SPL Token: deployer, founder, random attacker, vault PDA, config PDA, founder unsigned |
| Clean-client reproduction | a fresh directory using only public files and RPC reproduced root, digest and cap; 20/20 on-chain checks |

**Negative tests.** All were rejected on-chain:

| Attempt | Code | Meaning |
|---|---|---|
| double claim | `0xc` | AlreadyClaimed |
| wrong wallet | `0xb` | InvalidProof |
| forged amount | `0xb` | InvalidProof |
| wrong season | `0xb` | InvalidProof |
| root replacement | `0x9` | RootAlreadyPublished |
| publish before close | `0x12` | WrongPhase |
| wrong log head | `0x13` | LogHeadMismatch |
| above epoch cap | `0x15` | EpochCapExceeded |
| non-admin close / cancel / create | `0x3` | NotAdmin |
| cancel after claim | `0x12` | WrongPhase |
| MintTo by former authority / attacker | `0x5` | SPL FixedSupply |
| admin vault drain | `0x4` | SPL OwnerMismatch |

## Authority transfer to 2-of-3 multisigs (Squads v4, program `SQDS4ep65T869zMMBKyuUq6aD6EgTu8psMjkvj52pCf`)

| Role | Multisig | Vault = authority | Threshold / time lock |
|---|---|---|---|
| Program upgrade | `Fsocn2bJxHfGuk2JSjzNSGV9F2DBHadBXHL5tXM3kozH` | `3Z2PMt7dxrMZjUXqBLFkAxrQwW7QXmBR3f7G76yE1GDA` | 2 of 3 / 120 s (rehearsal; production 24–72 h) |
| Protocol admin | `2o9AHvqk1RJkrzQd6G3MuKarBESe9WV18qDxu2gFbxko` | `8CmWkaNDFG9ukSWSgtswnm1QvMkVg7xuE4woCaatwz36` | 2 of 3 / 0 |

| Step | Result | Signature / code |
|---|---|---|
| Upgrade authority → multisig vault | PASS | `4VmDMyZ4Wvj9UZPEVhkTrG78bEbeS9bYtSGsxeiphgRcUmo7RqRsWbbvceWBqvCeYMzhcR6tfP3YqPxagAjQdwLS` |
| Old deployer upgrades alone | rejected | `Incorrect authority provided` |
| Non-member approves | rejected | `NotAMember` |
| Execute with 1 of 3 | rejected | `0x1778` InvalidProposalStatus |
| Execute with 2 of 3 before the time lock | rejected | `0x1785` TimeLockNotReleased |
| Execute with 2 of 3 after the time lock (same-bytes upgrade) | PASS | `3Ng5g4uqV4TJmCPDtkqCn6ykj3erq7LjyEdC4Wm945QQCAUmPn7x1x2sYmgwXRQq6N8W33NN27s23s644kKbNebu` |
| After upgrade | ELF SHA-256 still `eeac5f03…858e7`; authority still the multisig; supply, vault and founder balances unchanged | — |
| Admin → multisig vault | PASS | `8Ebs6ws7tCXwdyvPsvrLCqdLeDrLRsUAi1eQQEaUHtoTFu7fxaGybRTDUThVuJWuFEgK66thDhFWxuoiv3j1PwK` |
| Old deployer performs an admin action | rejected | `0x3` NotAdmin |
| Admin action with 1 of 3 | rejected | `0x1778` |
| Admin action (create season 301) with 2 of 3 | PASS | `54Y6N3ohZw7wsPf4ubBSCAc9DnCtRkvHEV6M3JXmvnNpVKZcvvRGJqofLuPuhi1UCAzQuYN5MmaUaVxTJdY1eTjp` |

In this rehearsal the six multisig member keys were all generated in one
test environment. For any value-bearing deployment, each member must be an
independent person or device.
