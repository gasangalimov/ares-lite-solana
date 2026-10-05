# Security

ARES Lite is devnet/test software. Do not use it with real value.

Please report vulnerabilities privately to the repository owner through GitHub
(Security → Report a vulnerability) rather than in a public issue.

Known, documented trust assumptions:
- single-key upgrade authority on devnet (docs/UPGRADE_AUTHORITY_PLAN.md);
- the admin publishes epoch roots, bounded by the on-chain epoch cap and a public verification window (docs/DETERMINISTIC_FINALIZE.md);
- no external audit.
