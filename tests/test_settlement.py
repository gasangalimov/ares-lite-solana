"""Settlement-layer tests (fixed supply, halving, burn, epoch cap, client layouts, metrics).

Extracted verbatim from the ARES Lite test suite; competition-pipeline tests
(deterministic finalize, tie-break, manifest) need the private verifier core
and are not part of this repository. On-chain behaviour is covered by the
LiteSVM suite in solana/tests-svm.
"""

from __future__ import annotations

import itertools
import os
import random
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from ares_lite import solana_client as sc  # noqa: E402
from ares_lite import tokenomics as tk  # noqa: E402
from ares_lite.metrics import season_metrics  # noqa: E402
from ares_lite.supply import ReleaseCheckFailed, verify_fixed_supply  # noqa: E402

PROGRAM_SRC = (Path(__file__).resolve().parents[1] / "solana" / "program" / "src" / "lib.rs").read_text()
G = 1_800_000_000
DAY = 86_400


class FixedSupplyTests(unittest.TestCase):
    def test_genesis_constants(self):
        self.assertEqual(tk.TOTAL_SUPPLY, 1_000_000_000 * 10**6)
        self.assertEqual(tk.FOUNDER_ALLOCATION, 100_000_000 * 10**6)
        self.assertEqual(tk.REWARD_RESERVE, 900_000_000 * 10**6)
        self.assertEqual(tk.FOUNDER_ALLOCATION + tk.REWARD_RESERVE, tk.TOTAL_SUPPLY)
        self.assertEqual(tk.FOUNDER_ALLOCATION * 10, tk.TOTAL_SUPPLY)  # exactly 10%

    def test_python_constants_match_the_program_source(self):
        for name, value in (("TOTAL_SUPPLY", "1_000_000_000 * UNIT"), ("FOUNDER_ALLOCATION", "100_000_000 * UNIT"),
                            ("REWARD_RESERVE", "900_000_000 * UNIT"), ("ERA0_BUDGET", "450_000_000 * UNIT"),
                            ("ERA_SECONDS: i64", "157_788_000"), ("BOUNTY_BURN_BPS: u64", "1_000"), ("DECIMALS: u8", "6")):
            self.assertIn(f"pub const {name}", PROGRAM_SRC)
            self.assertIn(value, PROGRAM_SRC.split(f"pub const {name}")[1].split(";")[0])
        # The program has no mint instruction after genesis: MintTo (tag 7) is
        # only built inside `initialize`.
        body = PROGRAM_SRC.split("fn initialize(")[1].split("\nfn ")[0]
        self.assertEqual(PROGRAM_SRC.count("amount_ix(7,"), body.count("amount_ix(7,"))

    def snap(self, **overrides):
        base = {"mint_authority": None, "freeze_authority": None, "decimals": 6, "current_supply": tk.TOTAL_SUPPLY,
                "burned_total": 0, "cumulative_challenge_burns": 0, "reward_reserve_remaining": tk.REWARD_RESERVE,
                "distributed_to_miners": 0, "committed_to_seasons": 0, "unlocked_now": 0,
                "founder_balance_now": tk.FOUNDER_ALLOCATION}
        base.update(overrides)
        return base

    def test_release_gate_accepts_genesis_and_rejects_violations(self):
        self.assertIn("mint_authority_is_None", verify_fixed_supply(self.snap(), at_genesis=True))
        for bad in ({"mint_authority": "SomeKey"}, {"freeze_authority": "SomeKey"}, {"current_supply": tk.TOTAL_SUPPLY + 1},
                    {"decimals": 9}, {"reward_reserve_remaining": tk.REWARD_RESERVE - 1},
                    {"committed_to_seasons": 5, "unlocked_now": 4, "distributed_to_miners": 0}):
            with self.assertRaises(ReleaseCheckFailed):
                verify_fixed_supply(self.snap(**bad))
        with self.assertRaises(ReleaseCheckFailed):
            verify_fixed_supply(self.snap(founder_balance_now=1), at_genesis=True)


