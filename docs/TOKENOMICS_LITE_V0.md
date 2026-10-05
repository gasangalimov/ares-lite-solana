# ARES Lite — Token economics v0 (fixed supply)

Status: **implemented and tested**, `solana/program` v3 (v2 + epoch cap, pinned close, verification window). **Not deployed to
mainnet.** Mainnet requires a separate owner GO (see MAINNET_RELEASE_CHECKLIST.md).

Narrative (and the only one): **fixed supply + decreasing reward release +
usage-based burn.** The following phrasings are never used:
- guaranteed appreciation;
- scarcity profit;
- guaranteed returns;
- investment yield;
- passive income;
- "price will rise".

Burning reduces supply; it does not guarantee any price.

## Token

| Item | Value |
|---|---|
| Name / ticker | ARES / ARES (on-chain metadata optional, not required) |
| Standard | classic SPL Token (`Tokenkeg…`), no Token-2022 extensions |
| Decimals | 6 |
| Total supply | **1,000,000,000 ARES**, minted once at genesis |
| Creator allocation | **100,000,000 ARES (10%)** to `FOUNDER_WALLET` (explicit at deployment; never hard-coded) |
| Mining / community reward reserve | **900,000,000 ARES (90%)** in a PDA-owned vault |
| Mint authority after genesis | **None** (revoked inside the genesis instruction) |
| Freeze authority | **None** (genesis rejects a mint that has one) |

There are no other allocations: no team, advisor, marketing, treasury,
private-sale, presale or liquidity carve-out from the 90%.

## Invariants (enforced on-chain or by release checks)

```
TOTAL_GENESIS_SUPPLY = 1_000_000_000 ARES        program const; genesis post-condition
FOUNDER_ALLOCATION   =   100_000_000 ARES        program const; genesis post-condition
REWARD_RESERVE       =   900_000_000 ARES        program const; vault balance checked at genesis
future_mint          = impossible                mint authority = None; no mint instruction exists
current_supply      <= genesis_supply            SPL Token semantics with authority None
current_supply       = genesis_supply - provably_burned   verify_fixed_supply
distributed_rewards <= committed <= unlocked(now)          publish + claim checks
creator allocation   = exactly 10% at genesis    FOUNDER_ALLOCATION * 10 == TOTAL_SUPPLY
```

### How "no new ARES, ever" is proven

1. `initialize` is atomic. In one transaction it mints 900M to the vault and
   100M to the founder, calls `SetAuthority(MintTokens, None)` and then
   re-reads the mint. It aborts unless supply = 1e15, mint authority = None
   and freeze authority = None.
2. SPL Token cannot mint without a mint authority, and cannot set a new
   authority without the current one. The program contains no other MintTo.
   A test asserts that MintTo appears only inside `initialize`.
3. The mint-authority death test simulates and attempts MintTo by:
   - the founder;
   - the deployer/admin;
   - a random attacker;
   - the vault-authority PDA;
   - the config PDA.

   All of them FAIL in both LiteSVM and a real local validator.
4. `verify-fixed-supply` (a release gate) fails if the mint authority or the
   freeze authority is not None.

**Remaining power: the program upgrade authority** can replace the program.
A new program still cannot mint (the authority is None at the SPL level), but
it could transfer vault funds. See AUTHORITY_INVENTORY.md; this is a mainnet
blocker until the authority is a multisig with a timelock, or is finalized.

## Distribution halving (NOT mint halving)

All 1B ARES exist after genesis. Halving reduces the **release rate** of the
existing reward reserve; it never creates tokens.

```
ERA_SECONDS   = 157,788,000 s   (exactly 5 Julian years = 5 × 365.25 × 86,400)
ERA0_BUDGET   = 450,000,000 ARES
ERA_BUDGET(n) = ERA0_BUDGET >> n          (integer halving, base units)
elapsed       = now − genesis_ts          (genesis_ts recorded on-chain at genesis, immutable)
n             = floor(elapsed / ERA_SECONDS)
unlocked(t)   = Σ_{k<n} ERA_BUDGET(k) + ERA_BUDGET(n) × (elapsed mod ERA_SECONDS) / ERA_SECONDS
on-chain:     committed_total_after_publish ≤ unlocked(now)
```

Release is linear within each era. Even a compromised reward publisher cannot
commit or claim future eras early, because the chain clock bounds it.

