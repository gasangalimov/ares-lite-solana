# Security

ARES Lite is devnet/test software. Do not use it with real value.

Please report vulnerabilities privately to the repository owner through GitHub
(Security → Report a vulnerability) rather than in a public issue.

Known, documented trust assumptions (docs/SECURITY_MODEL.md):
- upgrade authority: 2-of-3 Squads v4 multisig with a time lock on devnet (not immutable);
- the admin publishes epoch roots, bounded by the on-chain epoch cap and a public verification window (docs/DETERMINISTIC_FINALIZE.md);
- no external audit.
