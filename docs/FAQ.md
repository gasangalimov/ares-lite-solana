# FAQ

**Is this a token sale or an investment?**
No. The current release is **DEVNET / TEST-ONLY**. There is:
- no mainnet deployment;
- no token sale;
- no airdrop promised;
- no statement about price or returns.

**Does Season Zero pay ARES?**
No. Season Zero is **points only**. Points have no value and will not be
converted retrospectively into tokens.

**Can I use 1,000 GPUs, Claude/Codex/other AI agents, a big API budget, a team?**
Yes. The rule is *equality of rules, not equality of resources*. A stronger
solver legitimately wins more.

**Can I run 1,000 wallets?**
Yes, but identical solvers in many wallets resolve to a single result: the
earliest commit wins exact ties, so copies earn nothing. Extra wallets help
only if each carries genuinely better work.

**Can pools participate?**
Yes. Submit under the pool's address. How a pool splits its result is up to
the pool.

**Who can change the program?**
On the devnet deployment, upgrades need **2 of 3** keys of a Squads v4
multisig, plus a time lock. Season operations (close, publish, cancel,
bounty award) need **2 of 3** keys of a separate admin multisig. The old
single deployer key was rejected on-chain after the transfer
([DEVNET_EVIDENCE.md](DEVNET_EVIDENCE.md)).

**Can anyone mint more ARES?**
No. The mint authority was revoked inside the genesis transaction and the
freeze authority was never set. This is enforced by SPL Token itself,
regardless of program code.

**What is the token model?**
- 1,000,000,000 fixed supply;
- 10% creator allocation (disclosed, no vesting);
- 90% mining/community reserve in a program vault;
- 5-year reward-distribution halving;
- 10% burn on paid challenge bounties.

See [TOKENOMICS_LITE_V0.md](TOKENOMICS_LITE_V0.md).

**What can the operator still do?**
See [SECURITY_MODEL.md](SECURITY_MODEL.md). In short:
- the operator orders submissions;
- the admin multisig publishes results, but only within an on-chain cap, against a pinned input log, and with a public verification window.

A wrong result is detectable by anyone, not prevented on-chain.

**When mainnet?**
There is no date and no commitment. Mainnet needs a separate owner decision,
an external audit, a legal review and independent multisig custody.
