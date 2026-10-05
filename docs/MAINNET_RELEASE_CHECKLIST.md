# ARES Lite — Mainnet release checklist (GO / NO-GO)

Status: **PREPARED, NOT EXECUTED.** Mainnet execution requires a separate,
explicit owner GO. Everything below has been rehearsed on a local validator
with the same script (`release/release.sh`, `CLUSTER=localnet`). Public
devnet is blocked by the build environment's network policy.

## Irreversible actions (mainnet)

| Action | Reversible? |
|---|---|
| Program deployment | upgradeable until the upgrade authority is finalized |
| SPL mint creation | permanent address |
| Genesis: 1B minted, **100M sent to FOUNDER_WALLET**, 900M to the vault | **irreversible** |
| Mint authority revoked | **irreversible** (that is the point) |
| Upgrade authority → multisig / `--final` | `--final` is irreversible |

## Pre-GO checklist (owner)

- [ ] Review `solana/program/src/lib.rs` at the release commit; record `EXPECTED_SO_SHA256`.
- [ ] Independent security review/audit of the program (strongly recommended).
- [ ] FOUNDER_WALLET decided and double-checked, ideally hardware or multisig. It is **not** stored in the repository.
- [ ] Deployer key and admin key: fresh, offline-backed-up, outside the repository. Plan to move admin to a multisig (`set_admin`).
- [ ] Upgrade-authority plan: Squads multisig plus timelock, or `--final` after audit (AUTHORITY_INVENTORY.md).
- [ ] Mainnet RPC endpoint and ~3–5 SOL for deployment rent and fees.
- [ ] Founder wallet holds ~0.01 SOL (the release burn simulation needs bounty rent), or run with `SKIP_BURN_SIM=1`, which is recorded.
- [ ] Legal review of the token release in your jurisdiction.
- [ ] Accept, or resolve, the remaining blockers below.

## One command sequence (after GO)

```bash
git checkout <reviewed commit> && git status --porcelain   # must be clean
export PATH=$HOME/.cargo/bin:$PATH                          # cargo-build-sbf 4.0.0, platform-tools v1.53
export CLUSTER=mainnet-beta FOUNDER_WALLET=<base58> EXPECTED_SO_SHA256=<reviewed hash>
export ARES_LITE_MAINNET_GO=OWNER-GO-MAINNET-IRREVERSIBLE
release/release.sh            # asks you to type FOUNDER_WALLET; stops at any mismatch
```

`release.sh` steps (each stops on mismatch):
1. Build the reviewed binary (`cargo build-sbf -- --locked`).
2. Verify the binary hash equals `EXPECTED_SO_SHA256`.
3. Deploy, then compare the on-chain program bytes with the reviewed binary.
4. Create the SPL mint (6 dp, freeze authority None) and the founder token account.
5. Atomic genesis: mint exactly 1B; exactly 100M to the founder and exactly 900M to the vault; revoke the mint authority. On-chain post-conditions are checked.
6. `verify-fixed-supply --at-genesis`: totals, mint authority None, freeze authority None, and a simulated mint death test (deployer, random attacker, founder unsigned, PDAs).
7. Verify the emission schedule: genesis timestamp, era 0, next halving exactly 5 years later, curve values, vault = 900M.
8. Verify burn: an unsigned simulation of a founder-funded 10,000 ARES bounty drops simulated supply by exactly 1,000. Nothing is committed.
9. Publish `release/out/mainnet-beta/RELEASE.json`:
   - cluster;
   - commit;
   - binary hash;
   - deployment signature and timestamp;
   - program id, ProgramData address, upgrade authority;
   - mint, founder, vault, PDAs, admin;
   - genesis signature.
10. Reproduce from a clean client using only the public addresses.

After release:
- publish `RELEASE.json` and the Supply page (`ares_lite.py supply --html`);
- run `set_admin` to the multisig;
- execute the upgrade-authority plan.

## Remaining mainnet conditions

Fixed in program v3 (no longer blockers):
- Epoch amount chosen by the admin up to the whole unlocked schedule. It is now the on-chain epoch cap, ≤ 7 days of release.
- Inputs changeable after scoring. The log head is now pinned on-chain before scoring.
- Claims before anyone can verify. There is now a claim delay (72 h default on mainnet) and `cancel_root`.

Conditions that must be met before mainnet:
1. **Upgrade authority** → multisig with a time lock (UPGRADE_AUTHORITY_PLAN.md). As a single key it can move the vault through an upgrade.
2. **`config.admin`** → multisig. One compromised admin key can still publish a wrong root of ≤ one epoch cap. It is detectable in the window, and the multisig cancels it.
3. **At least one independent watcher** runs `verify-season` on every published epoch inside the window.
4. **External audit** of program v3.
5. **Legal review** of a token launch with a 10% creator allocation.
6. ~~Public devnet run of `release/full_run.sh` (22 steps)~~ **DONE 2026-10-05: all 22 steps passed on devnet** (private research notes, not published); program `7Xeon6BKCnAf8tNxuM7ZbaQjXH7AyPcjxtSFtxTxvHDc`).

Known limits, not blockers (documented and accepted as v0):
- single verifier implementation;
- Season Zero challenge quality (R2c: UX/competition test only, (private research notes, not published);
- winner-takes-epoch variance (pools);
- receipt-log omission model (private research notes, not published);
- beacon bias of about 1 bit;
- bounty designation by the admin, bounded by the escrow.

## NO-GO conditions (automatic)

- Any `release.sh` stop.
- `verify-fixed-supply` reports a mint authority or freeze authority that is not None.
- On-chain program bytes differ from the reviewed binary.
- The founder allocation differs from exactly 100,000,000 ARES, or the vault from exactly 900,000,000 ARES.
