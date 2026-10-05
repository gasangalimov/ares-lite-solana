# Security model

> DEVNET / TEST-ONLY. There is no external audit yet.

## On-chain (program v4, frozen economics)

These are enforced, not promised:
- **No minting:** the SPL mint authority is `None`, so supply is 100,000,000 forever. There is no freeze authority.
- **The mining vault pays out only through `claim`,** against a FINAL epoch result, capped by `cap(e) = floor(A × 379,735 / 10⁹)`. No withdraw, sweep or admin path exists.
- **The founder vesting vault pays out only through `release_vested`,** signed by the founder beneficiary, up to the vested amount (1-year cliff, then linear to year 5). There is no admin path.
- **One correction per epoch,** and only inside the verification window. The original result stays on-chain, and claimable or claimed results cannot be withdrawn.
- Upgrade authority and admin are **2-of-3 Squads multisigs** (devnet). The old deployer key is rejected.

Season Zero publishes `total = 0`, so no tokens move. The on-chain record
holds the manifest hash, the closed log head, and the score digest and root.

## Operator (off-chain)

The operator runs the receipt log and scoring. It cannot:
- change the challenge, which is recomputed from the manifest plus a Solana beacon;
- change your submission, because the commitment binds the module, address, epoch and salt;
- silently drop or reorder you, because your signed receipt and the on-chain log head would prove it;
- fake scores, because anyone recomputes them (`ares-lite verify`).

It *can* delay or censor (refuse service). Your receipts prove it if it does.
Correctness of a published result is **detected** by public recomputation
during the verification window, not **prevented** on-chain.

## Participant SDK and ARES Miner

- **No key custody.** Mining needs only your PUBLIC address. Given a Solana CLI keypair file, the Miner reads only its public half; it never stores the secret or the file path, and never sends either anywhere.
- **Salt secrecy.** Each commitment's salt is written to your season directory (`participant.json`, mode 600) before anything is sent. It leaves your machine only in your reveal, after commits close.
- **Untrusted operator data.** Downloads are size-limited. The challenge, `season.rs` and the manifest hash on Solana are checked at join; the reward beacon and the result are checked at verify.
- **Untrusted solvers.** Solvers run without a shell, with a timeout and a minimal environment, in a fresh directory. Their output must be a regular file inside that directory (no `..`, no symlinks) of at most 131,072 bytes. The module is re-encoded and run only inside the deterministic verifier (no host calls). Claimed scores are ignored.
- **Local app.** The Miner window listens on 127.0.0.1 only and requires a per-launch random token. `Host` and `Origin` checks block DNS rebinding and cross-site requests.
- **No telemetry, no auto-update.** The Miner talks only to the operator URL you join and to the Solana RPC you choose.
- **RPC trust.** The on-chain checks trust the RPC endpoint you configure (default `api.devnet.solana.com`). Use your own RPC if you need independence from it.

## Reporting

Report privately through GitHub (Security → Report a vulnerability).
