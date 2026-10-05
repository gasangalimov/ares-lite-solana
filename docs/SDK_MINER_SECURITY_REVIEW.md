# SDK + ARES Miner self red-team (Season Zero)

> DEVNET / TEST-ONLY · NO MAINNET · NO TOKEN SALE · NO AIRDROP PROMISE · SEASON ZERO POINTS-ONLY.
> This is an internal self-review of the participant surface only (`sdk/`), **not** an external audit.

Scope:
- the participant SDK (`ares_lite.client`, `cli`, `solver_contract`, `conformance`);
- ARES Miner (`ares_lite.miner`);
- the reference operator endpoints that participants call.

The on-chain program, the economics and the frozen verifier are out of scope here.

## Findings and fixes

| # | Area | Finding | Status |
|---|---|---|---|
| F1 | Miner robustness | An RPC or network error while joining (`httpx.ConnectError`) escaped the local request handler. The window lost the connection and showed nothing. | **Fixed.** Every action failure is reported to the window. RPC failures are wrapped as `ClientError`. The UI handles fetch errors. |
| F2 | Practice harness | The baseline's fuel cost is heavy-tailed. On some challenge instances it exceeds the frozen per-test cap on a *practice gate* case. The client then refused to practise at all ("baseline invalid"). | **Fixed (client only).** The baseline reference fuel is measured on the benchmark cases. Candidates still pass the full practice gate. Consensus scoring is unchanged. |
| F3 | CLI | `ares-lite status` crashed on leaderboard rows without a rank (only invalid submissions, or a copied solution). | **Fixed**, with a regression test. |
| F4 | Windows | Rust solvers were built through `bash build.sh`, which is not available on stock Windows. | **Fixed.** The build runs from Python with the same flags; the output is byte-identical (verified against the published devnet baseline, sha256 `4512507b…`). `build.sh` remains for manual use. |
| F5 | URL handling | `http_post` did not re-check the URL scheme (`http_get` did). | **Fixed.** Only `http(s)://` is accepted, with a regression test. |
| F7 | Miner liveness | Any operator, RPC or build error inside the mining loop ended the Miner thread. A transient outage near the phase change would leave commitments unrevealed. | **Fixed.** Errors are logged and retried with back-off; regression test `test_transient_operator_error_does_not_stop_reveals`. |
| F6 | Copies across wallets | The Season Zero leaderboard ranked an identical module committed later from another wallet. Rewards were not affected (equal scores rank by earliest commit), but the display was misleading. | **Fixed (non-consensus leaderboard).** A revealed solution identical to an earlier commit from another address is marked `duplicate_of` and gets no rank. |

## Checklist

### Key leakage
The Miner and CLI take a public address. Given a Solana CLI keypair file, they read only bytes 32..64, the public half. Tests: `test_keypair_file_reads_only_the_public_half` and `test_keypair_file_is_reduced_to_its_public_key`.

The Miner config (`~/.config/ares-lite/miner.json`) stores an allow-list of fields, and no key material. Nothing signs with a wallet key: Season Zero commitments bind the address and need no signature.

### Shell injection and arbitrary execution
Solvers run with `shell=False` from an argv list, in their own directory, with stdin closed. They get a minimal environment and a timeout. Test: `test_timeout_and_no_shell`.

The solver path can only be set:
- from the local CLI;
- from the token-protected local Miner window.

A web page cannot set it: the local app checks the token, Host and Origin (`test_token_host_and_origin_are_enforced`). A solver is code you chose to run, with your user's rights. Run solvers you trust.

### Malicious solver output and oversized files
These are refused:
- a module larger than 131,072 bytes;
- a response larger than 64 KiB;
- paths that escape the work directory, and symlinks;
- non-canonical or undecodable modules, which are reported invalid and never crash the client.

Tests: `test_malicious_outputs_are_refused` and `test_bad_specs`.

### Malformed challenge data
`join` writes fixed file names only. It then recomputes the challenge from the manifest and beacon, and refuses any mismatch. With RPC enabled, it checks the manifest hash and the challenge beacon against the on-chain epoch account.

Modules, including the operator's `baseline.wasm`, run only inside the deterministic, metered interpreter. Downloads are capped at 8 MiB.

### RPC spoofing
An RPC endpoint you configure is trusted for what it returns. A lying RPC could make a bad operator look consistent. Use an RPC you trust, or cross-check with a second one: `--rpc` and Miner → Advanced → RPC.

The on-chain checks only ever *refuse*. They never move funds and never sign.

### Replay
Commitments bind the epoch lane, challenge, solution hash, reward address and salt. The conformance negative vectors reject:
- another epoch or another challenge;
- a changed address, byte or salt;
- a double reveal, a late reveal or a late commit;
- a duplicate commitment.

### Commit secret leakage
The salt is generated locally and saved before the commit is sent, in `participant.json` with mode 0600 on POSIX. It is sent only in the reveal, after commits close. Revealing early is off by default and labelled "not recommended".

### Telemetry and auto-update
There is no telemetry and no auto-update. Network calls are limited to:
- the operator URL you chose;
- the Solana RPC you chose.

The Miner serves its window on 127.0.0.1 only (`test_binds_localhost_only`).

### Arbitrary file read/write
The local app accepts JSON actions of up to 16 KiB. It has no file-browse or file-serve endpoint. The page is served with a strict CSP and `X-Frame-Options: DENY`.

Writes go to the season directory and the config file only.

### Dependencies
Runtime dependencies are `httpx` and `pycryptodome` (SHA3). Rust is optional, needed only to build Rust solvers.

The frozen encoder, verifier and transcripts are vendored: they are generated from the canonical implementation and checked for drift.

### Repo secrets
The public repo is assembled by an exporter that runs a secret scan before every push. The scan covers keypair arrays, PEM keys, API-key and token formats, local key paths, private imports and forbidden file names. The scanner self-test catches planted secrets.

## Residual risks (open)
- Single operator and receipt key for Season Zero. Omission is detectable from signed receipts, but cannot be prevented.
- RPC trust, as above.
- A solver you install runs with your user's permissions. There is no OS sandbox around solvers. The *verifier* sandbox is the consensus-relevant one.
- No external audit.
