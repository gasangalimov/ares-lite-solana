#!/usr/bin/env python3
"""ARES Lite settlement CLI (DEVNET/TEST-ONLY): the Solana-side commands only.

    chain-genesis -> verify-fixed-supply -> supply [--html] -> epoch-budget
    bounty-fund / bounty-award

The function bodies are the reviewed ones from the ARES Lite operator CLI.
The competition commands (season manifest, commit/reveal log, deterministic
finalize, root publication and claims) need the ARES verifier core and are
not part of this public repository. Private keys are only ever read from or
written to paths outside the repository (default ~/.config/ares-lite/).
Mainnet endpoints are refused unless ARES_LITE_MAINNET_GO is set.
"""

from __future__ import annotations

import argparse
import html
import json
import os
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from ares_lite import solana_client as sc  # noqa: E402

KEY_HOME = Path(os.environ.get("ARES_LITE_KEYS", Path.home() / ".config" / "ares-lite"))


class Workdir:
    """Public addresses of one deployment (chain.json). Never contains keys."""

    def __init__(self, path) -> None:
        self.path = Path(path)

    chain_file = property(lambda self: self.path / "chain.json")


CSS = """
:root{--bg:#fff;--fg:#1b1b1b;--muted:#5d5d5d;--line:#d9d9d9;--accent:#0b5cad;--ok:#1b7f3b;--bad:#a12020}
@media (prefers-color-scheme:dark){:root{--bg:#121212;--fg:#e8e8e8;--muted:#a5a5a5;--line:#333;--accent:#6aa9ff;--ok:#5fcf86;--bad:#ff7b7b}}
body{background:var(--bg);color:var(--fg);font:15px/1.5 system-ui,sans-serif;max-width:960px;margin:0 auto;padding:16px}
h1{margin:.2em 0}h2{border-bottom:1px solid var(--line);padding-bottom:4px;margin-top:2em}
code,pre{font-family:ui-monospace,monospace;font-size:13px}pre{overflow-x:auto;border:1px solid var(--line);padding:8px}
table{border-collapse:collapse;width:100%;font-size:14px}td,th{border-bottom:1px solid var(--line);padding:4px 6px;text-align:left}
.tag{display:inline-block;border:1px solid var(--bad);color:var(--bad);padding:0 6px;font-size:12px}
.muted{color:var(--muted)}.ok{color:var(--ok)}.bad{color:var(--bad)}.wrap{overflow-x:auto}
svg text{fill:var(--fg);font-size:11px}svg .axis{stroke:var(--line)}svg .line{stroke:var(--accent);fill:none;stroke-width:2}
"""


def esc(value) -> str:
    return html.escape(str(value))


