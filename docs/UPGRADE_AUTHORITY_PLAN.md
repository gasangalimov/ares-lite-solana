# ARES Lite — Upgrade authority plan

Status: applies to program v3 (`solana/program`). Nothing
here has been executed on mainnet. `--final` is **not** executed and must
not be executed without a separate written owner decision.

## 1. Current power (devnet / localnet, stage 1)

| Holder | Power | Limits that do *not* depend on this key |
|---|---|---|
| Upgrade authority = deployer key (single key, outside the repo) | Replace the program code of `PROGRAM_ID` | **Cannot mint**: the SPL mint authority is `None` since genesis, which SPL Token enforces. Cannot move founder or holder tokens. Cannot change past transactions |
| Same key as `config.admin` (until `set_admin`) | create/close seasons, publish one root per closed season, cancel a root inside its verification window, award funded bounties | Root total ≤ on-chain epoch cap (ARES unlocked since the previous settlement, ≤ 7 days of release). Σ commitments ≤ `unlocked(now)`. No vault withdrawal outside `claim`. Log head pinned before scoring |

What the upgrade key **can** do today, by deploying new code:
- transfer the 900M vault (the vault-authority PDA signs for whatever code the program id runs);
- ignore the halving schedule;
- rewrite config/season accounts.

This is the single largest trust assumption of ARES Lite. Acceptable on
devnet. **Not acceptable on mainnet as a single key.**

## 2. Threat model

| # | Threat | Effect | Mitigation (target) |
|---|---|---|---|
| U1 | Upgrade key stolen | Vault drained via malicious upgrade | Key → multisig (2-of-3 minimum, 3-of-5 preferred), members on separate hardware wallets |
| U2 | Upgrade key lost | No bug fixes possible; vault logic frozen as is (not lost: claims still work) | Multisig with spare signer; recovery plan |
| U3 | Rogue insider / coerced signer | Malicious upgrade | Threshold > 1, independent signers, published signer list, time lock so holders/watchers see the pending upgrade |
| U4 | Malicious binary in an honest-looking proposal | Hidden drain | Reproducible build (`cargo build-sbf -- --locked`, pinned toolchain), buffer hash = reviewed commit hash, every signer verifies independently |
| U5 | Upgrade breaks invariants by mistake | Lost or locked funds | Upgrade only with the LiteSVM suite plus the 22-step full run on devnet with the exact buffer; diff review of account layouts |
| U6 | Admin (`config.admin`) compromise | Publishes a self-serving root (≤ one epoch cap) | `set_admin` → separate ops multisig; public verification window (`claim_delay`); `cancel_root` before claims; watchers run `verify-season` |
| U7 | Multisig program itself compromised | Same as U1 | Use a widely deployed, audited multisig (e.g. Squads v4); keep the threshold; consider `--final` later |
| U8 | Premature `--final` | Unfixable bugs | §5 conditions; separate decision |

## 3. Transfer procedure (devnet first, then mainnet only with owner GO)

1. Create the multisig: 3-of-5 preferred, 2-of-3 minimum. Use an audited
   Solana multisig, e.g. Squads v4, with a time lock of ≥ 24–72 h.
   - Members are independent people/devices, on hardware wallets.
   - Publish the member list and threshold.
2. Record the multisig *vault* address (the PDA that will hold authority).
3. Dry run on devnet: deploy a copy, transfer authority, perform one no-op
   upgrade through the multisig, verify.
4. Transfer:
   ```
   solana program set-upgrade-authority <PROGRAM_ID> \
     --new-upgrade-authority <MULTISIG_VAULT> --skip-new-upgrade-authority-signer-check
   solana program show <PROGRAM_ID>          # Authority must equal <MULTISIG_VAULT>
   ```
   `--skip-new-upgrade-authority-signer-check` is needed because a PDA cannot
   co-sign; a typo here is irreversible. Copy the address from the
   multisig UI, then verify it twice.
5. Move protocol admin: `set_admin(<OPS_MULTISIG>)`. This may be a different, faster
   2-of-3 multisig, because every epoch needs close + publish. The LiteSVM
   suite covers `set_admin`.
6. Publish both addresses on the supply page and in `RELEASE.json`.
7. Re-run `verify-fixed-supply` and `chain-verify` from a clean client.

## 4. Emergency upgrade process

Order of preference: the least power first.
1. **Stop new epochs.** The admin simply does not close/publish. Published
   roots stay claimable; nothing else moves. No code change needed.
2. **Withdraw a bad root.** `chain-cancel` within the verification window,
   if nothing has been claimed and it is the latest settlement. The season
   returns to CLOSED with the same pinned log head.
3. **Code upgrade** (bug in the program):
   1. Fix the bug, add a LiteSVM regression test, and tag the commit.
   2. Do a reproducible build and record the SHA-256.
   3. `solana program write-buffer` → `solana program set-buffer-authority <buffer> --new-buffer-authority <MULTISIG_VAULT>`.
   4. Create a multisig upgrade proposal from the buffer.
   5. Each signer independently checks the buffer hash against their own build of the tag.
   6. Wait for the time lock to expire. There is **no bypass**: an emergency does not justify
      one, because a bypass is exactly what an attacker would use.
   7. Execute, then dump the program and compare bytes (`release.sh` step 1b
      logic).
4. Post-mortem published with signatures.

## 5. Eventual `--final` (NOT now)

`solana program set-upgrade-authority <PROGRAM_ID> --final` makes the code
immutable forever. Conditions (all required, separate owner decision):
- Independent audit of program v3+, with no open critical/high findings.
- At least N months of mainnet operation under the multisig with no
  incident. The owner sets N, and it is suggested to be ≥ 12.
- The halving schedule, claim and bounty paths are exercised across at least
  one full season cycle, and the long-horizon tests (era ≥ 64) pass.
- An explicit statement that bugs found later can never be fixed, and that the
  remaining vault then depends only on the immutable code.
- Admin (`config.admin`) is handled first: either a multisig kept forever
  or a trustless root mechanism. Finalizing the code does **not** remove the
  admin's epoch-root power.

## 6. Status

| Item | State |
|---|---|
| Devnet upgrade authority (`ares-lite-devnet-v0.1`) | **2-of-3 Squads v4 multisig** `Fsocn2bJxHfGuk2JSjzNSGV9F2DBHadBXHL5tXM3kozH`, vault `3Z2PMt7dxrMZjUXqBLFkAxrQwW7QXmBR3f7G76yE1GDA`, time lock 120 s (rehearsal) |
| Devnet protocol admin | **2-of-3 Squads v4 multisig** `2o9AHvqk1RJkrzQd6G3MuKarBESe9WV18qDxu2gFbxko`, vault `8CmWkaNDFG9ukSWSgtswnm1QvMkVg7xuE4woCaatwz36` |
| Transfer, negative tests, 2-of-3 same-bytes upgrade after time lock | **PASS on devnet**: DEVNET_EVIDENCE.md |
| Mainnet | **not deployed**. Requires: independent member custody (hardware wallets, separate people), time lock 24–72 h, owner GO |
| `--final` | **not executed**; prohibited without a separate decision |
