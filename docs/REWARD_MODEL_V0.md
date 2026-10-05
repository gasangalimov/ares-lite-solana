# ARES Lite — Reward model v0 (pool-compatible)

Status: **Lite economics v0, MVP.** It is not declared economically optimal.
Chosen for minimum Sybil surface and maximum simplicity.

## Requirements

1. Not strongly order-dependent.
2. No free gain from 1,000 wallets.
3. Large participants may win more through real compute or capital.
4. Pool-compatible.
5. Understandable.
6. No identity or KYC.

## Options compared

| | A. Winner-takes-epoch + pools | B. Top-N rewards | C. After-close performance-weighted |
|---|---|---|---|
| Rule | best valid solver of the epoch gets the epoch budget | top N solvers split by rank | everyone above baseline shares ∝ improvement |
| Order dependence | only exact ties (earliest commit) | only ties | none (except dedup of identical solvers) |
| 1 solver × 1000 wallets | **no gain**: one winner | gain if tiny variants fill N slots (variants of one solver occupy ranks) | **gain**: each distinct variant earns (measured 1.3× in `reward_order.json`) |
| Big compute → better solver | wins the whole epoch | wins rank 1 share | wins larger share |
| Small participants | high variance → **pools** smooth it (as in Bitcoin) | some chance at lower ranks | steady small rewards |
| Pools | natural: pool address wins, splits off-chain | works | works |
| Explainability | "best solver this epoch wins" | medium | low (formula) |
| Sybil surface | **minimal** | medium | high |
| Verdict | **SELECTED (v0)** | rejected for v0 (variant-flooding) | rejected for v0 (identity-free dedup unsolved) |

Telescoping (earlier Lite research) is **not used for value**:
reward_order_sim showed 0 → full payout depending only on position.

## Lite economics v0 = `winner_takes_epoch_v0`

```
epoch cap     C = unlocked(t_close) − unlocked(max(t_last_settlement, t_close − MAX_EPOCH, genesis))
                  fixed on-chain at chain-close (season.close_cap); MAX_EPOCH = 7 days
publish check total ≤ C  and  total ≤ cap(t_publish)  and  committed + total ≤ unlocked(t_publish)
epoch budget  B = min(manifest season_cap, C)
winner        = argmin score over valid solvers with score ≤ baseline × (1 − θ), θ = 1%
                baseline = best of starter + pinned public reference variants (ratchet)
                ties → earliest commit (copies/identical re-submissions never win)
payout        = B to the winner's reward address (a wallet, a pool, a multisig — any address)
no winner     → total 0 is published; the unlocked amount is not paid now and stays in the vault
```

### Halving era ≠ mining epoch

| | Halving era | Mining / competition epoch |
|---|---|---|
| Length | 5 years (157,788,000 s) | one season: from the previous settlement to `chain-close`, at most 7 days counted |
| Budget | `450M / 2^n` ARES, released linearly | `C` = what the schedule released since the previous settlement |
| Who receives it | nobody directly: it is a release *rate* | the epoch winner (≤ `C`) |

One winner can therefore never receive an era budget. In era 0 the
largest possible epoch cap is 7 days of release:
450,000,000 × 604,800 / 157,788,000 ≈ **1,724,846 ARES** (0.38% of the era budget).
This applies however long the operator waits. Tests:
`tests/test_settlement.py::EpochVersusEraTests` and the LiteSVM tests
`epoch_cap_separates_mining_epochs_from_the_halving_era` and
`exact_five_year_halving_of_the_epoch_cap_on_chain`.

**Telescoping property.** Settlements at t₁ < t₂ < … each ≤ 7 days apart
pay at most Σ C_i = unlocked(t_k): exactly the schedule, never more. Gaps
longer than 7 days forfeit the excess to later epochs. The excess stays in
the vault and is not lost.

- **Pools.** Many members combine AI, API, GPU and research into one strong solver submitted under the pool's reward address. The protocol pays the pool, and the pool distributes to members by its own rules. The protocol imposes nothing on this layer.
- **Why a wealthy participant is not limited.** There is no per-address or per-epoch cap other than the epoch budget, so better resources lead to a better solver and the whole budget.
- **Why wallet multiplication prints nothing.** Exactly one payout exists per epoch, and it goes to the best solver. Identical solvers from 1,000 wallets resolve to the earliest one, so 999 wallets get 0.

### Known limits (honest)

- **Variance.** Solo participants rarely win. Pools are the intended remedy, as in Bitcoin.
- **Public variants.** The ratchet makes pinned public variants (`reference_features`) part of the baseline, so committing a known variant first never wins. Which variant is best differs per challenge instance, so the manifest should pin all public variants it publishes.
- **Near-duplicates.** A 1-fuel improvement on someone else's revealed solver can win the next epoch. Code is published only after close, and threshold θ is relative to the baseline, not to the previous winner. This is a v0 limit.
- **Operator trust.** The operator runs evaluation and publishes the root.
  - The operator cannot choose inputs after close: the log head is pinned on-chain.
  - The operator cannot choose the amount: it is the on-chain epoch cap.
  - Anyone can re-run the result during the claim delay, and `cancel_root` exists for that window.
  - Solana cannot run the WASM verifier, so a dishonest admin *can* still publish a wrong root of at most one epoch cap. This is detected publicly before claims open. The mitigation is an admin multisig plus watchers (DETERMINISTIC_FINALIZE.md).
  - Omission at the receipt log is covered in (private research notes, not published).
- **Challenge quality.** Season Zero results apply (private research notes, not published). Whether algorithmic discovery dominates engineering is not proven yet. R2c is used for UX, execution and competition testing only.

Tests:
- `tests/test_settlement.py::EconomicsV0Tests` (Sybil, ties, order, pools, thresholds);
- `solana/tests-svm` (pool addresses, multiple wallets);
- the rehearsal (`season_zero_rehearsal.sh`, policy `winner_takes_epoch_v0`).
