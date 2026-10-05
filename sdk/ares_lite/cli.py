"""ares-lite: participant command line (DEVNET / TEST-ONLY; Season Zero POINTS-ONLY).

  ares-lite join --server URL [--dir D]        fetch the season, recompute the challenge, check it on Solana devnet
  ares-lite solve [--solver builtin:starter]   run one solver iteration -> my.wasm (+ local practice score)
  ares-lite practice [--module my.wasm]        local score vs the baseline on public practice cases
  ares-lite submit --address PUBKEY            commit (hash only) now, reveal later (`reveal`) — or --reveal-now
  ares-lite reveal                             reveal every pending commitment (reveal phase)
  ares-lite status                             phase, leaderboard, your submissions, receipt check
  ares-lite verify                             recompute the published results yourself (+ on-chain checks)
  ares-lite conformance                        reproduce the SDK conformance vectors
  ares-miner                                   the Miner window (Install -> Connect wallet -> Start mining)

Your wallet: pass your PUBLIC address (or a Solana CLI keypair file, of which
only the public half is read). No private key is needed to compete.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path

from . import solana_client as sc
from .client import DEVNET_RPC, ClientError, Season, address_from
from .solver_contract import load_solver, run_solver


def _season(args) -> Season:
    season = Season(Path(args.dir))
    if not season.state_file.exists():
        raise SystemExit(f"no season in {args.dir}: run `ares-lite join --server URL` first")
    return season


def cmd_join(args) -> None:
    season = Season.join(args.server, Path(args.dir), None if args.no_onchain_check else args.rpc)
    manifest = season.workdir().manifest()
    print(f"joined {manifest.name} (epoch {manifest.season_id}); {season.check_challenge()}")
    if not args.no_onchain_check:
        print(f"manifest hash matches the on-chain epoch account {season.state()['onchain']['epoch_account']}")
    print("next: ares-lite solve   (or open ARES Miner)")


def cmd_solve(args) -> None:
    season = _season(args)
    spec = load_solver(args.solver)
    res = run_solver(spec, season.path, season.path / "iterations" / "cli", args.iteration, os.urandom(32))
    if res.module is None:
        raise SystemExit(f"solver produced no module: {res.status}: {res.notes}\n{res.log_tail[-2000:]}")
    Path(args.out).write_bytes(res.module)
    p = season.practice(res.module, args.cases)
    print(f"{spec.name}@{spec.version}: wrote {args.out} ({len(res.module)} bytes); "
          f"{'valid' if p.valid else 'INVALID: ' + p.reason}; practice fuel {p.fuel:,} vs baseline {p.baseline_fuel:,} "
          f"({p.score_bps / 100:.2f}% better)")


def cmd_practice(args) -> None:
    season = _season(args)
    p = season.practice(Path(args.module).read_bytes(), args.cases)
    print(json.dumps({"valid": p.valid, "reason": p.reason, "practice_fuel": p.fuel, "baseline_fuel": p.baseline_fuel,
                      "score_bps": p.score_bps, "note": "public practice cases only; final scores use hidden cases"}, indent=2))


def cmd_submit(args) -> None:
    season = _season(args)
    module = Path(args.module).read_bytes()
    practice = season.practice(module, args.cases)
    if not practice.valid and not args.force:
        raise SystemExit(f"module is INVALID locally ({practice.reason}); fix it first (or --force)")
    sub = season.commit(module, address_from(args.address), args.pool or "", args.agent or [], args.solver_label or "",
                        practice)
    print(f"committed: commit #{sub['commit_seq']} for {sub['address']} (practice {practice.score_bps / 100:.2f}%); "
          f"salt kept in {season.state_file}")
    if args.reveal_now:
        sub = season.reveal(sub["id"])
        print(f"revealed: reveal #{sub['reveal_seq']}")
    else:
        print("reveal during the reveal phase with `ares-lite reveal` (ARES Miner does this automatically)")


def cmd_reveal(args) -> None:
    season = _season(args)
    n = 0
    for sub in season.state()["submissions"]:
        if sub.get("commit_seq") is not None and not sub.get("revealed"):
            sub = season.reveal(sub["id"])
            print(f"revealed commit #{sub['commit_seq']} (reveal #{sub['reveal_seq']})")
            n += 1
    print(f"{n} revealed")


def cmd_status(args) -> None:
    season = _season(args)
    status = season.status()
    print(json.dumps(status, indent=2))
    mine = {s["address"] for s in season.state()["submissions"]}
    for row in season.leaderboard()[: args.top]:
        mark = "  <- you" if sc.b58encode(bytes.fromhex(row["address"])) in mine else ""
        rank = f"{row['rank']:>4}." if row.get("rank") is not None else "   -."
        score = f"{row['best_score_bps'] / 100:>7.2f}%" if row.get("best_score_bps") is not None else "     --"
        copies = [h["duplicate_of"] for h in row.get("history", []) if h.get("duplicate_of") is not None]
        note = f"  (copy of commit #{copies[0]}: no credit)" if copies and row.get("rank") is None else ""
        print(f"{rank} {row['address_b58'][:10]}…  {score}  submissions {row['submissions']:>3}{mark}{note}")
    problems = season.check_receipts()
    print("receipts: all present in the published log" if not problems else "RECEIPT PROBLEMS:\n" + "\n".join(problems))


def cmd_verify(args) -> None:
    season = _season(args)
    out = season.verify(None if args.no_onchain_check else args.rpc)
    print(json.dumps(out, indent=2))
    if not out["all_ok"]:
        raise SystemExit(1)


def cmd_conformance(args) -> None:
    from . import conformance

    sys.argv = ["conformance"]
    conformance.main()


def main(argv=None) -> None:
    ap = argparse.ArgumentParser(prog="ares-lite", description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--dir", default=str(Path.home() / "ares-lite-season"))
    ap.add_argument("--rpc", default=DEVNET_RPC)
    sub = ap.add_subparsers(dest="command", required=True)
    p = sub.add_parser("join")
    p.add_argument("--server", required=True)
    p.add_argument("--no-onchain-check", action="store_true")
    p.set_defaults(func=cmd_join)
    p = sub.add_parser("solve")
    p.add_argument("--solver", default="builtin:starter")
    p.add_argument("--iteration", type=int, default=0)
    p.add_argument("--out", default="my.wasm")
    p.add_argument("--cases", type=int, default=16)
    p.set_defaults(func=cmd_solve)
    p = sub.add_parser("practice")
    p.add_argument("--module", default="my.wasm")
    p.add_argument("--cases", type=int, default=16)
    p.set_defaults(func=cmd_practice)
    p = sub.add_parser("submit")
    p.add_argument("--module", default="my.wasm")
    p.add_argument("--address", required=True, help="PUBLIC address or Solana CLI keypair file (public half only)")
    p.add_argument("--pool")
    p.add_argument("--agent", action="append", help="optional AI agent/tool used (metadata, never scored)")
    p.add_argument("--solver-label")
    p.add_argument("--reveal-now", action="store_true")
    p.add_argument("--force", action="store_true")
    p.add_argument("--cases", type=int, default=16)
    p.set_defaults(func=cmd_submit)
    sub.add_parser("reveal").set_defaults(func=cmd_reveal)
    p = sub.add_parser("status")
    p.add_argument("--top", type=int, default=20)
    p.set_defaults(func=cmd_status)
    p = sub.add_parser("verify")
    p.add_argument("--no-onchain-check", action="store_true")
    p.set_defaults(func=cmd_verify)
    sub.add_parser("conformance").set_defaults(func=cmd_conformance)
    args = ap.parse_args(argv)
    try:
        args.func(args)
    except ClientError as exc:
        raise SystemExit(f"error: {exc}")


if __name__ == "__main__":
    main()
