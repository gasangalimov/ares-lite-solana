"""Participant client: season files, wallet address, practice, commit, reveal, verify.

Used by the `ares-lite` CLI and by ARES Miner. Talks to an operator over plain
HTTP(S) and to Solana devnet RPC. Trust model:

- The operator is NOT trusted: the challenge is recomputed locally from the
  manifest + Solana beacon, the manifest hash is compared with the on-chain
  epoch account, receipts are checked against the published log, and final
  results are recomputed locally (`verify`).
- No private key is needed to mine: a commitment binds only your PUBLIC
  reward address. ARES Miner never reads, stores or sends a secret key.
- The salt of every commitment stays on this machine until you reveal.
"""

from __future__ import annotations

import hashlib
import json
import os
import time
import urllib.error
import urllib.request
from dataclasses import dataclass
from pathlib import Path

from . import solana_client as sc

DEVNET_RPC = "https://api.devnet.solana.com"
DEVNET_PROGRAM_ID = "9Tzp3MQFQR9d2VRfreJJgtq3cdYEdaDujRMHVMhpxBoV"
DEVNET_MINT = "BcdSq76FgstSMAyJgYJ4wx6LvBebw5NCReKRwQ6UzfTV"
SEASON_FILES = (("manifest.json", "/manifest"), ("challenge.json", "/challenge"), ("challenge_beacon.json", "/beacon"),
                ("season.rs", "/season.rs"), ("baseline.wasm", "/baseline.wasm"), ("AGENT_BRIEF.md", "/agent.md"))
MAX_DOWNLOAD = 8 << 20  # 8 MiB per response
PRACTICE_CASES = 16  # same as the operator's live practice board


class ClientError(RuntimeError):
    pass


# ------------------------------------------------------------------ wallet


def address_from(value: str) -> bytes:
    """A reward address: base58 Solana pubkey, or the PUBLIC half of a Solana
    CLI keypair file. Only bytes 32..64 of the file (the public key) are used;
    the secret half is never kept or sent anywhere."""

    path = Path(value).expanduser()
    if path.is_file():
        raw = json.loads(path.read_text(encoding="utf-8"))
        if not (isinstance(raw, list) and len(raw) == 64 and all(isinstance(b, int) and 0 <= b < 256 for b in raw)):
            raise ClientError("not a Solana CLI keypair file (64-byte JSON array)")
        public = bytes(raw[32:])
        del raw
        return public
    try:
        key = sc.b58decode(value.strip())
    except (ValueError, KeyError) as exc:
        raise ClientError("not a base58 Solana address") from exc
    if len(key) != 32:
        raise ClientError("a Solana address is 32 bytes")
    return key


# -------------------------------------------------------------------- http


def http_get(server: str, path: str, limit: int = MAX_DOWNLOAD) -> bytes:
    url = server.rstrip("/") + path
    if not url.startswith(("http://", "https://")):
        raise ClientError("server must be an http(s) URL")
    try:
        with urllib.request.urlopen(url, timeout=60) as response:
            data = response.read(limit + 1)
    except urllib.error.HTTPError as err:
        raise ClientError(f"GET {path}: HTTP {err.code}") from err
    except urllib.error.URLError as err:
        raise ClientError(f"GET {path}: {err.reason}") from err
    if len(data) > limit:
        raise ClientError(f"GET {path}: response larger than {limit} bytes")
    return data


