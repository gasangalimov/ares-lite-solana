"""ARES Lite economics v0: fixed supply, distribution halving, bounty burn.

Mirrors the on-chain constants and `unlocked()` in solana/program
(cross-checked in tests). All amounts are base units (6 decimals).
"""

from __future__ import annotations

from datetime import datetime, timezone

DECIMALS = 6
UNIT = 10**DECIMALS
TOTAL_SUPPLY = 1_000_000_000 * UNIT
FOUNDER_ALLOCATION = 100_000_000 * UNIT
REWARD_RESERVE = 900_000_000 * UNIT
ERA0_BUDGET = 450_000_000 * UNIT
ERA_SECONDS = 157_788_000  # exactly 5 Julian years = 5 * 365.25 * 86400
BOUNTY_BURN_BPS = 1_000  # 10.00%
MAX_EPOCH_SECONDS = 7 * 86_400  # a mining epoch pays at most one week of release
DUST = bin(ERA0_BUDGET).count("1")  # base units that integer halving never releases

assert FOUNDER_ALLOCATION + REWARD_RESERVE == TOTAL_SUPPLY
assert FOUNDER_ALLOCATION * 10 == TOTAL_SUPPLY
assert 2 * ERA0_BUDGET == REWARD_RESERVE


def unlocked(elapsed: int) -> int:
    """Max base units the protocol may have released `elapsed` s after genesis."""

    if elapsed <= 0:
        return 0
    era, within = divmod(elapsed, ERA_SECONDS)
    total = sum(ERA0_BUDGET >> k for k in range(min(era, 64)))
    if era < 64:
        total += (ERA0_BUDGET >> era) * within // ERA_SECONDS
    return total


def epoch_reward_cap(genesis_ts: int, last_settlement_ts: int, now_ts: int) -> int:
    """Mirror of the on-chain cap: newly unlocked since the previous settlement,
    looking back at most MAX_EPOCH_SECONDS. Never the 5-year era budget."""

    start = max(last_settlement_ts, now_ts - MAX_EPOCH_SECONDS, genesis_ts)
    if now_ts <= start:
        return 0
    return unlocked(now_ts - genesis_ts) - unlocked(start - genesis_ts)


def era_at(elapsed: int) -> tuple[int, int]:
    era = max(0, elapsed) // ERA_SECONDS
    return era, (ERA0_BUDGET >> era) if era < 64 else 0


def release_rate_per_day(elapsed: int) -> int:
    _, budget = era_at(elapsed)
    return budget * 86_400 // ERA_SECONDS


def bounty_burn(amount: int, bps: int = BOUNTY_BURN_BPS) -> int:
    return amount * bps // 10_000


def next_halving(genesis_ts: int, now_ts: int) -> int:
    era, _ = era_at(now_ts - genesis_ts)
    return genesis_ts + (era + 1) * ERA_SECONDS


def iso(ts: int) -> str:
    return datetime.fromtimestamp(ts, tz=timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def era_table(eras: int = 8) -> list[dict]:
    rows = []
    for n in range(eras):
        budget = ERA0_BUDGET >> n
        rows.append({
            "era": n,
            "years": f"{5 * n}-{5 * (n + 1)}",
            "era_budget_ares": budget / UNIT,
            "cumulative_unlocked_ares": unlocked((n + 1) * ERA_SECONDS) / UNIT,
            "per_day_ares": budget * 86_400 // ERA_SECONDS / UNIT,
        })
    return rows


def burn_table(rates=(0, 200, 500, 1_000, 2_000), volumes=(1_000_000, 10_000_000, 100_000_000, 1_000_000_000)) -> list[dict]:
    """usage -> burned supply (no price modelling). Volumes = ARES of bounties
    funded per year (gross; the same tokens can circulate back into bounties).
    Reports the yearly burn and the years of constant volume needed to burn
    10% of genesis supply. No price, no yield, no appreciation is implied."""

    genesis = TOTAL_SUPPLY / UNIT
    rows = []
    for bps in rates:
        for volume in volumes:
            per_year = volume * bps / 10_000
            rows.append({
                "burn_rate": f"{bps / 100:g}%",
                "annual_bounty_volume_ares": volume,
                "burned_per_year_ares": per_year,
                "burned_per_year_pct_of_genesis": per_year / genesis * 100,
                "years_to_burn_10pct_of_genesis": (0.1 * genesis / per_year) if per_year else None,
            })
    return rows