def render_supply(snap: dict) -> str:
    """ARES Supply page: every number is read from on-chain state (supply.snapshot)."""

    unit = 10**6

    def ares(v: int) -> str:
        return f"{v / unit:,.6f}".rstrip("0").rstrip(".") + " ARES"

    rows = [
        ("Genesis supply", ares(snap["genesis_supply"])),
        ("Current total supply", ares(snap["current_supply"])),
        ("Burned total", ares(snap["burned_total"])),
        ("Cumulative challenge burns", ares(snap["cumulative_challenge_burns"])),
        ("Creator allocation (10%, at genesis)", ares(snap["creator_allocation_at_genesis"])),
        ("Mining / community reward reserve (90%, at genesis)", ares(snap["reward_reserve_at_genesis"])),
        ("Reward reserve remaining (vault)", ares(snap["reward_reserve_remaining"])),
        ("Distributed to miners", ares(snap["distributed_to_miners"])),
        ("Unlocked by schedule so far", ares(snap["unlocked_now"])),
        ("Current era", f"{snap['current_era']} (era budget {ares(snap['current_era_budget'])})"),
        ("Current reward release rate", ares(snap["current_release_per_day"]) + " / day"),
        ("Next halving (distribution, not mint)", snap["next_halving_utc"]),
        ("Genesis", snap["genesis_utc"]),
        ("Epoch reward cap now (unlocked since last settlement)", ares(snap.get("epoch_reward_cap_now", 0))),
        ("Last settlement", str(snap.get("last_settlement_utc", ""))),
        ("Public verification window before claims", f"{snap.get('claim_delay_seconds', 0)} s"),
        ("Mint authority", str(snap["mint_authority"]) + "  (None = no one can ever mint)"),
        ("Freeze authority", str(snap["freeze_authority"])),
        ("Mint address", snap["mint"]),
        ("Reward vault", snap["reward_vault"]),
        ("Program", snap["program_id"]),
    ]
    body = "".join(f"<tr><th>{esc(k)}</th><td><code>{esc(v)}</code></td></tr>" for k, v in rows)
    creator = [
        ("Founder wallet", snap["founder"]),
        ("Founder token account", snap["founder_token_account"]),
        ("Exact creator allocation", f"{ares(snap['creator_allocation_at_genesis'])} "
                                     f"({snap['creator_allocation_at_genesis']} base units)"),
        ("Share of genesis supply", f"{snap['creator_allocation_at_genesis'] * 100 / snap['genesis_supply']:g}%"),
        ("Founder balance now", ares(snap["founder_balance_now"])),
        ("Genesis transaction", str(snap.get("genesis_signature") or "see chain.json / RELEASE.json")),
        ("Vesting", "none (the allocation is liquid from genesis; disclosed, not locked)"),
    ]
    creator_body = "".join(f"<tr><th>{esc(k)}</th><td><code>{esc(v)}</code></td></tr>" for k, v in creator)
    return (
        "<!doctype html><html lang='en'><head><meta charset='utf-8'>"
        "<meta name='viewport' content='width=device-width,initial-scale=1'>"
        f"<title>ARES Supply</title><style>{CSS}</style></head><body>"
        "<h1>ARES Supply</h1>"
        "<p>Fixed supply · reward release halves every 5 years · usage-based burn. "
        "All values are read from on-chain state and can be reproduced with "
        "<code>ares_lite.py supply</code>. This page makes no statement about price, returns or value.</p>"
        f"<div class='wrap'><table>{body}</table></div>"
        "<h2>Creator allocation disclosure</h2>"
        "<p>10% of the fixed genesis supply was minted to the founder wallet in the same atomic genesis "
        "transaction that minted the 90% mining/community reserve to the program vault and revoked the "
        "mint authority. The founder wallet has no authority over the reserve vault: the vault pays out only "
        "through claims against a published epoch root, bounded by the halving schedule. The 90% reserve is used "
        "only for mining/community rewards: no team, marketing, advisor, listing, liquidity or treasury use.</p>"
        f"<div class='wrap'><table>{creator_body}</table></div></body></html>"
    )


def _print(obj) -> None:
    print(json.dumps(obj, indent=2, sort_keys=True))


def _key(path: str | None, name: str) -> sc.Keypair:
    path = Path(path) if path else KEY_HOME / f"{name}.json"
    if not path.exists():
        kp = sc.Keypair.generate()
        kp.save(path)
        print(f"created {name} keypair at {path} (outside the repository)", file=sys.stderr)
    return sc.Keypair.load(path)


def _chain(work: Workdir) -> dict:
    return json.loads(work.chain_file.read_text())


def _lite(chain: dict) -> "sc.LiteProgram":
    return sc.LiteProgram(sc.b58decode(chain["program_id"]), sc.b58decode(chain["mint"]))


def genesis(rpc: "sc.Rpc", deployer: "sc.Keypair", program_id: bytes, founder_wallet: bytes,
            claim_delay_seconds: int) -> dict:
    """Create the SPL mint and run the program's ATOMIC genesis:
    exactly 1B ARES; 100M -> founder ATA; 900M -> PDA reward vault;
    mint authority revoked (None); freeze authority never set."""

    from ares_lite import tokenomics as tk

    mint = sc.Keypair.generate()  # throwaway signer for the mint address only
    rpc.send(deployer, [
        sc.create_account_ix(deployer.public, mint.public, rpc.rent(82), 82, sc.TOKEN_PROGRAM),
        sc.initialize_mint_ix(mint.public, tk.DECIMALS, deployer.public),
        sc.create_ata_idempotent_ix(deployer.public, founder_wallet, mint.public),
    ], [mint])
    founder_ata = sc.associated_token_address(founder_wallet, mint.public)
    lite = sc.LiteProgram(program_id, mint.public)
    signature = rpc.send(deployer, [lite.initialize(deployer.public, founder_ata, claim_delay_seconds)])
    return {"program_id": sc.b58encode(program_id), "mint": sc.b58encode(mint.public),
            "founder_wallet": sc.b58encode(founder_wallet), "founder_token_account": sc.b58encode(founder_ata),
            "config": sc.b58encode(lite.config), "reward_vault": sc.b58encode(lite.vault),
            "vault_authority_pda": sc.b58encode(lite.vault_authority), "admin": deployer.address,
            "genesis_signature": signature, "claim_delay_seconds": claim_delay_seconds}


