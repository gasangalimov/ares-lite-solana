"""Season Zero product metrics (NON-consensus, never used for rewards).

Main question of a points-only Season Zero: do people come back and iterate?
Per reward address we take its submissions in commit order and report the
first / second / third submission score and the best improvement over the
season baseline. Pool and AI-agent fields are optional self-declarations
stored next to (never inside) the commitments; nothing here limits compute,
agents or budget.
"""

from __future__ import annotations

from collections import Counter


def _bps(baseline: int, score: int) -> int:
    return (baseline - score) * 10_000 // baseline if baseline > 0 else 0


def season_metrics(commits: list[tuple[int, str]], results: dict, metadata: list[dict]) -> dict:
    """`commits`: [(commit_seq, address_hex)] for every commit (revealed or not);
    `results`: the published results.json; `metadata`: submission_metadata rows."""

    baseline = results["baseline"]["fuel"]
    scored = {row["commit_seq"]: row for row in results.get("score_table", [])}
    per_address: dict[str, list[int]] = {}
    for seq, address in sorted(commits):
        per_address.setdefault(address, []).append(seq)
    participants = []
    for address, seqs in sorted(per_address.items()):
        attempts = []
        for seq in seqs:
            row = scored.get(seq)
            state = "unrevealed" if row is None else ("valid" if row["valid"] else "invalid")
            score = row["score"] if row and row["valid"] else None
            attempts.append({"commit_seq": seq, "state": state, "score": score,
                             "improvement_bps": _bps(baseline, score) if score else None})
        valid = [a["improvement_bps"] for a in attempts if a["improvement_bps"] is not None]
        participants.append({
            "address": address,
            "submissions": len(seqs),
            "first": attempts[0], "second": attempts[1] if len(attempts) > 1 else None,
            "third": attempts[2] if len(attempts) > 2 else None,
            "best_improvement_bps": max(valid) if valid else None,
            "improved_on_own_first": len(valid) > 1 and max(valid[1:]) > valid[0],
        })
    meta_by_seq = {m["commit_seq"]: m for m in metadata}
    pools = Counter(m["pool"] for m in metadata if m.get("pool"))
    agents = Counter(agent for m in metadata for agent in m.get("ai_agents", ()))
    total = len(participants)
    returning = sum(1 for p in participants if p["submissions"] >= 2)
    best = [p["best_improvement_bps"] for p in participants if p["best_improvement_bps"] is not None]
    return {
        "status": "points-only Season Zero metrics; no token, no airdrop, no reward promise",
        "participants": total,
        "submissions": len(commits),
        "return_iteration_rate": round(returning / total, 4) if total else 0.0,
        "participants_with_2plus_submissions": returning,
        "participants_with_3plus_submissions": sum(1 for p in participants if p["submissions"] >= 3),
        "participants_improving_on_own_first": sum(1 for p in participants if p["improved_on_own_first"]),
        "best_improvement_bps": max(best) if best else None,
        "baseline_fuel": baseline,
        "pool_participation": {"submissions_declaring_a_pool": sum(1 for s, _ in commits if meta_by_seq.get(s, {}).get("pool")),
                               "pools": dict(sorted(pools.items()))},
        "ai_agents_declared": dict(sorted(agents.items())),
        "evaluation_cost": results.get("evaluation_cost"),
        "per_participant": participants,
    }