| Era | Years | Era budget | Cumulative unlocked | Release per day |
|---|---|---|---|---|
| 0 | 0–5 | 450,000,000 | 450,000,000 | 246,406.57 |
| 1 | 5–10 | 225,000,000 | 675,000,000 | 123,203.29 |
| 2 | 10–15 | 112,500,000 | 787,500,000 | 61,601.64 |
| 3 | 15–20 | 56,250,000 | 843,750,000 | 30,800.82 |
| 4 | 20–25 | 28,125,000 | 871,875,000 | 15,400.41 |
| 5 | 25–30 | 14,062,500 | 885,937,500 | 7,700.21 |

Integer rounding:
- Σ floor(450e12 / 2^k) = 900e12 − popcount(450e12) = 900e12 − 16, so
  **16 base units (0.000016 ARES) can never be released and stay in the vault**;
- intermediate products use u128, so there is no overflow;
- tests cover 300 eras and `i64::MAX` elapsed seconds.

Admin cannot accelerate, delay, raise or restore a rate:
- there is no instruction that touches `genesis_ts` or the schedule constants;
- unknown instruction tags fail (tested);
- the schedule is code.

Changing it requires a program upgrade, which is the upgrade-authority risk
above.

## Utility burn (bounties)

```
fund_bounty(amount):  burn = floor(amount × 1000 / 10000)   (BOUNTY_BURN_BPS = 10.00%)
                      SPL Burn(burn) from the funder's own account (funder signs)   → total supply decreases
                      Transfer(amount − burn) → bounty escrow (PDA)
award_bounty:         escrow → solver, exactly once (MVP: designated by the admin/operator)
```

- Example: a 10,000 ARES bounty burns 1,000 ARES (supply −1,000) and leaves 9,000 for the solver. This is verified on LiteSVM and on a local validator.
- Burns are real SPL burns, not a dead wallet.
- There is no transfer tax, no Token-2022 fee, no admin burn and no burning of anyone else's tokens. SPL Burn needs the owner's signature, and the program has no instruction that burns tokens it does not own.

### Burn-rate scenarios (usage → burned supply; no price modelling)

| Burn rate | 1M ARES/yr bounty volume | 10M | 100M | 1B |
|---|---|---|---|---|
| 0% | 0 | 0 | 0 | 0 |
| 2% | 20,000 (0.002%/yr) | 200,000 | 2,000,000 | 20,000,000 (2%/yr) |
| 5% | 50,000 | 500,000 | 5,000,000 | 50,000,000 (5%/yr) |
| **10%** | **100,000 (0.01%/yr)** | **1,000,000 (0.1%/yr)** | **10,000,000 (1%/yr)** | **100,000,000 (10%/yr)** |
| 20% | 200,000 | 2,000,000 | 20,000,000 | 200,000,000 (20%/yr) |

Years of constant volume needed to burn 10% of genesis at 10%: 1,000 / 100 / 10 / 1.

10% creates no technical or UX problem:
- one integer division;
- amounts below 10 base units burn 0, and this is tested;
- the solver's share remains 90%.

It is therefore the Lite v0 parameter. It is a hypothesis to revisit with real
usage, not a sacred constant (it is a code constant, so a change requires a
program upgrade).

## Creator allocation

- It is fully public: 10% to `FOUNDER_WALLET`, shown on the Supply page.
- No vesting was invented in this phase.
- **Optional, for trust** (not implemented): a public lockup, e.g. founder tokens locked in a time-locked escrow with linear release over 2–4 years. It would be a separate owner decision and must not reduce the 90% reserve.

### Disclosure

The Supply page (`ares_lite.py supply --html`) shows the following, all read from chain state and `chain.json`/`RELEASE.json`:
- the founder wallet and its token account;
- the exact allocation (100,000,000 ARES = 100,000,000,000,000 base units);
- the percentage (10% of genesis supply);
- the current founder balance;
- the genesis transaction signature;
- "no vesting".

## Use of the 900M reserve (restriction)

The 900M vault is **only for mining/community rewards**: epoch winners via
`claim`, under the halving schedule. It is **not** used for:
- team or advisors;
- marketing;
- listing fees;
- market making or hidden liquidity;
- a treasury or operations budget.

This is enforced by code, not by promise. The program has no instruction
that moves vault tokens other than `claim` against a published, capped,
pinned-log epoch root. The admin and founder have no withdraw path, which
the LiteSVM tests `founder_admin_and_attacker_cannot_drain_the_vault` and
`vault_cannot_be_bypassed_*` cover. The remaining exception is a program
upgrade (UPGRADE_AUTHORITY_PLAN.md). Liquidity, if any, comes only from
the creator's 10% or from holders (LIQUIDITY_OPTIONS.md).

## Relation to the ARES research protocol

ARES Lite is a separate, simplified settlement profile. It is not the ARES research
protocol, and Lite tokens are not issuance of that protocol.
