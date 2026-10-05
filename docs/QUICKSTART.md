# Quick start

> **Current release: DEVNET / TEST-ONLY.** Do not use with real value.

## Requirements

- Rust (stable). The LiteSVM tests pin 1.97.1 in `solana/tests-svm/rust-toolchain.toml`.
- Solana CLI (Agave 3.x) with `cargo-build-sbf`.
- Python ≥ 3.12 with `httpx` and `pycryptodome`.

## Build and test locally

```bash
git clone https://github.com/gasangalimov/ares-lite-solana && cd ares-lite-solana
cd solana/program && cargo build-sbf -- --locked && cargo test --locked && cd ../..
sha256sum solana/program/target/deploy/ares_lite_rewards.so     # expect eeac5f03…858e7
cd solana/tests-svm && cargo test --release --locked && cd ../..   # 16 LiteSVM tests
python3 -m unittest discover -s tests                             # settlement tests
```

## Local validator rehearsal (genesis + fixed-supply release checks)

```bash
solana-test-validator --reset &            # in another terminal
solana-keygen new -o /tmp/founder.json     # any founder wallet you control
CLUSTER=localnet FOUNDER_WALLET=$(solana-keygen pubkey /tmp/founder.json) release/release.sh
```

The script:
- deploys the program;
- runs the atomic genesis: 1B minted, 100M to the founder, 900M to the vault, mint authority revoked;
- verifies supply, authorities, the schedule and the vault;
- runs the mint-death test and simulates a 10% bounty burn.

It stops on any mismatch.

## Devnet

```bash
python3 release/budget.py --rpc https://api.devnet.solana.com   # exact SOL needed (≈ 0.70 SOL incl. deploy)
CLUSTER=devnet FOUNDER_WALLET=<base58> release/release.sh
```

## Inspect the public devnet deployment

```bash
python3 cli/ares_lite_chain.py verify-fixed-supply --rpc https://api.devnet.solana.com \
  --program-id 7Xeon6BKCnAf8tNxuM7ZbaQjXH7AyPcjxtSFtxTxvHDc --mint J79qQp757mrFA4Jn3SY3SvA1MFsQRNCW8CTxgJzakmQA
```

See [DEVNET_EVIDENCE.md](DEVNET_EVIDENCE.md) for every address and signature.

## Participating in Season Zero

See [PARTICIPATE.md](PARTICIPATE.md).