def cmd_chain_genesis(args) -> None:
    rpc, work = sc.Rpc(args.rpc), Workdir(args.workdir)
    deployer = _key(args.admin_key, "admin")
    if args.airdrop:
        rpc.airdrop(deployer.public, 2_000_000_000)
    founder = sc.b58decode(args.founder_wallet)
    chain = genesis(rpc, deployer, sc.b58decode(args.program_id), founder, args.claim_delay)
    chain["rpc"] = args.rpc
    work.path.mkdir(parents=True, exist_ok=True)
    work.chain_file.write_text(json.dumps(chain, indent=2) + "\n")
    _print(chain)


def cmd_verify_fixed_supply(args) -> None:
    """Release gate: prints the supply facts and fails on any violation."""

    from ares_lite.supply import mint_death_test, snapshot, verify_fixed_supply

    chain = _chain(Workdir(args.workdir)) if args.workdir else {"rpc": args.rpc, "program_id": args.program_id, "mint": args.mint}
    rpc = sc.Rpc(args.rpc or chain["rpc"])
    program_id, mint = sc.b58decode(chain["program_id"]), sc.b58decode(chain["mint"])
    snap = snapshot(rpc, program_id, mint)
    passed = verify_fixed_supply(snap, at_genesis=args.at_genesis)
    report = {key: snap[key] for key in ("mint", "current_supply", "mint_authority", "freeze_authority",
                                         "founder_balance_now", "reward_reserve_remaining", "burned_total")}
    report["checks_passed"] = passed
    if args.death_test_key:
        signers = {f"key:{Path(p).name}": sc.Keypair.load(Path(p)) for p in args.death_test_key}
        signers["random_attacker"] = sc.Keypair.generate()
        if args.airdrop_attacker:
            rpc.airdrop(signers["random_attacker"].public, 50_000_000)
        lite = sc.LiteProgram(program_id, mint)
        destination = sc.b58decode(snap["founder_token_account"])
        report["additional_mint_attempts"] = mint_death_test(
            rpc, mint, destination, signers,
            {"vault_authority_pda": lite.vault_authority, "config_pda": lite.config},
            unsigned={"founder (unsigned simulation)": sc.b58decode(snap["founder"])})
    if args.simulate_burn:
        from ares_lite.supply import simulate_bounty_burn

        report["simulated_bounty_burn"] = simulate_bounty_burn(rpc, program_id, mint, sc.b58decode(snap["founder"]),
                                                               10_000 * 10**6, 2**63 + 1)
    _print(report)


def cmd_supply(args) -> None:
    from ares_lite.supply import snapshot

    chain = _chain(Workdir(args.workdir))
    snap = snapshot(sc.Rpc(args.rpc or chain["rpc"]), sc.b58decode(chain["program_id"]), sc.b58decode(chain["mint"]))
    snap["genesis_signature"] = chain.get("genesis_signature")
    if getattr(args, "html", None):
        sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "web"))
        from render import render_supply

        Path(args.html).write_text(render_supply(snap))
    _print(snap)


def cmd_epoch_budget(args) -> None:
    """Budget an epoch may commit right now: unlocked(now) - committed."""

    from ares_lite.supply import snapshot

    chain = _chain(Workdir(args.workdir))
    snap = snapshot(sc.Rpc(chain["rpc"]), sc.b58decode(chain["program_id"]), sc.b58decode(chain["mint"]))
    _print({key: snap[key] for key in ("unlocked_now", "committed_to_seasons", "available_to_commit_now",
                                       "epoch_reward_cap_now", "last_settlement_utc", "claim_delay_seconds",
                                       "current_era", "current_release_per_day", "next_halving_utc")})


