# FAQ

**Is this a token sale, an investment or an airdrop?**
No. This is a DEVNET / TEST-ONLY release. There is no mainnet, no sale, no
airdrop promise and no statement about price or returns.

**Does Season Zero pay ARES?**
No. Season Zero is points only. Its manifests use the policy
`season_zero_points_v0`, which allocates zero tokens, and the on-chain result
total is 0. Points will not be converted into anything retrospectively.

**Do I have to give my private key to ARES Miner?**
Never. Mining needs only your public address. If you point the Miner at a
Solana CLI keypair file, it reads only the public half.

**Do I need Rust?**
Only to *improve* the starter solver. Without Rust you can join, practice and
submit the baseline. Your own solver can be in any language
([SOLVER_CONTRACT.md](../sdk/SOLVER_CONTRACT.md)).

**Can I use Claude, Codex, 1,000 GPUs, a team?**
Yes. The rules are equal for everyone; resources are not capped. A stronger
solver legitimately ranks higher.

**Can I run 1,000 wallets?**
You can, but identical solvers resolve to one result: equal scores rank by
the earliest commit. Extra wallets help only with genuinely better work.

**Pools?**
Submit under the pool's address. The split is up to the pool.

**Why doesn't my rank show right after I commit?**
Commitments are hashes, so nothing is scored until you reveal. ARES Miner
reveals automatically when commits close. Your local best is shown the whole time.

**Can the operator cheat?**
It can refuse service. It cannot change the challenge, alter or reorder your
submission without contradicting your signed receipt and the on-chain log
head, or fake scores without failing public recomputation (`ares-lite verify`).
See [SECURITY_MODEL.md](SECURITY_MODEL.md).

**When mainnet?**
There is no date and no commitment.