class HalvingTests(unittest.TestCase):
    E = tk.ERA_SECONDS

    def test_exact_values_and_boundaries(self):
        self.assertEqual(self.E, 5 * 36525 * 864)  # exactly 5 Julian years
        self.assertEqual(tk.unlocked(0), 0)
        self.assertEqual(tk.unlocked(-1), 0)
        self.assertEqual(tk.unlocked(self.E // 2), tk.ERA0_BUDGET // 2)
        self.assertEqual(tk.unlocked(self.E), tk.ERA0_BUDGET)
        self.assertLess(tk.unlocked(self.E - 1), tk.ERA0_BUDGET)
        self.assertEqual(tk.unlocked(2 * self.E), tk.ERA0_BUDGET * 3 // 2)
        self.assertEqual(tk.era_at(self.E - 1), (0, tk.ERA0_BUDGET))
        self.assertEqual(tk.era_at(self.E), (1, tk.ERA0_BUDGET // 2))
        self.assertEqual(tk.era_at(2 * self.E), (2, tk.ERA0_BUDGET // 4))

    def test_rates_halve(self):
        d = self.E // 10
        r0 = tk.unlocked(d)
        r1 = tk.unlocked(self.E + d) - tk.unlocked(self.E)
        r2 = tk.unlocked(2 * self.E + d) - tk.unlocked(2 * self.E)
        self.assertEqual(r1, r0 // 2)
        self.assertEqual(r2, r0 // 4)
        self.assertEqual(tk.release_rate_per_day(0), tk.ERA0_BUDGET * 86_400 // self.E)

    def test_long_horizon_rounding_and_dust(self):
        self.assertEqual(tk.DUST, 16)
        self.assertEqual(tk.unlocked(10**15), tk.REWARD_RESERVE - tk.DUST)
        previous = 0
        for t in range(0, 70 * self.E, self.E // 3 + 7):
            value = tk.unlocked(t)
            self.assertGreaterEqual(value, previous)
            self.assertLessEqual(value, tk.REWARD_RESERVE)
            previous = value

    def test_next_halving_is_exactly_five_years(self):
        genesis = 1_800_000_000
        self.assertEqual(tk.next_halving(genesis, genesis), genesis + self.E)
        self.assertEqual(tk.next_halving(genesis, genesis + self.E), genesis + 2 * self.E)


class BurnTests(unittest.TestCase):
    def test_burn_formula_matches_program(self):
        self.assertEqual(tk.bounty_burn(10_000 * tk.UNIT), 1_000 * tk.UNIT)
        self.assertEqual(tk.bounty_burn(9), 0)
        self.assertEqual(tk.bounty_burn(10), 1)
        self.assertEqual(tk.bounty_burn(2**64 - 1), (2**64 - 1) * 1000 // 10_000)
        for amount in (1, 9, 10, 11, 99, 12_345_678, 10**15):
            burn = tk.bounty_burn(amount)
            self.assertEqual(burn + (amount - burn), amount)
            self.assertLessEqual(burn * 10, amount)

    def test_burn_table_has_no_price_language(self):
        rows = tk.burn_table()
        self.assertEqual(len(rows), 20)
        text = repr(rows).lower()
        for banned in ("price", "yield", "return", "apprec", "profit"):
            self.assertNotIn(banned, text)
        ten = [r for r in rows if r["burn_rate"] == "10%" and r["annual_bounty_volume_ares"] == 100_000_000][0]
        self.assertEqual(ten["burned_per_year_ares"], 10_000_000)


class ClientTests(unittest.TestCase):
    def test_parse_config_layout(self):
        raw = bytearray(197)  # program v3 layout
        raw[0], raw[1] = 11, 3
        raw[130:138] = (1_800_000_000).to_bytes(8, "little")
        raw[146:154] = (123).to_bytes(8, "little")
        raw[181:189] = (1_800_000_500).to_bytes(8, "little")
        raw[189:197] = (3600).to_bytes(8, "little")
        config = sc.parse_config(bytes(raw))
        self.assertEqual(config["genesis_ts"], 1_800_000_000)
        self.assertEqual(config["distributed"], 123)
        self.assertEqual(config["last_settlement_ts"], 1_800_000_500)
        self.assertEqual(config["claim_delay"], 3600)
        with self.assertRaises(ValueError):
            sc.parse_config(bytes(181))  # the v2 layout is rejected

    def test_mainnet_requires_owner_go(self):
        os.environ.pop("ARES_LITE_MAINNET_GO", None)
        with self.assertRaises(ValueError):
            sc.Rpc("https://api.mainnet-beta.solana.com")
        os.environ["ARES_LITE_MAINNET_GO"] = "yes please"
        with self.assertRaises(ValueError):
            sc.Rpc("https://api.mainnet-beta.solana.com")
        os.environ.pop("ARES_LITE_MAINNET_GO", None)

    def test_pdas_differ_by_purpose(self):
        lite = sc.LiteProgram(b"\x01" * 32, b"\x02" * 32)
        self.assertEqual(len({lite.config, lite.vault, lite.vault_authority, lite.season(0), lite.bounty(0), lite.escrow(0)}), 6)


class EpochVersusEraTests(unittest.TestCase):
    def test_constants_match_program(self):
        self.assertEqual(tk.MAX_EPOCH_SECONDS, 7 * DAY)
        self.assertIn("pub const MAX_EPOCH_SECONDS: i64 = 7 * 86_400;", PROGRAM_SRC)
        self.assertIn("pub const CONFIG_LEN: usize = 1 + 1 + 32 * 4 + 8 * 6 + 3 + 16;", PROGRAM_SRC)  # = 197
        self.assertEqual(1 + 1 + 32 * 4 + 8 * 6 + 3 + 16, 197)
        self.assertIn("pub const SEASON_LEN: usize = 187;", PROGRAM_SRC)

    def test_one_winner_can_never_take_the_era_budget(self):
        worst = max(tk.epoch_reward_cap(G, G, G + t) for t in range(0, 30 * DAY, 3600))
        week = tk.unlocked(7 * DAY)
        self.assertLessEqual(abs(worst - week), 1)  # one MAX_EPOCH of release (+-1 base unit of floor rounding)
        self.assertLess(worst * 200, tk.ERA0_BUDGET)  # < 0.5% of the 450M era budget
        self.assertLess(worst, 2_000_000 * tk.UNIT)

    def test_cap_is_exactly_what_unlocked_since_last_settlement(self):
        self.assertEqual(tk.epoch_reward_cap(G, G + 5 * DAY, G + 5 * DAY), 0)
        self.assertEqual(tk.epoch_reward_cap(G, G + DAY, G + 3 * DAY), tk.unlocked(3 * DAY) - tk.unlocked(DAY))
        # a long pause does not accumulate: lookback is bounded by MAX_EPOCH
        self.assertEqual(tk.epoch_reward_cap(G, G, G + 100 * DAY), tk.unlocked(100 * DAY) - tk.unlocked(93 * DAY))

    def test_settlements_telescope_and_never_exceed_the_schedule(self):
        rng = random.Random(10)
        for _ in range(50):
            last, now, paid = G, G, 0
            for _ in range(rng.randrange(1, 60)):
                now += rng.randrange(0, 14 * DAY)
                cap = tk.epoch_reward_cap(G, last, now)
                paid += rng.choice((0, cap, cap // 2))
                self.assertLessEqual(paid, tk.unlocked(now - G))
                last = now
        # contiguous settlements (each <= MAX_EPOCH apart) sum to unlocked exactly
        last, total = G, 0
        for now in range(G + DAY, G + 400 * DAY, DAY):
            total += tk.epoch_reward_cap(G, last, now)
            last = now
        self.assertEqual(total, tk.unlocked(last - G))

    def test_epoch_cap_halves_with_the_era(self):
        era = tk.ERA_SECONDS
        before = tk.epoch_reward_cap(G, G + era - 8 * DAY, G + era - DAY)
        after = tk.epoch_reward_cap(G, G + era + DAY, G + era + 8 * DAY)
        self.assertLessEqual(abs(before - 2 * after), 2)
        self.assertEqual(tk.epoch_reward_cap(G, G + 64 * era, G + 64 * era + DAY), 0)


class ClientV3Tests(unittest.TestCase):
    def test_parse_season_layout(self):
        raw = bytearray(187)
        raw[0], raw[1] = 12, 2
        raw[2:10] = (5).to_bytes(8, "little")
        raw[90:122] = b"\xcd" * 32
        raw[179:187] = (42).to_bytes(8, "little")
        season = sc.parse_season(bytes(raw))
        self.assertEqual((season["status"], season["season_id"], season["close_cap"]), ("CLOSED", 5, 42))
        self.assertEqual(season["log_head"], "cd" * 32)
        with self.assertRaises(ValueError):
            sc.parse_season(bytes(131))

    def test_instruction_encodings(self):
        lite = sc.LiteProgram(b"\x01" * 32, b"\x02" * 32)
        admin = b"\x03" * 32
        self.assertEqual(lite.initialize(admin, b"\x04" * 32, 300).data, b"\x00" + (300).to_bytes(8, "little"))
        self.assertEqual(lite.close_season(admin, 1, b"\xcd" * 32, 9).data, b"\x07" + b"\xcd" * 32 + (9).to_bytes(8, "little"))
        publish = lite.publish(admin, 1, b"\xaa" * 32, 5, b"\xcd" * 32, b"\xee" * 32).data
        self.assertEqual(publish, b"\x02" + b"\xaa" * 32 + (5).to_bytes(8, "little") + b"\xcd" * 32 + b"\xee" * 32)
        self.assertEqual(lite.cancel_root(admin, 1).data, b"\x08")


class MetricsTests(unittest.TestCase):
    def test_return_iterations_and_metadata(self):
        a, b, c = "aa" * 32, "bb" * 32, "cc" * 32
        commits = [(1, a), (2, b), (3, a), (4, a), (5, c)]
        results = {"baseline": {"fuel": 1000}, "evaluation_cost": {"wall_seconds": 1.5},
                   "score_table": [{"commit_seq": 1, "valid": True, "score": 990},
                                   {"commit_seq": 2, "valid": False, "score": 0},
                                   {"commit_seq": 3, "valid": True, "score": 900},
                                   {"commit_seq": 4, "valid": True, "score": 800}]}
        metadata = [{"commit_seq": 1, "pool": "p1", "ai_agents": ["agent-x"]},
                    {"commit_seq": 3, "pool": "p1", "ai_agents": ["agent-x", "agent-y"]}]
        out = season_metrics(commits, results, metadata)
        self.assertEqual(out["participants"], 3)
        self.assertEqual(out["participants_with_2plus_submissions"], 1)
        self.assertEqual(out["participants_with_3plus_submissions"], 1)
        self.assertEqual(out["participants_improving_on_own_first"], 1)
        self.assertEqual(out["best_improvement_bps"], 2000)
        self.assertEqual(out["pool_participation"]["submissions_declaring_a_pool"], 2)
        self.assertEqual(out["ai_agents_declared"], {"agent-x": 2, "agent-y": 1})
        alice = [p for p in out["per_participant"] if p["address"] == a][0]
        self.assertEqual([alice[k]["commit_seq"] for k in ("first", "second", "third")], [1, 3, 4])
        cohort = out["return_iteration_rate"]  # headline metric: cohort = first VALID submission
        self.assertEqual(cohort["cohort_first_valid_submission"], 1)  # b only invalid, c only unrevealed
        self.assertEqual((cohort["made_2nd"]["participants"], cohort["made_3rd"]["participants"],
                          cohort["made_5th"]["participants"]), (1, 1, 0))
        late = season_metrics([(1, b), (2, b), (3, b), (4, b), (5, b), (6, b)],
                              {"baseline": {"fuel": 1000}, "score_table": [
                                  {"commit_seq": 1, "valid": False, "score": 0}, {"commit_seq": 2, "valid": True, "score": 900}]}, [])
        # counting starts at the first valid submission: seqs 2..6 = 5 submissions
        self.assertEqual(late["return_iteration_rate"]["made_5th"]["participants"], 1)
        unrevealed = [p for p in out["per_participant"] if p["address"] == c][0]
        self.assertEqual(unrevealed["first"]["state"], "unrevealed")
        self.assertNotIn("price", repr(out).lower())


class KeyGuardTests(unittest.TestCase):
    def test_keys_are_never_written_inside_the_repository(self):
        repo = Path(__file__).resolve().parents[1]
        with self.assertRaises(ValueError):
            sc.Keypair.generate().save(repo / "should-not-exist.json")
        self.assertFalse((repo / "should-not-exist.json").exists())


if __name__ == "__main__":
    unittest.main()
