"""ARES Lite conformance vectors (format `ares-lite-conformance/1`).

`compute(inputs_dir)` derives every protocol-relevant value from public inputs
only (a season manifest, its challenge beacon, solver modules) using the
public API of the `ares_lite` package. The golden file
`vectors/conformance_v1.json` was produced by the operator's implementation;
an independent implementation conforms when it reproduces the same JSON.

All values are integers, hex strings or ASCII strings. No floats; no locale.
"""

from __future__ import annotations

import hashlib
import json
from dataclasses import replace
from pathlib import Path

FORMAT = "ares-lite-conformance/1"
ADDRESS_A = bytes(range(1, 33))
ADDRESS_B = bytes(range(33, 65))
SALT = bytes.fromhex("5a" * 32)
PRACTICE_CASES = 4


def _h(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _rejects(fn) -> str:
    try:
        fn()
    except ValueError as exc:  # SubmissionRejected, codec and parse errors are ValueErrors
        return f"rejected: {type(exc).__name__}: {exc}"
    return "ACCEPTED"


def encoding_vectors() -> dict:
    from ares_lite.evaluation import decode
    from ares_lite.submission import encode

    values = {
        "uint_0": 0, "uint_23": 23, "uint_24": 24, "uint_255": 255, "uint_256": 256, "uint_65536": 65536,
        "uint_2^32": 2**32, "uint_2^64-1": 2**64 - 1, "negint_-1": -1, "bytes_empty": b"", "bytes_3": b"\x00\x01\xff",
        "text_ascii": "CODE-SYNTH", "list_empty": [], "list_nested": [0, [1, b"\x02"], "x"],
    }
    good = {name: encode(v).hex() for name, v in values.items()}
    for name, hexed in good.items():  # round trip
        assert encode(decode(bytes.fromhex(hexed))) == bytes.fromhex(hexed), name
    non_canonical = {
        "uint_23_in_1_byte": "1817", "uint_0_in_2_bytes": "190000", "bytes_len_in_1_byte": "5803010203",
        "trailing_byte": "0000", "truncated_bytes": "4301", "float_half": "f93c00", "map": "a0",
        "indefinite_bytes": "5fff",
    }
    bad = {name: _rejects(lambda h=h: decode(bytes.fromhex(h))) for name, h in non_canonical.items()}
    return {"canonical": good, "must_reject": bad}


def hashing_vectors() -> dict:
    from ares_lite import domains
    from ares_lite.submission import hash_bytes

    return {"mining_key_ref(ADDRESS_A)": hash_bytes(domains.MINING_KEY_REF, (ADDRESS_A,)).hex()}


def compute(inputs: Path) -> dict:
    from ares_lite import merkle
    from ares_lite.evaluation import benchmark_cases, evaluate
    from ares_lite.pipeline import baseline_reveal
    from ares_lite.policies import ScoreRow, score_table_digest
    from ares_lite.season import Beacon, SeasonManifest, family_config_ref, practice_seed, render_solver_constants, season_challenge
    from ares_lite.submission import (
        ReceiptLog,
        canonical_solution,
        commitment_for,
        reveal_bytes_for,
    )
    from ares_lite.tooling import canonicalize_module
    from ares_lite import solana_client as sc

    inputs = Path(inputs)
    manifest = SeasonManifest.load(inputs / "manifest.json")
    beacon = Beacon(**json.loads((inputs / "challenge_beacon.json").read_text()))
    challenge = season_challenge(manifest, beacon)
    modules = {p.stem: p.read_bytes() for p in sorted((inputs / "modules").glob("*.wasm"))}

    season = {
        "manifest_hash": manifest.manifest_hash().hex(),
        "season_id": manifest.season_id,
        "challenge_id": challenge.challenge_id.hex(),
        "ticket_id": challenge.context.ticket_id.hex(),
        "family_config_ref": family_config_ref(manifest).hex(),
        "season_rs_sha256": _h(render_solver_constants(challenge).encode()),
        "practice_seed_0": practice_seed(challenge, 0).hex(),
        "practice_seed_1": practice_seed(challenge, 1).hex(),
    }

    def log_open() -> ReceiptLog:
        log = ReceiptLog(manifest, challenge)
        log.open(beacon.bytes(), hashlib.sha256(modules["baseline"]).digest())
        return log

    per_module = {}
    cases = benchmark_cases(challenge, practice_seed(challenge, 1), PRACTICE_CASES)
    for name, raw in modules.items():
        module = canonicalize_module(raw)
        solution = canonical_solution(challenge, module)
        commitment = commitment_for(manifest, challenge, ADDRESS_A, solution, SALT)
        reveal = reveal_bytes_for(manifest, challenge, solution, SALT)
        rev, record = baseline_reveal(manifest, challenge, module)
        ev = evaluate(challenge, rev, record, [practice_seed(challenge, 0)], cases)
        log = log_open()
        seq = log.commit(ADDRESS_A, commitment).seq
        accepted = log.reveal(seq, reveal)
        per_module[name] = {
            "module_sha256": _h(module),
            "solution_sha256": _h(solution),
            "commitment(ADDRESS_A,SALT)": commitment.hex(),
            "commitment(ADDRESS_B,SALT)": commitment_for(manifest, challenge, ADDRESS_B, solution, SALT).hex(),
            "reveal_sha256": _h(reveal),
            "reveal_len": len(reveal),
            "log_head_after_commit_and_reveal": accepted.head.hex(),
            "practice": {"cases": PRACTICE_CASES, "valid": ev.valid, "reason": ev.reason,
                         "benchmark_fuel": ev.benchmark_fuel, "evaluation_digest": ev.digest().hex()},
        }

    # ---- negative vectors (all must be rejected by the operator log / transcripts)
    base = canonicalize_module(modules["baseline"])
    solution = canonical_solution(challenge, base)
    commitment = commitment_for(manifest, challenge, ADDRESS_A, solution, SALT)
    reveal = reveal_bytes_for(manifest, challenge, solution, SALT)
    flipped = bytearray(solution)
    flipped[-1] ^= 1
    other_season = replace(manifest, season_id=manifest.season_id + 1)
    other_beacon = Beacon(beacon.kind, beacon.slot, ("00" * 32) if beacon.value != "00" * 32 else ("11" * 32))
    negatives = {}

    def case(name, fn):
        negatives[name] = _rejects(fn)

    def with_commit(addr=ADDRESS_A, com=commitment):
        log = log_open()
        return log, log.commit(addr, com).seq

    # The operator records the address sent with the commit; a commitment computed for another address never opens.
    case("reveal_with_changed_reward_address",
         lambda: (lambda lg, s: lg.reveal(s, reveal))(
             *with_commit(ADDRESS_A, commitment_for(manifest, challenge, ADDRESS_B, solution, SALT))))
    case("reveal_with_changed_solution_byte",
         lambda: (lambda lg, s: lg.reveal(s, reveal_bytes_for(manifest, challenge, bytes(flipped), SALT)))(*with_commit()))
    case("reveal_with_changed_salt",
         lambda: (lambda lg, s: lg.reveal(s, reveal_bytes_for(manifest, challenge, solution, b"\x00" * 32)))(*with_commit()))
    case("reveal_replayed_from_another_epoch",
         lambda: (lambda lg, s: lg.reveal(s, reveal_bytes_for(other_season, challenge, solution, SALT)))(*with_commit()))
    case("reveal_for_another_challenge",
         lambda: (lambda lg, s: lg.reveal(s, reveal_bytes_for(manifest, season_challenge(manifest, other_beacon), solution, SALT)))(
             *with_commit()))
    case("malformed_reveal_truncated", lambda: (lambda lg, s: lg.reveal(s, reveal[:-1]))(*with_commit()))
    case("non_canonical_reveal_padding", lambda: (lambda lg, s: lg.reveal(s, reveal + b"\x00"))(*with_commit()))
    case("wrong_commit_seq", lambda: (lambda lg, s: lg.reveal(s + 7, reveal))(*with_commit()))

    def double_reveal():
        lg, s = with_commit()
        lg.reveal(s, reveal)
        lg.reveal(s, reveal)
    case("double_reveal", double_reveal)

    def late_reveal():
        lg, s = with_commit()
        lg.close_commits()
        lg.close_reveals(1)
        lg.reveal(s, reveal)
    case("late_reveal_after_close", late_reveal)

    def late_commit():
        lg = log_open()
        lg.close_commits()
        lg.commit(ADDRESS_A, commitment)
    case("late_commit_after_close", late_commit)

    def duplicate_commit():
        lg, _ = with_commit()
        lg.commit(ADDRESS_B, commitment)
    case("duplicate_commitment", duplicate_commit)
    case("short_reward_address", lambda: log_open().commit(ADDRESS_A[:31], commitment))
    case("short_commitment", lambda: log_open().commit(ADDRESS_A, commitment[:31]))
    negatives["changed_address_changes_commitment"] = commitment != commitment_for(manifest, challenge, ADDRESS_B, solution, SALT)
    negatives["changed_epoch_changes_commitment"] = commitment != commitment_for(other_season, challenge, ADDRESS_A, solution, SALT)

    leaves = [merkle.leaf_hash(manifest.season_id, ADDRESS_A, 100), merkle.leaf_hash(manifest.season_id, ADDRESS_B, 7),
              merkle.leaf_hash(manifest.season_id + 1, ADDRESS_A, 100)]
    root, proofs = merkle.build(leaves)
    rows = [ScoreRow(1, ADDRESS_A, b"\x01" * 32, True, 900_000), ScoreRow(2, ADDRESS_B, b"\x02" * 32, False, 0)]
    program = sc.b58decode("9Tzp3MQFQR9d2VRfreJJgtq3cdYEdaDujRMHVMhpxBoV")
    mint = sc.b58decode("BcdSq76FgstSMAyJgYJ4wx6LvBebw5NCReKRwQ6UzfTV")
    lite = sc.LiteProgram(program, mint)
    return {
        "format": FORMAT,
        "inputs": {"ADDRESS_A": ADDRESS_A.hex(), "ADDRESS_B": ADDRESS_B.hex(), "SALT": SALT.hex(),
                   "manifest_sha256": _h((inputs / "manifest.json").read_bytes()),
                   "beacon_sha256": _h((inputs / "challenge_beacon.json").read_bytes()),
                   "modules_sha256": {k: _h(v) for k, v in modules.items()}},
        "encoding": encoding_vectors(),
        "hashing": hashing_vectors(),
        "season": season,
        "modules": per_module,
        "negative": negatives,
        "merkle": {"leaves": [x.hex() for x in leaves], "root": root.hex(), "proof_0": [p.hex() for p in proofs[0]]},
        "score_table_digest": score_table_digest(rows, 1_000_000).hex(),
        "devnet_v1_addresses": {"config": sc.b58encode(lite.config), "mining_vault": sc.b58encode(lite.vault),
                                "founder_vault": sc.b58encode(lite.founder_vault), "epoch_7": sc.b58encode(lite.epoch(7))},
    }


def main() -> None:
    import argparse
    import sys

    ap = argparse.ArgumentParser(description="compute or check the ARES Lite conformance vectors")
    ap.add_argument("--inputs", default=str(Path(__file__).resolve().parent / "vectors"))
    ap.add_argument("--write")
    args = ap.parse_args()
    got = compute(Path(args.inputs))
    text = json.dumps(got, indent=1, sort_keys=True) + "\n"
    if args.write:
        Path(args.write).write_text(text)
        return
    golden = (Path(args.inputs) / "conformance_v1.json").read_text()
    if golden != text:
        print("NON-CONFORMANT: output differs from vectors/conformance_v1.json", file=sys.stderr)
        sys.exit(1)
    print("CONFORMANT: reproduces vectors/conformance_v1.json exactly")


if __name__ == "__main__":
    main()
