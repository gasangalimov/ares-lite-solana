# ARES Lite — Exact devnet SOL budget (program v3, reviewed binary)

Binary: `solana/program` v3, 134,176 bytes, SHA-256
`eeac5f03b6713184b209be81a23a8b14916a6238e83f65e6df829d3f408858e7`.
Rent was read live from `api.devnet.solana.com` (`getMinimumBalanceForRentExemption`)
on 2026-10-04. Fees are 5,000 lamports per signature; there are no
priority fees (devnet recent prioritization fee = 0, and the scripts set
none). Recompute any time with:

```
python3 release/budget.py --rpc https://api.devnet.solana.com
```

**Verification.** The plan is not an estimate:
- The local-validator ledger (`release/ledger.py`) equals the plan to the lamport: 955,801,400 planned = 955,801,400 measured at local rent.
- A run with the admin funded at **exactly** the computed minimum, with no airdrop, passed all 22 steps (`NO_AIRDROP=1`).
- A deployer funded with exactly the deploy cost deployed successfully and ended at 0 lamports. The upgradeable loader refunds the buffer before paying for programdata, so the deploy peak is never 2× the rent.

## 1. Program deployment

| Item | Lamports | SOL |
|---|---:|---:|
| ProgramData rent (45 + 134,176 bytes; `max_len` = binary size) | 682,492,920 | 0.68249292 |
| Program account rent (36 bytes) | 833,120 | 0.00083312 |
| Fees: 137 signatures (buffer create 2 + 133 writes × 1,012 bytes + deploy 2) | 685,000 | 0.000685 |
| **Deployment total** | **684,011,040** | **0.68401104** |
| Peak during deploy (buffer 37 + 134,176 bytes + fees; refunded inside Deploy) | 683,137,280 | ≤ total |

## 2. Protocol accounts, all required, all permanent

| Account | Size | Payer | Lamports |
|---|---:|---|---:|
| Mint | 82 | deployer | 1,066,800 |
| Founder ATA | 165 | deployer | 1,488,440 |
| Config PDA | 197 | deployer | 1,651,000 |
| Reward vault PDA | 165 | deployer | 1,488,440 |
| Season PDA | 187 | admin | 1,600,200 |
| Winner ATA | 165 | winner | 1,488,440 |
| Claim receipt PDA | 50 | winner | 904,240 |
| Bounty PDA | 59 | funder | 949,960 |
| Bounty escrow PDA | 165 | funder | 1,488,440 |
| **Critical-flow accounts** | | | **12,125,960** |
| Scratch season for the adversarial checks | 187 | admin | 1,600,200 |

None of these can be closed without changing the program, and that is not
done: receipts and seasons are the double-claim and audit record. The
attacker ATA is temporary; it is closed in-run and its rent returned.

## 3. Transaction fees

| Phase | Signatures | Lamports |
|---|---:|---:|
| Deploy | 137 | 685,000 |
| Critical flow (genesis 2+1, season 1, close 1, publish 1, fund winner 1, claim 1, fund founder 1, bounty fund 1, bounty award 2, sweep 1) | 13 | 65,000 |
| Adversarial extra (fund attacker 1, attacker ATA 1, scratch season 3, close ATA 1, sweep 1) | 7 | 35,000 |
| 16 rejected attacks, 6 mint-death simulations, all verify-* reads | 0 | 0 (rejected at preflight, never landed) |
| **Total, deterministic** | **157** | **785,000** |
| Upper bound if every loader write were resent and landed twice (+133) | 290 | 1,450,000 |

## 4. Minimum balance, critical end-to-end flow

Deploy → genesis (1B, 100M founder, 900M vault, revoke) → season → close →
publish → claim from the vault → bounty fund (10% burn) → award.

| | Lamports | SOL |
|---|---:|---:|
| Net cost (deploy + accounts + fees) | 696,202,000 | 0.696202 |
| + winner wallet float while signing (swept back) | 650,240 | |
| + admin's own rent-exempt minimum (kept, never spent) | 650,240 | |
| **Minimum balance** | **697,502,480** | **0.69750248** |

## 5. Full 22-step adversarial run

| | Lamports | SOL |
|---|---:|---:|
| Net cost | 697,837,200 | 0.6978372 |
| **Minimum balance** (adds attacker float + temporary ATA, winner float, admin minimum) | **701,276,360** | **0.70127636** |
| Retry with `PROGRAM_ID=<deployed>`: no redeploy | net 13,826,160 / min 17,265,320 | 0.0138 / 0.0173 |
| **Recommended funding**: minimum + one retry + fee upper bound | ≈ 0.716 | **0.72** |

## What the rehearsal does to avoid waste (security unchanged)

- **One payer.** The deployer funds every temporary wallet by exact transfer (`ARES_LITE_FUNDER`); there are no faucet calls per wallet.
- **Only wallets that pay get SOL.**
  - Participants commit off-chain, so they need none.
  - The founder gets exactly the bounty rent plus 1 fee and ends at 0.
  - The winner gets exactly its receipt and ATA rent plus fees.
  - The attacker gets exactly its ATA rent plus 3 fees.
- **Temporary accounts closed.** The attacker ATA is closed, and the winner and attacker wallets are swept back to the deployer.
- **Retries never redeploy.** `PROGRAM_ID=` reuses the deployed program after a byte-for-byte check against the reviewed binary.
- **Optional `RECLAIM_PROGRAM=1`.** After the evidence is written, this closes the program and returns 0.68332604 SOL. It ends the devnet deployment, so it is off by default.
- **Rejection reasons are checked.** Every adversarial check must be rejected by the program or SPL Token itself (exact error code), never for lack of fees. The mint-death test uses a funded fee payer, so an unfunded attacker key cannot "fail" for the wrong reason.

## Known non-budget risk

On some beacon instances the starter baseline exceeds the per-test fuel cap
(`GATE_FUEL_EXHAUSTED`) and `finalize` refuses to score. This was observed
once in 5 local runs, on the provisional R2c profile. A retry with
`PROGRAM_ID` reuse costs 0.0138 SOL. This is a Season Zero liveness issue,
not a budget or security issue (private research notes, not published).