def http_post(server: str, path: str, obj: dict) -> dict:
    url = server.rstrip("/") + path
    request = urllib.request.Request(url, json.dumps(obj).encode(), {"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(request, timeout=300) as response:
            return json.loads(response.read(1 << 20))
    except urllib.error.HTTPError as err:
        try:
            message = json.loads(err.read(1 << 16)).get("error")
        except ValueError:
            message = f"HTTP {err.code}"
        raise ClientError(f"operator rejected {path}: {message}") from err
    except urllib.error.URLError as err:
        raise ClientError(f"POST {path}: {err.reason}") from err


# ------------------------------------------------------------------ session


@dataclass
class Practice:
    valid: bool
    reason: str
    fuel: int
    baseline_fuel: int

    @property
    def score_bps(self) -> int:
        """Improvement over the baseline in basis points (integer; may be negative)."""

        if not self.valid or self.baseline_fuel <= 0:
            return 0
        return (self.baseline_fuel - self.fuel) * 10_000 // self.baseline_fuel


class Season:
    """A local season directory (public files + this participant's submissions)."""

    def __init__(self, path: Path) -> None:
        self.path = Path(path)
        self.state_file = self.path / "participant.json"

    # ---- state
    def state(self) -> dict:
        return json.loads(self.state_file.read_text(encoding="utf-8")) if self.state_file.exists() else {"submissions": []}

    def save(self, state: dict) -> None:
        tmp = self.state_file.with_suffix(".tmp")
        tmp.write_text(json.dumps(state, indent=2), encoding="utf-8")
        try:
            os.chmod(tmp, 0o600)  # holds unrevealed salts
        except OSError:
            pass
        os.replace(tmp, self.state_file)

    @property
    def server(self) -> str:
        return self.state()["server"]

    def workdir(self):
        from .tooling import Workdir

        return Workdir(self.path)

    # ---- join
    @classmethod
    def join(cls, server: str, path: Path, rpc: str | None = None, program_id: str = DEVNET_PROGRAM_ID) -> "Season":
        path = Path(path)
        path.mkdir(parents=True, exist_ok=True)
        for name, route in SEASON_FILES:
            (path / name).write_bytes(http_get(server, route))
        season = cls(path)
        state = season.state()
        state.update({"server": server.rstrip("/"), "joined_unix": int(time.time())})
        season.save(state)
        season.check_challenge()
        if rpc:
            state["onchain"] = season.check_manifest_on_chain(rpc, program_id)
            season.save(state)
        return season

    def check_challenge(self) -> str:
        """Recompute the challenge from manifest + beacon; never trust the operator's copy."""

        work = self.workdir()
        challenge = work.challenge()
        published = json.loads((self.path / "challenge.json").read_text(encoding="utf-8"))["challenge_id"]
        if challenge.challenge_id.hex() != published:
            raise ClientError("operator challenge does not match manifest + beacon: do not trust this operator")
        from .season import render_solver_constants

        if (self.path / "season.rs").read_text(encoding="utf-8") != render_solver_constants(challenge):
            raise ClientError("operator season.rs does not match the challenge")
        baseline_hash = hashlib.sha256((self.path / "baseline.wasm").read_bytes()).hexdigest()
        return f"challenge {published[:16]}… recomputed locally; baseline sha256 {baseline_hash[:16]}…"

    def check_manifest_on_chain(self, rpc: str, program_id: str = DEVNET_PROGRAM_ID, mint: str = DEVNET_MINT) -> dict:
        """The epoch account on Solana pins this season's manifest hash (create_epoch)."""

        try:
            return self._check_manifest_on_chain(rpc, program_id, mint)
        except ClientError:
            raise
        except Exception as exc:  # network/TLS/RPC failures: report, never crash
            raise ClientError(f"Solana RPC check failed ({type(exc).__name__}: {exc})") from exc

    def _check_manifest_on_chain(self, rpc: str, program_id: str, mint: str) -> dict:

        manifest = self.workdir().manifest()
        lite = sc.LiteProgram(sc.b58decode(program_id), sc.b58decode(mint))
        account = sc.Rpc(rpc).account(lite.epoch(manifest.season_id))
        if account is None:
            raise ClientError(f"no on-chain epoch account for season/epoch {manifest.season_id}")
        if account["owner"] != program_id:
            raise ClientError("epoch account is not owned by the ARES Lite program")
        epoch = sc.parse_epoch(account["raw"])
        if epoch["manifest_hash"] != manifest.manifest_hash().hex():
            raise ClientError("on-chain manifest hash differs from the operator's manifest")
        beacon = self.workdir().challenge_beacon()
        if beacon.kind == "solana-slot-blockhash":
            slot, value = sc.Rpc(rpc).beacon(manifest.challenge_beacon_slot)
            if (slot, value.hex()) != (beacon.slot, beacon.value):
                raise ClientError("challenge beacon differs from the Solana blockhash at the declared slot")
        return {"epoch_account": sc.b58encode(lite.epoch(manifest.season_id)), "status": epoch["status"],
                "manifest_hash": epoch["manifest_hash"], "challenge_beacon_checked": beacon.kind == "solana-slot-blockhash"}

    # ---- practice
    def practice(self, module: bytes, cases: int = PRACTICE_CASES, baseline_fuel: int | None = None) -> Practice:
        from .evaluation import benchmark_cases, evaluate
        from .pipeline import baseline_reveal
        from .season import practice_seed
        from .tooling import canonicalize_module

        work = self.workdir()
        manifest, challenge = work.manifest(), work.challenge()
        case_list = benchmark_cases(challenge, practice_seed(challenge, 1), cases)

        def run(raw: bytes):
            try:
                mod = canonicalize_module(raw)
            except Exception as exc:  # malformed module: invalid, never a crash
                return False, f"undecodable module: {type(exc).__name__}", 0
            reveal, record = baseline_reveal(manifest, challenge, mod)
            ev = evaluate(challenge, reveal, record, [practice_seed(challenge, 0)], case_list)
            return ev.valid, ev.reason, ev.benchmark_fuel

        if baseline_fuel is None:
            # The reference is the baseline's fuel on the benchmark cases only: its fuel is
            # heavy-tailed, so on some instances it exceeds the per-test cap on a practice
            # gate case. A candidate still has to pass the full gate below.
            baseline_fuel = self.baseline_benchmark_fuel(case_list)
        valid, reason, fuel = run(module)
        return Practice(valid, reason, fuel, baseline_fuel)

    def baseline_benchmark_fuel(self, case_list) -> int:
        from .evaluation import benchmark_fuel

        work = self.workdir()
        challenge = work.challenge()
        raw = (self.path / "baseline.wasm").read_bytes()
        if getattr(challenge, "is_scaled", False):
            from .scaled import run_cases

            res = run_cases(challenge, raw, case_list)
            ok, fuel, reason = res.valid, res.fuel, res.reason
        else:
            ok, fuel, reason = benchmark_fuel(challenge, raw, case_list)
        if not ok:
            raise ClientError(f"the published baseline fails the practice benchmark ({reason}); "
                              "the season files are inconsistent")
        return fuel

    # ---- commit / reveal
    def commit(self, module: bytes, address: bytes, pool: str = "", agents: list[str] | None = None,
               solver: str = "", practice: Practice | None = None) -> dict:
        from .submission import canonical_solution, commitment_for
        from .tooling import canonicalize_module

        work = self.workdir()
        manifest, challenge = work.manifest(), work.challenge()
        module = canonicalize_module(module)
        if len(module) > 131_072:
            raise ClientError("module larger than the profile limit")
        salt = os.urandom(32)
        solution = canonical_solution(challenge, module)
        commitment = commitment_for(manifest, challenge, address, solution, salt)
        state = self.state()
        sub = {"id": len(state["submissions"]), "address": sc.b58encode(address), "commitment": commitment.hex(),
               "salt": salt.hex(), "module_sha256": hashlib.sha256(module).hexdigest(), "solver": solver[:64],
               "practice_fuel": practice.fuel if practice else None, "practice_score_bps": practice.score_bps if practice else None,
               "created_unix": int(time.time()), "commit_seq": None, "revealed": False}
        (self.path / "modules").mkdir(exist_ok=True)
        (self.path / "modules" / f"{sub['module_sha256']}.wasm").write_bytes(module)
        state["submissions"].append(sub)
        self.save(state)  # salt is on disk BEFORE anything is sent
        body = {"address": sub["address"], "commitment": sub["commitment"]}
        if pool:
            body["pool"] = pool
        if agents:
            body["ai_agents"] = agents
        if solver:
            body["solver"] = solver
        reply = http_post(self.server, "/commit", body)
        state = self.state()
        state["submissions"][sub["id"]].update({"commit_seq": reply["seq"], "commit_head": reply["head"],
                                               "commit_receipt": reply.get("receipt")})
        self.save(state)
        return state["submissions"][sub["id"]]

    def reveal(self, sub_id: int) -> dict:
        from .submission import canonical_solution, reveal_bytes_for

        state = self.state()
        sub = state["submissions"][sub_id]
        if sub["commit_seq"] is None:
            raise ClientError("not committed yet")
        if sub["revealed"]:
            return sub
        work = self.workdir()
        manifest, challenge = work.manifest(), work.challenge()
        module = (self.path / "modules" / f"{sub['module_sha256']}.wasm").read_bytes()
        raw = reveal_bytes_for(manifest, challenge, canonical_solution(challenge, module), bytes.fromhex(sub["salt"]))
        reply = http_post(self.server, "/reveal", {"commit_seq": sub["commit_seq"], "reveal": raw.hex()})
        state = self.state()
        state["submissions"][sub_id].update({"revealed": True, "reveal_seq": reply["seq"], "reveal_head": reply["head"],
                                            "reveal_receipt": reply.get("receipt")})
        self.save(state)
        return state["submissions"][sub_id]

    # ---- status / verification
    def status(self) -> dict:
        return json.loads(http_get(self.server, "/status"))

    def leaderboard(self) -> list:
        return json.loads(http_get(self.server, "/leaderboard"))

    def check_receipts(self) -> list[str]:
        """Every signed receipt we hold must be in the published log (else: operator equivocation)."""

        manifest = self.workdir().manifest()
        entries = [json.loads(line) for line in http_get(self.server, "/log").decode().splitlines() if line.strip()]
        from .submission import verify_chain

        verify_chain(entries)
        problems = []
        if manifest.receipt_signer:
            from .transparency import SignedReceipt, TransparencyViolation, check_receipt_against_log

            for sub in self.state()["submissions"]:
                for key in ("commit_receipt", "reveal_receipt"):
                    if sub.get(key):
                        try:
                            check_receipt_against_log(bytes.fromhex(manifest.receipt_signer), SignedReceipt.from_json(sub[key]),
                                                      entries)
                        except TransparencyViolation as exc:
                            problems.append(f"submission {sub['id']} {key}: {exc}")
        return problems

    def verify(self, rpc: str | None = None, program_id: str = DEVNET_PROGRAM_ID, mint: str = DEVNET_MINT) -> dict:
        """Independent verification of the published result from public data only."""

        try:
            return self._verify(rpc, program_id, mint)
        except ClientError:
            raise
        except Exception as exc:
            raise ClientError(f"verification could not complete ({type(exc).__name__}: {exc})") from exc

    def _verify(self, rpc: str | None, program_id: str, mint: str) -> dict:

        from .pipeline import finalize
        from .season import Beacon
        from .submission import ReceiptLog

        work = self.workdir()
        manifest, challenge = work.manifest(), work.challenge()
        log_bytes = http_get(self.server, "/log")
        results = json.loads(http_get(self.server, "/results.json"))
        reward_beacon = Beacon(**json.loads(http_get(self.server, "/reward_beacon.json")))
        (self.path / "receipts.jsonl").write_bytes(log_bytes)
        refs = json.loads(http_get(self.server, "/references.json")) if manifest.reference_features else {}
        reference_modules = [http_get(self.server, f"/references/{name}.wasm") for name in manifest.reference_features]
        for name, module in zip(manifest.reference_features, reference_modules):
            if refs.get(name) and hashlib.sha256(module).hexdigest() != refs[name]:
                raise ClientError(f"reference module {name} does not match references.json")
        log = ReceiptLog.load(self.path / "receipts.jsonl", manifest, challenge)
        result = finalize(manifest, challenge, log, reward_beacon, (self.path / "baseline.wasm").read_bytes(),
                          reference_modules, results.get("epoch_reward_cap"))
        checks = {
            "merkle_root": result.merkle_root.hex() == results["merkle_root"],
            "score_table_digest": result.score_digest.hex() == results["score_table_digest"],
            "log_head": result.log_head.hex() == results["log_head"],
        }
        mine = {s["commit_seq"] for s in self.state()["submissions"] if s.get("commit_seq") is not None}
        my_rows = [{"commit_seq": r.commit_seq, "valid": r.valid, "score": r.score} for r in result.score_rows if r.commit_seq in mine]
        out = {"checks": checks, "my_results": my_rows, "baseline_fuel": result.baseline.benchmark_fuel}
        if rpc:
            lite = sc.LiteProgram(sc.b58decode(program_id), sc.b58decode(mint))
            account = sc.Rpc(rpc).account(lite.epoch(manifest.season_id))
            if account is None or account["owner"] != program_id:
                raise ClientError("epoch account missing or not owned by the program")
            ep = sc.parse_epoch(account["raw"])
            checks["onchain_manifest_hash"] = ep["manifest_hash"] == manifest.manifest_hash().hex()
            checks["onchain_log_head"] = ep["log_head"] == result.log_head.hex()
            if ep["status"] in ("PUBLISHED", "CORRECTED", "FINAL"):
                checks["onchain_results_digest"] = ep["results_digest"] == result.score_digest.hex()
                checks["onchain_root"] = ep["reward_root"] == result.merkle_root.hex()
                checks["onchain_total_le_cap"] = ep["reward_total"] <= ep["cap"]
            out["onchain_status"] = ep["status"]
            if reward_beacon.kind == "solana-slot-blockhash":
                close_slot = log.entries[-1].fields[0]
                slot, value = sc.Rpc(rpc).beacon(close_slot + manifest.reward_beacon_delay_slots)
                checks["reward_beacon_is_solana_blockhash"] = (slot, value.hex()) == (reward_beacon.slot, reward_beacon.value)
        out["all_ok"] = all(checks.values())
        return out
