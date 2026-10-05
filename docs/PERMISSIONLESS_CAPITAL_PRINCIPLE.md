# ARES Lite — Permissionless Capital Principle

Status: **MANDATORY** ARES Lite founding principle. Any Lite change that conflicts with it is rejected.

## Principle

**ARES provides equality of rules, not equality of resources.**

ARES does not try to make participants equal in capital. All of the
following are allowed, with no artificial limits:
- large budgets;
- thousands of GPUs and CPU clusters;
- thousands of AI agents;
- expensive APIs, and Claude/GPT/Gemini/local models used at the same time;
- developer teams and proprietary algorithms;
- unlimited experiments;
- mining pools;
- research labs;
- companies;
- individual wealthy participants.

A participant who spends more and thereby builds a better solver **has the
right to win more.**

## What ARES therefore does NOT build

- one-person-one-wallet;
- KYC mining;
- GPU limits;
- AI-agent limits;
- API limits;
- compute equality;
- anti-whale reward caps;
- wealth caps.

## The only anti-Sybil goal

Prevent an extra wallet or identity from creating extra reward **without**
extra useful work or solver contribution.

| Situation | Verdict |
|---|---|
| 1 solver × 1,000 wallets → 1,000× reward | **Attack.** Must be impossible |
| 1,000 GPUs + 1,000 agents → substantially better solver → larger reward | **Intended.** Must be allowed |
| 1,000 wallets each holding a *different, genuinely better* solver | Allowed. Each is real contribution |

1,000 wallets by themselves are not an attack.

## How Lite economics v0 implements it

- `winner_takes_epoch_v0` (REWARD_MODEL_V0.md): the single best valid solver of the epoch is paid. Copies and identical re-submissions never win (ties go to the earliest commit). Extra wallets therefore print nothing, whatever their number.
- There is no per-address cap. A pool or a lab can win the entire epoch budget if its solver is the best.
- Reward addresses are opaque. A pool address, a multisig or a company wallet are all ordinary claimants. How a pool splits its winnings is outside the core protocol.
- There is no identity, KYC or fee per key. The score is a deterministic function of the submitted program only; who or what wrote it is irrelevant.

## Consequences (fixed)

- **Pools are allowed** and are the intended answer to winner-takes-epoch variance.
- **Ties** go to the earliest commit (DETERMINISTIC_FINALIZE.md §3). There is no identity- or wealth-based tie-break, and no lottery that byte-different copies could grind.
- **No compute, agent, API, budget or GPU limits** apply in Season Zero either. Self-declared pool or agent metadata is optional, never scored, and only measured.
- **Rejected and not to be built:**
  - anti-whale caps, one-person-one-wallet, KYC;
  - identity systems, staking-for-eligibility, referral rewards.
