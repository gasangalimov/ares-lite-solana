# Security policy

DEVNET / TEST-ONLY software with no external audit. Do not use it with real funds.

Report vulnerabilities privately through GitHub: **Security → Report a vulnerability** on this repository.
Please include the affected component (`solana/program`, `sdk/ares_lite`, ARES Miner), steps to reproduce, and the impact.

Scope highlights: on-chain fund or authority bypasses, commitment/reveal binding breaks,
verifier non-determinism, receipt-log equivocation, private-key or salt leakage from the SDK/Miner,
local-app request forgery, and solver sandbox escapes (paths, symlinks, sizes, shell).
