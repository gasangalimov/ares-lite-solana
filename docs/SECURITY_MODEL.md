# Security model

> **Current release: DEVNET / TEST-ONLY.** There is no external audit yet.

## What nobody can do (enforced, not promised)

- **Mint more ARES.** The SPL mint authority is `None` (revoked inside genesis). SPL Token enforces this, independent of this program.
- **Freeze anyone's tokens.** The freeze authority was never set.
- **Withdraw from the reward vault** except through `claim` against a published, capped epoch root. The program has no withdraw, recovery, sweep, pause or emergency instruction.
- **Burn other people's tokens.** SPL Burn requires the owner's signature. The bounty burn is always on the funder's own tokens.
- **Claim twice, claim for someone else, or claim before the verification window.** One receipt PDA exists per (season, claimant), the destination must be owned by the claimant, and the claim must come after `claim_open_ts`.

## Powers that remain, and how they are bounded

Era 0: the maximum epoch cap is 1,724,846 ARES (7 days of release), and the release rate is 246,406.57 ARES/day.

| Power | Holder (devnet deployment) | Worst case if compromised | Bound / mitigation |
|---|---|---|---|
| Program upgrade | **2-of-3 Squads v4 multisig** with a time lock | Replace the code and move the remaining reserve (≤ 900M) | Threshold of 2 independent signers; the time lock gives honest members time to cancel; no single key |
| `set_admin` | **2-of-3 admin multisig** | Hand the admin powers to someone else | Threshold |
| `publish` (epoch root) | admin multisig | Pay one epoch's amount to a wrong address | ≤ the on-chain epoch cap per settlement; the input log is pinned before scoring; the result digest is on-chain; anyone can re-run the verification during the claim delay; `cancel_root` exists before claims open |
| `close_season`, `cancel_root`, `create_season` | admin multisig | Delay or censor an epoch | No theft possible: amounts are program-computed, and cancel is limited to the latest unclaimed root inside the window |
| `award_bounty` | admin multisig | Misdirect a funded bounty escrow | ≤ the escrowed bounty amounts |
| Submission ordering, operator server | operator | Reorder, omit or see reveals early | Signed, hash-chained receipts make equivocation provable; omission is visible to the affected participant |

**Not removable by on-chain checks today.** Solana cannot run the WASM
verifier, so a wrong root is *detected* by public re-computation, not
*prevented*. For any value-bearing deployment this requires:
- an admin multisig;
- a claim delay of 72 h or more;
- at least one independent watcher.

## Multisig rehearsal (devnet)

Upgrade authority and admin were moved from a single deployer key to two
separate 2-of-3 Squads v4 multisigs. On devnet, these were then shown:

| Attempt | Outcome |
|---|---|
| Old key upgrades | rejected |
| Non-member approves | rejected |
| 1-of-3 executes | rejected |
| 2-of-3 executes before the time lock | rejected |
| 2-of-3 after the time lock performs a same-bytes upgrade | succeeded; the deployed bytes re-hash to the reviewed binary |
| Old key performs an admin action | rejected |
| 2-of-3 performs an admin action | succeeded |

Signatures: [DEVNET_EVIDENCE.md](DEVNET_EVIDENCE.md).

The program is **not** immutable (`--final` was not executed). Making it immutable is a separate, later decision.

## Reporting

Report vulnerabilities privately through GitHub (Security → Report a vulnerability).
