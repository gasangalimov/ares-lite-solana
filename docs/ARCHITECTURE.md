# Architecture

```
 participant machine                         operator (public URL)              Solana devnet (program v4)
 ┌────────────────────────────┐             ┌──────────────────────────┐        ┌──────────────────────────┐
 │ ARES Miner / ares-lite     │  GET season │ operator_server          │        │ epoch account:           │
 │  solver (any language) ──► │◄────────────┤  manifest, challenge,    │ create │  manifest hash  ◄────────┤ create_epoch (multisig)
 │  verifier (local scoring)  │  POST commit│  baseline, receipt log   │ close  │  closed log head ◄───────┤ close_epoch  (multisig)
 │  commit (salt stays local) ├────────────►│  signed receipts         │ publish│  score digest, root ◄────┤ publish_result total 0
 │  reveal at reveal phase    ├────────────►│  /status /leaderboard    │        │                          │
 │  verify ◄──────────────────┤ results.json│  /metrics /results.json  │        │ mint (fixed 100M),       │
 │  (+ on-chain checks) ◄─────┼─────────────┼──────────────────────────┼────────┤ vaults, vesting          │
 └────────────────────────────┘             └──────────────────────────┘        └──────────────────────────┘
```

- **SDK (`sdk/ares_lite`):** canonical CBOR plus framed SHA3 domain hashing; the
  GRAPH-ROUTE generator and the ARES-WASM-V0 verifier (`_core`, identical to
  the operator's implementation, proven by the conformance vectors); season,
  submission and receipt transcripts; scoring; Merkle; the Solana client;
  leaderboard and metrics; the participant client, CLI and Miner; and the
  reference operator server (for local practice seasons).
- **Randomness:** the challenge beacon and the reward beacon are Solana
  blockhashes at slots fixed in advance. The reward beacon comes after the log closes.
- **Authority:** the Squads 2-of-3 admin vault creates, closes and publishes
  epochs. The upgrade authority is a separate 2-of-3 multisig.
