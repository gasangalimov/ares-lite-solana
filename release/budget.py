"""Exact devnet SOL budget for the reviewed ARES Lite binary (read-only).

    python3 release/budget.py [--rpc https://api.devnet.solana.com] [--so <program.so>]

Rent comes live from the cluster (getMinimumBalanceForRentExemption); fees
are 5,000 lamports per signature (the rehearsal sets no priority fee). The
signature counts below are the transactions the scripts actually send. They
were counted from a full_run.sh ledger, where fees_total / 5,000 equals this
plan exactly. Transactions rejected at preflight (all adversarial checks)
are never landed and cost nothing.
"""

from __future__ import annotations

import argparse
import json
import math
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import ares_lite.solana_client as sc  # noqa: E402

FEE = sc.LAMPORTS_PER_SIGNATURE
# Upgradeable-loader sizes: programdata = 45-byte header + ELF; program = 36;
# buffer = 37-byte header + ELF (rent refunded to the payer inside Deploy).
PROGRAMDATA_HEADER, PROGRAM_LEN, BUFFER_HEADER = 45, 36, 37
WRITE_CHUNK = 1012  # bytes of ELF per loader Write transaction (solana CLI)


def deploy_signatures(elf_len: int) -> int:
    # create buffer (payer + buffer) = 2, writes = ceil(len / chunk) x 1, deploy (payer + program) = 2
    return 2 + math.ceil(elf_len / WRITE_CHUNK) + 2


# (description, signatures, accounts created as (name, size))
CRITICAL = [
    ("genesis: create mint + init mint + founder ATA (deployer + mint key)", 2, [("mint", 82), ("founder ATA", 165)]),
    ("genesis: initialize (config + vault, mint 1B, revoke)", 1, [("config", 197), ("reward vault", 165)]),
    ("create season", 1, [("season", 187)]),
    ("close season (pin log head + epoch cap)", 1, []),
    ("publish root", 1, []),
    ("fund winner (transfer from deployer)", 1, []),
    ("claim (winner ATA + receipt + vault transfer)", 1, [("winner ATA", 165), ("claim receipt", 50)]),
    ("fund founder (transfer from deployer)", 1, []),
    ("bounty fund (10% SPL burn, 90% escrow)", 1, [("bounty", 59), ("bounty escrow", 165)]),
    ("bounty award (ATA idempotent + award)", 2, []),
    ("sweep winner back to deployer (founder ends at exactly 0)", 1, []),
]
ADVERSARIAL_EXTRA = [
    ("fund attacker (transfer)", 1, []),
    ("attacker ATA (closed again at the end; rent refunded)", 1, []),
    ("scratch season for negatives: create + close + publish(0)", 3, [("scratch season", 187)]),
    ("close attacker ATA + sweep attacker", 2, []),
    ("all rejected attacks (16 checks), mint-death simulations, verify-*", 0, []),
]


def plan(rpc: sc.Rpc, elf_len: int) -> dict:
    rent = {}

    def r(size: int) -> int:
        if size not in rent:
            rent[size] = rpc.rent(size)
        return rent[size]

    deploy = {
        "programdata_rent": r(PROGRAMDATA_HEADER + elf_len),
        "program_account_rent": r(PROGRAM_LEN),
        "signatures": deploy_signatures(elf_len),
    }
    deploy["fees"] = deploy["signatures"] * FEE
    deploy["total"] = deploy["programdata_rent"] + deploy["program_account_rent"] + deploy["fees"]
    # Peak during deploy: the buffer is funded first; Deploy drains it back to the
    # payer before creating programdata, so the peak is never 2x the rent.
    deploy["peak_before_refund"] = r(BUFFER_HEADER + elf_len) + deploy["fees"]

    def section(rows):
        accounts = {name: r(size) for _, _, created in rows for name, size in created}
        sigs = sum(n for _, n, _ in rows)
        return {"accounts": accounts, "accounts_total": sum(accounts.values()), "signatures": sigs, "fees": sigs * FEE,
                "total": sum(accounts.values()) + sigs * FEE}

    critical, extra = section(CRITICAL), section(ADVERSARIAL_EXTRA)
    # Temporary wallets that sign after paying must keep their own rent-exempt
    # minimum (r(0)) until the final sweep: it raises the peak, not the cost.
    critical["temporary_float_returned"] = r(0)  # winner
    extra["temporary_float_returned"] = r(0) + r(165)  # attacker wallet minimum + its ATA (closed, refunded)
    return {
        "elf_bytes": elf_len,
        "rent_source": rpc.url,
        "deploy": deploy,
        "critical_flow_after_deploy": critical,
        "critical_net_cost": deploy["total"] + critical["total"],
        # the admin wallet itself must stay rent-exempt while non-empty: r(0) is kept, never spent
        "admin_wallet_minimum_kept": r(0),
        "critical_minimum_balance": deploy["total"] + critical["total"] + critical["temporary_float_returned"] + r(0),
        "adversarial_extra": extra,
        "full_22_step_net_cost": deploy["total"] + critical["total"] + extra["total"],
        "full_22_step_minimum_balance": deploy["total"] + critical["total"] + extra["total"]
        + critical["temporary_float_returned"] + extra["temporary_float_returned"] + r(0),
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--rpc", default="https://api.devnet.solana.com")
    parser.add_argument("--so", default=str(Path(__file__).resolve().parents[1] / "solana/program/target/deploy/ares_lite_rewards.so"))
    args = parser.parse_args()
    if "mainnet" in args.rpc:
        raise SystemExit("budget.py is for devnet/localnet only")
    print(json.dumps(plan(sc.Rpc(args.rpc), Path(args.so).stat().st_size), indent=2))


if __name__ == "__main__":
    main()
