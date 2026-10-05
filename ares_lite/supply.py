"""Supply dashboard and fixed-supply release checks (from on-chain state only)."""

from __future__ import annotations

import time

from . import solana_client as sc
from . import tokenomics as tk


class ReleaseCheckFailed(RuntimeError):
    """A fixed-supply invariant does not hold; release must stop."""


def snapshot(rpc: "sc.Rpc", program_id: bytes, mint: bytes, founder_token_account: bytes | None = None,
             now_ts: int | None = None) -> dict:
    lite = sc.LiteProgram(program_id, mint)
    mint_info = rpc.account(mint)
    config_info = rpc.account(lite.config)
    vault_info = rpc.account(lite.vault)
    if not (mint_info and config_info and vault_info):
        raise ReleaseCheckFailed("mint, config or vault account missing")
    mint_state = sc.parse_mint(mint_info["raw"])
    config = sc.parse_config(config_info["raw"])
    vault_balance = int.from_bytes(vault_info["raw"][64:72], "little")
    founder_ata = founder_token_account or sc.associated_token_address(sc.b58decode(config["founder"]), mint)
    founder_balance = sc.token_balance(rpc, founder_ata)
    now_ts = now_ts if now_ts is not None else int(time.time())
    elapsed = now_ts - config["genesis_ts"]
    era, era_budget = tk.era_at(elapsed)
    supply = mint_state["supply"]
    return {
        "mint": sc.b58encode(mint),
        "program_id": sc.b58encode(program_id),
        "decimals": mint_state["decimals"],
        "mint_authority": mint_state["mint_authority"],
        "freeze_authority": mint_state["freeze_authority"],
        "genesis_supply": tk.TOTAL_SUPPLY,
        "current_supply": supply,
        "burned_total": tk.TOTAL_SUPPLY - supply,
        "cumulative_challenge_burns": config["bounty_burned"],
        "creator_allocation_at_genesis": tk.FOUNDER_ALLOCATION,
        "founder": config["founder"],
        "founder_token_account": sc.b58encode(founder_ata),
        "founder_balance_now": founder_balance,
        "reward_reserve_at_genesis": tk.REWARD_RESERVE,
        "reward_vault": sc.b58encode(lite.vault),
        "reward_reserve_remaining": vault_balance,
        "distributed_to_miners": config["distributed"],
        "committed_to_seasons": config["committed"],
        "unlocked_now": tk.unlocked(elapsed),
        "available_to_commit_now": tk.unlocked(elapsed) - config["committed"],
        "epoch_reward_cap_now": tk.epoch_reward_cap(config["genesis_ts"], config["last_settlement_ts"], now_ts),
        "last_settlement_utc": tk.iso(config["last_settlement_ts"]),
        "claim_delay_seconds": config["claim_delay"],
        "creator_allocation_pct_of_genesis": tk.FOUNDER_ALLOCATION * 100 / tk.TOTAL_SUPPLY,
        "genesis_ts": config["genesis_ts"],
        "genesis_utc": tk.iso(config["genesis_ts"]),
        "current_era": era,
        "current_era_budget": era_budget,
        "current_release_per_day": tk.release_rate_per_day(elapsed),
        "next_halving_ts": tk.next_halving(config["genesis_ts"], now_ts),
        "next_halving_utc": tk.iso(tk.next_halving(config["genesis_ts"], now_ts)),
        "bounty_escrowed": config["bounty_escrowed"],
        "admin": config["admin"],
        "program_version": config["version"],
    }


