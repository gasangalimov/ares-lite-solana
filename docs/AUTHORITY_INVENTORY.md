# ARES Lite — Authority inventory

Status: **Updated for program v3 and the devnet multisig rehearsal** (SECURITY_MODEL.md has the per-power table). v3 is v2 (fixed supply) plus:
- an on-chain epoch cap;
- a pinned close;
- a verification window;
- `cancel_root`.

The v1 table below (mint authority = PDA) is superseded.

## Program v3 changes to the admin's power

| Power | v2 | v3 |
|---|---|---|
| Amount of an epoch root | anything ≤ `unlocked(now) − committed` (up to the whole released schedule) | ≤ `close_cap` = ARES unlocked since the previous settlement, ≤ 7 days of release, fixed at close |
| Change submissions after scoring | possible off-chain | log head pinned on-chain by `close_season`; publish requires it |
| Who wins | admin's root | deterministic finalize; `results_digest` on-chain; anyone verifies during `claim_delay`; a wrong root is ≤ one epoch cap and detectable (DETERMINISTIC_FINALIZE.md) |
| Undo a bad root | impossible | `cancel_root`: latest settlement, before claims open, nothing claimed |
| Upgrade authority | single key | unchanged on devnet; transfer plan in UPGRADE_AUTHORITY_PLAN.md |

## Program v2 (fixed supply)

| Authority | Holder | Capability | Can bypass caps? | Rotation | Mainnet acceptable? |
|---|---|---|---|---|---|
| **Program upgrade authority** (ProgramData) | devnet v0.1: **2-of-3 Squads v4 multisig** (vault `3Z2PMt7d…`, time lock 120 s; DEVNET_EVIDENCE.md). Earlier stage: deployer key | replace program code | **YES for the vault and schedule**: new code could transfer vault ARES or ignore the schedule. It **cannot mint**, because the SPL mint authority is None, and that holds regardless of program code. The vault-authority PDA signs for whatever the program id runs, so a PDA does **not** protect against this | `solana program set-upgrade-authority` → multisig plus timelock, or `--final` | **NO** as a single key |
| **SPL mint authority** | **None** (revoked at genesis) | — | — | impossible to set again | Yes |
| **Freeze authority** | **None** | — | — | — | Yes |
| **Lite protocol admin** (`config.admin`) | devnet v0.1: **2-of-3 Squads v4 multisig** (vault `8CmWkaND…`); old deployer rejected with NotAdmin | create seasons; publish one root per season; award funded bounties | **NO** for schedule and supply: commitments ≤ `unlocked(now)`, claims ≤ committed, no mint, no burn of others. **YES** for *who* gets an unlocked epoch budget or an escrowed bounty | `set_admin` | NO as a single key; multisig minimum |
| **Reward publisher** | = protocol admin | chooses the epoch winner (root) | bounded by `unlocked(now)`; omission possible | `set_admin` | NO without accepting the omission trust assumption |
| **Vault authority** | PDA `["vault_authority", mint]`, no key | signs vault transfers only inside `claim` | NO (program logic), unless the program is upgraded | — | depends on the upgrade authority |
| **Bounty escrow authority** | bounty PDA, no key | pays the escrow once in `award_bounty` | NO | — | depends on the upgrade authority |
| **Founder wallet** | `FOUNDER_WALLET` (owner's key) | owns its 100M ARES like any holder | NO: no special program rights | owner's choice | Yes (disclosed) |
| **Backend receipt-signing key** | operator Ed25519 key, pinned per season | signs receipts | NO | new season | transparency only |

**Key point.** After genesis nobody can mint. That is enforced by SPL Token
itself, not by this program. The 900M vault and the release schedule are
only as safe as the **program upgrade authority**. On mainnet it must be a
multisig with a timelock, or finalized after an audit.

## Program v1 (superseded, kept for history)

| Authority | Holder (Season Zero devnet) | Capability | Can bypass caps? | Rotation | Mainnet acceptable? |
|---|---|---|---|---|---|
| **Solana program upgrade authority** (BPF loader-upgradeable, ProgramData) | Deployer key, kept outside the repo (`~/.config/ares-lite/deployer.json`) | Replace the program code arbitrarily | **YES.** New code can mint to anyone, change caps, replace roots and ignore receipts. **The PDA mint authority does not protect against this**: the PDA signs for whatever code the program id currently runs | `solana program set-upgrade-authority <id> --new-upgrade-authority <multisig>`, or `--final` (irreversible) | **NO** while a single key holds it. Requires a multisig plus timelock, or `--final` after audit, by a separate owner decision |
| **Lite protocol admin** (`config.admin`) | Operator key (`admin.json`, outside the repo) | `create_season` (within the immutable global cap); `publish_reward_commitment` once per season; `set_admin` | **NO** for caps: season Σ ≤ global cap, and claims ≤ committed total ≤ season cap (on-chain checks). **YES** for *allocation inside a cap*: it can publish a self-serving root | `set_admin` → multisig (tested) | NO as a single key; at minimum a multisig, plus off-chain recomputation by independent parties |
| **Token mint authority** | PDA `["mint_authority", mint]` of the program (no private key) | MintTo, only inside `claim` after a Merkle proof and cap checks | NO (bounded by program logic), **unless the program is upgraded** (see first row) | Immutable for that mint | Yes as a mechanism; depends entirely on the upgrade authority |
| **Freeze authority** | **None** (rejected at initialize) | — | — | — | Yes (by design: nobody can freeze holders) |
| **Reward publisher** | Same key as the protocol admin | Chooses the root (and thereby the allocation) once per season | Bounded by caps (above); omission possible (private research notes, not published) | `set_admin` | **NO**: operator omission is a mainnet blocker unless explicitly accepted |
| **Backend receipt-signing key** | Operator Ed25519 key (`receipts.json`, outside the repo); its pubkey is pinned in the season manifest | Signs `(manifest_hash, seq, head)` receipts; cannot mint or publish roots | NO | New key ⇒ new season manifest (pinned per season) | Acceptable as transparency only; its compromise allows forged receipts, which signed checkpoints and the published log expose |
| **Operator server** | Operator | Orders, accepts or rejects submissions; sees early reveals | NO (no on-chain power) | — | Operational trust; censorship is not provable |
| **Beacon producer** | Solana leader at the declared slot | Withhold one block (≈1 bit of bias) | NO | — | Needs a stronger beacon for value |
| **Participant keys** | Participants' own wallets | Sign their own claim | NO | Their choice | Yes |

**Key point.** Caps, PDA mint authority, immutable roots and receipt PDAs are
only as strong as the program's upgrade authority. On devnet, a single
upgrade key is acceptable and expected. For any production or value-bearing
deployment it is not.
