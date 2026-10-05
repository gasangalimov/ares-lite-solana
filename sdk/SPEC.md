# ARES Lite participant protocol — SDK spec `ares-lite-sdk/1`

DEVNET / TEST-ONLY. Season Zero is POINTS-ONLY. This file states every
security-relevant encoding a participant or an independent verifier needs.
The reference implementation is the `ares_lite` package in this directory.
Conformance is defined by `ares_lite/vectors/conformance_v1.json`: an
implementation conforms when `compute()` over the same inputs reproduces that
file byte for byte (`python -m ares_lite.conformance`).

Everything below is integers, byte strings and ASCII. There are no floats and
no locale-dependent steps, and no JSON is ever hashed.

## 1. Canonical encoding (restricted deterministic CBOR)

- Allowed major types: 0 (unsigned int), 1 (negative int), 2 (bytes), 3 (UTF-8 text), 4 (array).
- Forbidden: maps (5), tags (6), floats/simple values (7), indefinite lengths.
- Every length/argument uses its shortest form, and exactly one value is
  decoded, with no trailing bytes. Alternate encodings are rejected
  (vectors: `encoding.must_reject`).

## 2. Domain-separated hashing

`H(domain, parts) = SHA3-256( u32be(len(domain)) || domain || Σ (u32be(len(p)) || p) )`

`domain` is ASCII and versioned, e.g. `ARES-LITE/MINING-KEY-REF/v0`. Every
domain is listed in `ares_lite/domains.py` and `ares_lite/_core/canonical/domains.py`.

## 3. Season and challenge

- Manifest: `manifest.json` → `canonical_value()` (a CBOR array; field order in
  `ares_lite/season.py`). `manifest_hash = H("ARES-LITE/SEASON-MANIFEST/v0", [CBOR(manifest)])`.
  The operator pins it on Solana devnet in the epoch account (`create_epoch`).
- The challenge is fully determined by `(manifest, challenge_beacon)`. The
  beacon is the blockhash of the first Solana slot ≥ `challenge_beacon_slot`, so
  anyone can recompute `challenge_id` and the task (`season_challenge`).
- The epoch / season id is part of the `lane_round_key =
  [0, chain_id, protocol_version, season_id, "GRAPH-ROUTE", 0]`. Any reveal
  or commitment from another epoch therefore never matches (replay protection).

## 4. Solution

- A solution is an ARES-WASM-V0 module, re-encoded with the frozen codec
  (`canonicalize_module`).
- `canonical_solution = CBOR([0, "CODE-SYNTH", 0, challenge_id, 0, module_bytes])`.
- `solution_hash = H("ARES/CODE-SYNTH/SOLUTION-HASH/v0", [canonical_solution])`.

## 5. Commitment (binds the reward address)

```
mining_key_ref = H("ARES-LITE/MINING-KEY-REF/v0", [reward_address])         # reward_address = 32-byte Solana pubkey
preimage       = CBOR([0, lane_round_key, ticket_id, challenge_id, solution_hash,
                       mining_key_ref, reward_address, salt])                 # salt = 32 random bytes
commitment     = H("ARES/RACE/COMMITMENT/v0", [preimage])
```

The operator records `(reward_address, commitment)`. A different address,
solution byte, salt, epoch or challenge never opens it (vectors: `negative`).

## 6. Reveal

`reveal = CBOR([0, lane_round_key, ticket_id, challenge_id, canonical_solution, salt])`,
which must be canonical. `reveal_hash = H("ARES/RACE/REVEAL/v0", [reveal])`.
The operator accepts a reveal only while reveals are open, only for a known
unrevealed commit, and only if it re-opens that commit's commitment.

## 7. Receipt log (operator transparency)

Each accepted entry extends a hash chain `head_i = H("ARES-LITE/RECEIPT/v0", [head_{i-1}, u64be(seq), CBOR([kind, fields])])`,
kinds OPEN=0, COMMIT=1, REVEAL=2, CLOSE_COMMITS=3, CLOSE_REVEALS=4, starting at `head_{-1} = 32 zero bytes`
(`ares_lite/submission.py`, `verify_chain`). With a receipt key pinned in the
manifest, every entry is Ed25519-signed. Keep your receipts: they prove inclusion.
At close, the operator pins the closed log head on-chain (`close_epoch`).

## 8. Scoring

- Validity: the module must pass every hidden test (the verifier runs the
  frozen ARES-WASM-V0 interpreter with fuel/memory metering; no clock, file or
  network access).
- Score: total metered fuel over the benchmark cases (lower is better).
  `score_bps` = improvement over the season baseline in basis points (integer).
- Final scoring uses verification/benchmark seeds derived from a beacon that
  is unknown when the log closes. Local practice uses the public
  `practice_seed(0/1)` and can be overfit, so it is only a guide.
- `results.json` and its `score_table_digest` are published, and the digest is
  pinned on-chain (`publish_result`).

## 9. Versioning

Every domain string ends in `/vN`; the SDK spec is `ares-lite-sdk/1`, the
solver contract is `ares-lite-solver/1` (SOLVER_CONTRACT.md), and the vectors
are `ares-lite-conformance/1`. Any change to an encoding requires a new version
string and new vectors.
