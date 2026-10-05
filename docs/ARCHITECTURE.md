# Architecture

```
            participants (any hardware, any AI agents, pools)
                 │  1. commit(hash) ──► signed receipt
                 │  2. reveal(module)
                 ▼
   ┌──────────────────────────────┐        hash-chained receipt log (public)
   │ competition service (off-chain)│ ───────────────────────────────────┐
   │  • challenge from slot beacon  │                                      │
   │  • deterministic WASM verifier │   3. close: pin log head ──────────┐ │
   │  • fuel scoring on hidden cases│   5. publish root + digest ──────┐ │ │
   └──────────────────────────────┘                                   │ │ │
                 │ 4. finalize (pure function of public inputs;       ▼ ▼ │
                 │    anyone can re-run it)              ┌───────────────────────────┐
                 ▼                                       │ Solana program (this repo)│
          results.json + Merkle root                     │  config / seasons PDAs    │
                                                         │  epoch cap = unlocked     │
   ┌────────────────────────┐   claim(proof) ──────────► │  since last settlement    │
   │ winner wallet / pool   │ ◄──── ARES from vault ──── │  reward vault PDA (900M)  │
   └────────────────────────┘    after verification      │  bounty: 10% SPL burn     │
                                 window                   └───────────────────────────┘
        authorities: program upgrade = 2-of-3 multisig (time lock)
                     protocol admin  = 2-of-3 multisig
        SPL mint authority = None (no future mint), freeze authority = None
```

| Layer | Where | Trust |
|---|---|---|
| Token | classic SPL Token: 1B fixed, mint and freeze authority `None` | none (SPL enforces it) |
| Reward vault, schedule, claims, epoch cap, verification window, bounty burn | the native Rust program in `solana/program` | code plus the upgrade authority (2-of-3 multisig) |
| Season operations (close, publish, cancel, bounty award) | admin = 2-of-3 multisig | bounded on-chain (epoch cap, pinned head, window); a wrong root is detectable |
| Challenge, verification, scoring | off-chain competition service | deterministic and reproducible from public files |
| Ordering of submissions | operator's receipt log | signed receipts; equivocation is provable |

Not in this repository: the competition service and the WASM verifier core.
This repository contains the Solana settlement layer and its tooling.