def cmd_bounty_fund(args) -> None:
    """Fund a bounty from your own tokens: 10% is burned (SPL Burn), 90% escrowed."""

    from ares_lite import tokenomics as tk

    chain = _chain(Workdir(args.workdir))
    rpc, funder = sc.Rpc(chain["rpc"]), sc.Keypair.load(Path(args.key))
    mint = sc.b58decode(chain["mint"])
    ata = sc.associated_token_address(funder.public, mint)
    supply_before = sc.parse_mint(rpc.account(mint)["raw"])["supply"]
    signature = rpc.send(funder, [_lite(chain).fund_bounty(funder.public, ata, args.bounty_id, args.amount)])
    supply_after = sc.parse_mint(rpc.account(mint)["raw"])["supply"]
    _print({"signature": signature, "amount": args.amount, "burned": tk.bounty_burn(args.amount),
            "escrowed": args.amount - tk.bounty_burn(args.amount), "supply_before": supply_before,
            "supply_after": supply_after, "supply_delta": supply_before - supply_after})


def cmd_bounty_award(args) -> None:
    chain = _chain(Workdir(args.workdir))
    rpc, admin = sc.Rpc(chain["rpc"]), _key(args.admin_key, "admin")
    mint = sc.b58decode(chain["mint"])
    winner = sc.b58decode(args.winner)
    rpc.send(admin, [sc.create_ata_idempotent_ix(admin.public, winner, mint)])
    destination = sc.associated_token_address(winner, mint)
    signature = rpc.send(admin, [_lite(chain).award_bounty(admin.public, args.bounty_id, destination)])
    _print({"signature": signature, "winner_token_account": sc.b58encode(destination),
            "winner_balance": sc.token_balance(rpc, destination)})


def main(argv=None) -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)
    p = sub.add_parser("chain-genesis", help="create mint + atomic fixed-supply genesis (devnet/local)")
    p.add_argument("--workdir", required=True)
    p.add_argument("--rpc", required=True)
    p.add_argument("--program-id", required=True)
    p.add_argument("--founder-wallet", required=True, help="FOUNDER_WALLET (base58); receives exactly 100M ARES")
    p.add_argument("--admin-key")
    p.add_argument("--airdrop", action="store_true")
    p.add_argument("--claim-delay", type=int, default=300, help="public verification window (s) between publish and claims")
    p.set_defaults(func=cmd_chain_genesis)
    p = sub.add_parser("verify-fixed-supply", help="release gate: fixed-supply invariants + mint death test")
    p.add_argument("--workdir")
    p.add_argument("--rpc")
    p.add_argument("--program-id")
    p.add_argument("--mint")
    p.add_argument("--at-genesis", action="store_true")
    p.add_argument("--death-test-key", action="append", help="keypair files to attempt (simulated) minting with")
    p.add_argument("--airdrop-attacker", action="store_true")
    p.add_argument("--simulate-burn", action="store_true", help="simulate a 10,000 ARES founder bounty (commits nothing)")
    p.set_defaults(func=cmd_verify_fixed_supply)
    for name, func in (("supply", cmd_supply), ("epoch-budget", cmd_epoch_budget)):
        p = sub.add_parser(name)
        p.add_argument("--workdir", required=True)
        p.add_argument("--rpc")
        if name == "supply":
            p.add_argument("--html", help="also write the ARES Supply page")
        p.set_defaults(func=func)
    p = sub.add_parser("bounty-fund", help="fund a bounty from your tokens: 10%% burned (SPL Burn), 90%% escrowed")
    p.add_argument("--workdir", required=True)
    p.add_argument("--key", required=True)
    p.add_argument("--bounty-id", type=int, required=True)
    p.add_argument("--amount", type=int, required=True, help="base units (6 decimals)")
    p.set_defaults(func=cmd_bounty_fund)
    p = sub.add_parser("bounty-award")
    p.add_argument("--workdir", required=True)
    p.add_argument("--bounty-id", type=int, required=True)
    p.add_argument("--winner", required=True)
    p.add_argument("--admin-key")
    p.set_defaults(func=cmd_bounty_award)
    args = parser.parse_args(argv)
    args.func(args)


if __name__ == "__main__":
    main()