def verify_fixed_supply(snap: dict, at_genesis: bool = False) -> list[str]:
    """Invariant checks; returns the list of passed checks or raises."""

    checks = {
        "mint_authority_is_None": snap["mint_authority"] is None,
        "freeze_authority_is_None": snap["freeze_authority"] is None,
        "decimals_is_6": snap["decimals"] == tk.DECIMALS,
        "current_supply_le_genesis": snap["current_supply"] <= tk.TOTAL_SUPPLY,
        "supply_equals_genesis_minus_burned": snap["current_supply"] == tk.TOTAL_SUPPLY - snap["burned_total"],
        "challenge_burns_le_total_burned": snap["cumulative_challenge_burns"] <= snap["burned_total"],
        "vault_equals_reserve_minus_distributed": snap["reward_reserve_remaining"] == tk.REWARD_RESERVE - snap["distributed_to_miners"],
        "distributed_le_committed_le_unlocked": snap["distributed_to_miners"] <= snap["committed_to_seasons"] <= snap["unlocked_now"] + 0,
        "founder_allocation_is_10pct": tk.FOUNDER_ALLOCATION * 10 == tk.TOTAL_SUPPLY,
    }
    if at_genesis:
        checks["supply_exactly_1B"] = snap["current_supply"] == tk.TOTAL_SUPPLY
        checks["founder_balance_exactly_100M"] = snap["founder_balance_now"] == tk.FOUNDER_ALLOCATION
        checks["vault_exactly_900M"] = snap["reward_reserve_remaining"] == tk.REWARD_RESERVE
    failed = [name for name, ok in checks.items() if not ok]
    if failed:
        raise ReleaseCheckFailed(f"fixed-supply invariants failed: {failed}")
    return list(checks)


def mint_death_test(rpc: "sc.Rpc", mint: bytes, destination: bytes, signers: dict[str, "sc.Keypair"],
                    pda_authorities: dict[str, bytes], unsigned: dict[str, bytes] | None = None,
                    fee_payer: "sc.Keypair | None" = None) -> dict:
    """Simulate MintTo by every candidate authority; every one must FAIL.

    Simulation commits nothing, so this is safe on mainnet. A funded
    `fee_payer` (default: the first signer) pays, and the candidate signs as
    the mint authority, so an unfunded candidate cannot "fail" for the wrong
    reason. Only a rejection by the token program itself (InstructionError)
    counts; any other error (missing payer, bad blockhash) aborts the test.
    PDAs are tried as (non-signing) authorities; a PDA can only sign inside
    its program, which has no mint instruction.
    """

    payer = fee_payer or next(iter(signers.values()))

    def verdict(name: str, result: dict) -> str:
        err = result.get("err")
        if not err:
            return "SUCCEEDED (BAD)"
        if not (isinstance(err, dict) and "InstructionError" in err):
            raise ReleaseCheckFailed(f"mint death test for {name} failed for an unrelated reason: {err}")
        return "FAILED (good)"

    outcome = {}
    for name, key in signers.items():
        extra = [] if key.public == payer.public else [key]
        outcome[name] = verdict(name, rpc.simulate(payer, [sc.mint_to_ix(mint, destination, key.public, 1)], extra))
    for name, address in pda_authorities.items():
        ix = sc.Ix(sc.TOKEN_PROGRAM, (sc.Meta(mint, False, True), sc.Meta(destination, False, True),
                                      sc.Meta(address, False, False)), bytes([7]) + (1).to_bytes(8, "little"))
        outcome[name] = verdict(name, rpc.simulate(payer, [ix]))
    for name, address in (unsigned or {}).items():
        outcome[name] = verdict(name, rpc.simulate_unsigned(payer.public, [sc.mint_to_ix(mint, destination, address, 1)]))
    if any("BAD" in v for v in outcome.values()):
        raise ReleaseCheckFailed(f"additional mint possible: {outcome}")
    return outcome


def simulate_bounty_burn(rpc: "sc.Rpc", program_id: bytes, mint: bytes, funder: bytes, amount: int, bounty_id: int) -> dict:
    """Simulate a bounty funded by `funder` (no key needed, nothing committed)
    and check the simulated mint supply drops by exactly the burn."""

    lite = sc.LiteProgram(program_id, mint)
    before = sc.parse_mint(rpc.account(mint)["raw"])["supply"]
    ata = sc.associated_token_address(funder, mint)
    result = rpc.simulate_unsigned(funder, [lite.fund_bounty(funder, ata, bounty_id, amount)], watch=[mint])
    if result.get("err"):
        raise ReleaseCheckFailed(f"bounty simulation failed: {result['err']} {result.get('logs', [])[-3:]}")
    import base64

    after = sc.parse_mint(base64.b64decode(result["accounts"][0]["data"][0]))["supply"]
    burn = tk.bounty_burn(amount)
    if before - after != burn:
        raise ReleaseCheckFailed(f"simulated burn {before - after} != expected {burn}")
    return {"amount": amount, "expected_burn": burn, "supply_before": before, "simulated_supply_after": after}
