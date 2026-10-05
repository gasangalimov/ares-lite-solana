# ARES Lite — Deterministic finalize (no manual winner selection)

Status: program v3 (on-chain part in this repository); the off-chain finalize pipeline runs on the private ARES verifier core. There is
no DAO, no consensus network and no operator discretion over the winner.

## 1. Guarantee

```
same public inputs → same scores → same winner → same amount → same Merkle root
```

Public inputs, all fixed **before** scoring:

| Input | Fixed by | Where |
|---|---|---|
| Season manifest (rules, policy θ, cases, profile, reference variants) | manifest hash on-chain at `chain-create-season` | season account `[10..42]` |
| Challenge | Solana slot-hash beacon at the pinned slot | `challenge_beacon.json` |
| Starter baseline and reference modules | pinned source SHA-256 + deterministic build; module hashes published at open | receipt log OPEN entry, `references.json` |
| Submissions and commit order | hash-chained receipt log (signed receipts) | `receipts.jsonl`; head pinned on-chain at `chain-close` |
| Hidden test and benchmark seeds | post-close beacon at `close_slot + delay` | `reward_beacon.json` |
| Epoch amount `C` | program, at `chain-close` (`season.close_cap`) | season account `[179..187]` |

`finalize` is a pure function of these inputs: integer fuel scores from the
frozen ARES-WASM-V0 verifier, plus integer allocation arithmetic. It has no
clock, no randomness beyond the beacons, and no operator parameter. The
operator may *trigger* `chain-close`, `finalize` and `chain-publish`. They
cannot:
- choose the winner: it is the argmin of the score table;
- choose the amount: it is `min(season_cap, close_cap)`;
- add or remove submissions after close: the log head is pinned on-chain, and publish requires the same head.

## 2. On-chain enforcement (program v3) and what stays off-chain

| Step | On-chain check | Error |
|---|---|---|
| `close_season(log_head, close_slot)` | admin; season OPEN; stores head, slot, `close_cap` = epoch cap now | WrongPhase |
| `publish(root, total, log_head, results_digest)` | season CLOSED; `log_head` == pinned; `total ≤ close_cap`; `total ≤ cap(now)`; `committed + total ≤ unlocked(now)`; stores `results_digest` (= score-table digest), `claim_open_ts = now + claim_delay` | WrongPhase, LogHeadMismatch, EpochCapExceeded, ScheduleExceeded |
| `claim` | `now ≥ claim_open_ts`; Merkle proof; one receipt PDA per (season, claimant); destination owned by the claimant | ClaimWindowNotOpen, BadProof, … |
| `cancel_root` | admin; latest settlement only; before `claim_open_ts`; nothing claimed. Returns the season to CLOSED with the **same** pinned head | WrongPhase |

**Residual trust, stated honestly.** Solana cannot execute the WASM verifier
within transaction limits, so the program cannot check that `root` equals
`finalize(inputs)`. A dishonest or compromised admin can publish a wrong
root. That root is bounded by one epoch cap (≤ 7 days of release) and is
publicly detectable:
1. anyone runs `ares_lite.py verify-season` during the claim delay;
2. a mismatch is cryptographic evidence: pinned head, digest and root are all on-chain.

The remedy is `cancel_root` by the admin multisig. Mainnet therefore
requires:
- `config.admin` = multisig (UPGRADE_AUTHORITY_PLAN.md §3);
- `claim_delay` ≥ 72 h (release.sh default for mainnet);
- at least one independent watcher.

These are a CONDITIONAL item, not a design gap of the reward rule.

## 3. Tie-break analysis

**When ties happen.** The score is total executed fuel on the shared
post-close benchmark cases. It is an integer in the millions, and distinct
algorithms differ by thousands or more. For example, in the full run:
- baseline 2,546,496;
- memo 2,233,833;
- compact 1,309,915.

Exact ties therefore occur essentially only for solvers that behave
identically on the cases, i.e. copies, or byte-different but equivalent
modules (`test_easy_instance_fishing_gains_nothing`).

**Spam race check.** The only realistic race is "commit a known solver
first". Commits are hidden until close, so a copier cannot see this epoch's
competitors.

| Known solver | Race? | Handling |
|---|---|---|
| Public starter variants (memo, bound, compact, …) | yes: everyone can build them at open | **Ratchet**: `reference_features` in the manifest are built at open and become part of the baseline. A known public variant can never beat it by θ, so committing it first wins nothing |
| Previous epoch's winning module | rebuilt solvers for new constants require the source, which reveals do not publish; binary patching is possible but skilled | Earliest commit among exact ties; the copier needs the patched solver to *beat* everyone else, not tie |
| Two independent identical novel solvers | rare | Earliest commit |

**Decision.** Keep **earliest commit wins**. Alternatives considered:
- **Lottery by hash(beacon ‖ solution).** Not identity-based, but byte-different equivalents are free to produce. A copier would grind many variants to win the draw. Rejected.
- **Split between tied solvers.** Each extra copy earns a share, the exact Sybil gain the Permissionless Capital Principle forbids. Rejected.
- **Earliest commit.** Order comes from the hash-chained, signed receipt log. Reordering is detectable (private research notes, not published). No identity or wealth input. **Kept.**

## 4. Reproduce

```
python3 cli/ares_lite.py verify-season --workdir <public season dir>   # rebuilds references, re-scores, compares root/digest/cap with chain
python3 cli/ares_lite.py chain-verify  --workdir <public season dir>   # season account == results; fixed-supply invariants
```

Tests:
- `tests/test_settlement.py::DeterministicFinalizeTests`;
- LiteSVM `deterministic_finalize_guards_log_head_window_and_cancel`;
- full run steps 10–16 and 22 (private research notes, not published).
