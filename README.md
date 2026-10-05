# ARES Lite on Solana

**ARES Lite — competitive algorithm mining settlement layer on Solana.**

Participants compete with deterministic, verifiable algorithm submissions
(Rust → WebAssembly). The best solver of each competition epoch is paid from
a fixed reward reserve by a native Solana program. This repository contains
the **settlement layer**: the SPL-token program, its tests, the client,
supply/tokenomics tooling and the devnet release tooling.

> **Current status: `DEVNET / TEST-ONLY`**
>
> - No mainnet deployment yet.
> - No token sale.
> - No airdrop promised.
> - Devnet/test software only.
>
> This page makes no statement about price, returns or value.

## Token model (ARES, program v3)

| | |
|---|---|
| Token | ARES (classic SPL Token, 6 decimals) |
| Supply | **1,000,000,000 fixed**, minted once in one atomic genesis transaction |
| Creator allocation | **10%** (100,000,000) to an explicit founder wallet, disclosed on the supply page, no vesting |
| Mining / community reserve | **90%** (900,000,000) in a program-owned vault (PDA), used only for epoch rewards |
| Future mint | **none**: the mint authority is revoked inside genesis |
| Freeze authority | **none** |
| Reward release | **5-year distribution halving**: era *n* releases 450,000,000 / 2ⁿ ARES linearly; one competition epoch can pay at most what the schedule unlocked since the previous settlement (≤ 7 days of release) |
| Burn | **10% of every paid challenge bounty** is burned with SPL Burn; 90% escrowed for the solver; no transfer tax, no admin burn |

## What the program enforces

- **Genesis.** One atomic transaction mints exactly 1B. It rejects pre-existing supply, a freeze authority, decimals ≠ 6 or a wrong authority. The post-conditions are checked on-chain.
- **Vault.** Tokens leave the reserve only through `claim`, against a published epoch Merkle root (SHA-256, domain-separated leaves). There is one receipt PDA per (season, claimant), and the destination must be owned by the claimant. No admin, founder or arbitrary withdrawal path exists.
- **Epoch cap.**
  - `close_season` pins the off-chain result log head and fixes the epoch cap.
  - `publish` requires the same head, `total ≤ epoch cap` and `committed ≤ unlocked(now)`.
- **Public verification window.** Claims open only after `claim_delay`. Within it, `cancel_root` can withdraw the latest root if nothing has been claimed.
- **Bounties.** `fund_bounty` burns 10% and escrows 90%; `award_bounty` pays once.

The full design, authorities and threat analysis are in [`docs/`](docs/).

## Layout

| Path | What |
|---|---|
| `solana/program/` | Native Solana program (Rust, `solana-program` 2.3, classic SPL Token) — `Cargo.toml`, `src/lib.rs` |
| `solana/tests-svm/` | LiteSVM integration tests (genesis exactness, mint-death, halving boundaries, vault bypass attempts, burn, epoch cap, verification window) |
| `ares_lite/` | Python client: transactions/PDAs/Merkle (`solana_client.py`, `merkle.py`), `supply.py` (fixed-supply release gate, mint-death test, burn simulation), `tokenomics.py` (schedule mirror) |
| `cli/ares_lite_chain.py` | Settlement CLI: `chain-genesis`, `verify-fixed-supply`, `supply [--html]`, `epoch-budget`, `bounty-fund`, `bounty-award` |
| `release/release.sh` | One release script for localnet / devnet (mainnet refused without an explicit owner GO phrase) |
| `release/budget.py` | Exact devnet SOL budget from live rent (deploy ≈ 0.684 SOL, full run ≈ 0.70 SOL) |
| `tests/test_settlement.py` | Python tests: supply constants vs program source, halving, burn, epoch cap, account layouts |

## Build and test

```bash
# program (Solana platform tools / cargo-build-sbf)
cd solana/program && cargo build-sbf -- --locked && cargo test
# on-chain behaviour in LiteSVM (Rust 1.97.1, pinned in rust-toolchain.toml)
cd solana/tests-svm && cargo test --release
# Python tooling (Python >= 3.12; pip install httpx pycryptodome)
python3 -m unittest discover -s tests
# exact devnet budget (read-only)
python3 release/budget.py --rpc https://api.devnet.solana.com
```

Reviewed program binary (v3): SHA-256
`eeac5f03b6713184b209be81a23a8b14916a6238e83f65e6df829d3f408858e7`.

## Devnet release

```bash
CLUSTER=localnet FOUNDER_WALLET=<base58> release/release.sh   # local validator rehearsal
CLUSTER=devnet   FOUNDER_WALLET=<base58> release/release.sh   # devnet
```

Keys are created under `~/.config/ares-lite/<cluster>`, never inside the
repository. Mainnet requires a separate owner decision and is refused by
default.

## Season Zero

The first public season is **points only** (no token, no airdrop): see
[`docs/SEASON_ZERO_PUBLIC.md`](docs/SEASON_ZERO_PUBLIC.md). The competition
service (challenge generation, deterministic WASM verification,
commit/reveal log, finalize) runs off-chain. This repository contains its
Solana settlement layer.

## Trust model (honest)

- The upgrade authority is a single deployer key on devnet; a multisig transfer is planned ([`docs/UPGRADE_AUTHORITY_PLAN.md`](docs/UPGRADE_AUTHORITY_PLAN.md)).
- The program cannot run the WASM verifier, so a wrong epoch root is bounded by one epoch cap and detectable during the verification window, not prevented on-chain ([`docs/DETERMINISTIC_FINALIZE.md`](docs/DETERMINISTIC_FINALIZE.md)).
- There is no external audit yet.
