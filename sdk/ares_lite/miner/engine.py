"""ARES Miner engine (headless). The GUI and `ares-miner --headless` drive it.

Loop while running:
  phase OPEN           run solver iterations -> score locally -> commit every
                       improvement over the best already-committed score
  phase CLOSE_COMMITS  reveal every pending commitment (automatic)
  phase CLOSE_REVEALS  stop solving; once results are published, verify them
Commit-before-reveal is kept: by default nothing is revealed while commits are
still open, so nobody can copy your module and commit it ahead of you.

Safety: only the PUBLIC reward address is used. Salts stay in the local
season directory until reveal. Stop/resume is safe at any point (state is on
disk before every network call).
"""

from __future__ import annotations

import hashlib
import os
import threading
import time
import traceback
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass, field
from pathlib import Path

from .. import solana_client as sc
from ..client import ClientError, Season
from ..solver_contract import SolverError, load_solver, run_solver

PHASES = {0: "OPEN", 1: "OPEN", 2: "OPEN", 3: "CLOSE_COMMITS", 4: "CLOSE_REVEALS"}


@dataclass
class MinerConfig:
    season_dir: Path
    address: str = ""                       # base58 PUBLIC key only
    solver: str = "builtin:starter"
    workers: int = 1                        # concurrent solver iterations
    poll_seconds: int = 10
    reveal_early: bool = False              # reveal right after each commit (exposes your module early)
    rpc: str | None = None                  # verify against Solana devnet when set
    pool: str = ""
    agents: list[str] = field(default_factory=list)
    max_iterations: int | None = None


class Miner:
    def __init__(self, config: MinerConfig) -> None:
        self.config = config
        self.season = Season(config.season_dir)
        self._stop = threading.Event()
        self._thread: threading.Thread | None = None
        self._lock = threading.Lock()
        self.events: list[dict] = []
        self.snapshot: dict = {"running": False, "phase": "?", "iteration": 0, "best": None, "baseline_fuel": None,
                               "rank": None, "verification": None, "last_solver": None, "error": None}

    # ------------------------------------------------------------- control
    def start(self) -> None:
        if self._thread and self._thread.is_alive():
            return
        self.address_bytes()  # validate before starting
        self._stop.clear()
        self._thread = threading.Thread(target=self._run, name="ares-miner", daemon=True)
        self._thread.start()

    def stop(self) -> None:
        self._stop.set()

    def join(self, timeout: float | None = None) -> None:
        if self._thread:
            self._thread.join(timeout)

    @property
    def running(self) -> bool:
        return bool(self._thread and self._thread.is_alive())

    def address_bytes(self) -> bytes:
        if not self.config.address:
            raise ClientError("connect a wallet (public address) first")
        key = sc.b58decode(self.config.address)
        if len(key) != 32:
            raise ClientError("invalid wallet address")
        return key

    def log(self, kind: str, message: str, **data) -> None:
        with self._lock:
            self.events.append({"t": int(time.time()), "kind": kind, "message": message, **data})
            del self.events[:-500]

    def state(self) -> dict:
        with self._lock:
            snap = dict(self.snapshot)
            snap["running"] = self.running
            snap["events"] = list(self.events[-200:])
        try:
            st = self.season.state()
            snap["submissions"] = [{k: s.get(k) for k in ("id", "commit_seq", "revealed", "practice_score_bps", "practice_fuel",
                                                         "module_sha256", "solver", "created_unix")} for s in st.get("submissions", [])]
            snap["server"] = st.get("server")
        except (OSError, ValueError):
            snap["submissions"] = []
        snap["address"] = self.config.address
        snap["solver"] = self.config.solver
        return snap

    def _set(self, **kv) -> None:
        with self._lock:
            self.snapshot.update(kv)

    # ---------------------------------------------------------------- loop
    def _run(self) -> None:
        self.log("info", "mining started")
        try:
            self._loop()
        except Exception as exc:  # surfaced to the UI; the loop can be restarted
            self._set(error=f"{type(exc).__name__}: {exc}")
            self.log("error", f"miner stopped: {type(exc).__name__}: {exc}", trace=traceback.format_exc()[-2000:])
        finally:
            self.log("info", "mining stopped")

    def _loop(self) -> None:
        cfg = self.config
        address = self.address_bytes()
        try:
            spec = load_solver(cfg.solver)
        except SolverError as exc:
            raise ClientError(str(exc)) from exc
        work = self.season.workdir()
        manifest = work.manifest()
        self._set(season=manifest.name, epoch=manifest.season_id, challenge_id=work.challenge().challenge_id.hex(), error=None)
        if self.snapshot.get("baseline_fuel") is None:
            base = self.season.practice((self.season.path / "baseline.wasm").read_bytes())
            self._set(baseline_fuel=base.baseline_fuel)
            self.log("info", f"baseline practice fuel {base.baseline_fuel:,}")
        best_committed = max((s.get("practice_score_bps") or -10**9 for s in self.season.state()["submissions"]
                              if s.get("commit_seq") is not None), default=None)
        committed_hashes = {s["module_sha256"] for s in self.season.state()["submissions"]}
        iteration = self.snapshot.get("iteration", 0)
        last_poll = 0.0
        phase = "?"
        while not self._stop.is_set():
            if time.monotonic() - last_poll >= cfg.poll_seconds or phase == "?":
                status = self.season.status()
                phase = PHASES.get(status.get("phase_code", 0), "OPEN")
                self._set(phase=phase, entries=status.get("entries"), head=status.get("head"))
                self._update_rank(address)
                last_poll = time.monotonic()
            if phase == "OPEN":
                if cfg.max_iterations is not None and iteration >= cfg.max_iterations:
                    self._reveal_pending(only_if=cfg.reveal_early)
                    self._stop.wait(cfg.poll_seconds)
                    last_poll = 0.0
                    continue
                batch = list(range(iteration, iteration + max(1, cfg.workers)))
                iteration += len(batch)
                self._set(iteration=iteration)
                with ThreadPoolExecutor(max_workers=len(batch)) as pool:
                    results = list(pool.map(lambda i: self._one_iteration(spec, i), batch))
                for i, res, practice in results:
                    if practice is None or not practice.valid:
                        continue
                    digest = hashlib.sha256(res.module).hexdigest()
                    if best_committed is not None and practice.score_bps <= best_committed:
                        continue
                    if digest in committed_hashes:
                        continue
                    sub = self.season.commit(res.module, address, cfg.pool, cfg.agents, f"{spec.name}@{spec.version}", practice)
                    committed_hashes.add(sub["module_sha256"])
                    best_committed = practice.score_bps
                    self._set(best={"score_bps": practice.score_bps, "fuel": practice.fuel, "commit_seq": sub["commit_seq"]})
                    self.log("commit", f"committed improvement: {practice.score_bps / 100:.2f}% vs baseline "
                             f"(practice fuel {practice.fuel:,}), commit #{sub['commit_seq']}", commit_seq=sub["commit_seq"])
                    if cfg.reveal_early:
                        self._reveal_pending(only_if=True)
            elif phase == "CLOSE_COMMITS":
                self._reveal_pending(only_if=True)
                self._stop.wait(cfg.poll_seconds)
            else:  # CLOSE_REVEALS: wait for published results, then verify once
                if self.snapshot.get("verification") is None:
                    try:
                        result = self.season.verify(cfg.rpc)
                        self._set(verification=result)
                        self.log("verify", "results verified independently" if result["all_ok"] else "VERIFICATION FAILED",
                                 checks=result["checks"])
                    except ClientError as exc:
                        self.log("info", f"waiting for published results ({exc})")
                self._stop.wait(cfg.poll_seconds)

    def _one_iteration(self, spec, i: int):
        seed = hashlib.sha256(b"ares-miner/iteration" + i.to_bytes(8, "big") + os.urandom(16)).digest()
        out = self.season.path / "iterations" / f"w{i % max(1, self.config.workers)}"
        res = run_solver(spec, self.season.path, out, i, seed)
        practice = None
        if res.module is not None:
            practice = self.season.practice(res.module, baseline_fuel=self.snapshot.get("baseline_fuel"))
        self._set(last_solver={"iteration": i, "status": res.status, "notes": res.notes, "seconds": res.seconds,
                               "valid": practice.valid if practice else None,
                               "score_bps": practice.score_bps if practice else None,
                               "fuel": practice.fuel if practice else None})
        if practice is None:
            self.log("solver", f"iteration {i}: no solution ({res.status}: {res.notes})", log_tail=res.log_tail[-600:])
        else:
            self.log("solver", f"iteration {i}: {'valid' if practice.valid else 'INVALID ' + practice.reason}, "
                     f"practice fuel {practice.fuel:,} ({practice.score_bps / 100:.2f}% vs baseline)")
            best = self.snapshot.get("best_seen")
            if practice.valid and (best is None or practice.score_bps > best):
                self._set(best_seen=practice.score_bps)
        return i, res, practice

    def _reveal_pending(self, only_if: bool) -> None:
        if not only_if:
            return
        for sub in self.season.state()["submissions"]:
            if sub.get("commit_seq") is not None and not sub.get("revealed"):
                try:
                    self.season.reveal(sub["id"])
                    self.log("reveal", f"revealed commit #{sub['commit_seq']}", commit_seq=sub["commit_seq"])
                except ClientError as exc:
                    self.log("error", f"reveal of commit #{sub['commit_seq']} failed: {exc}")

    def _update_rank(self, address: bytes) -> None:
        try:
            board = self.season.leaderboard()
        except ClientError:
            return
        me = address.hex()
        for row in board:
            if row.get("address") == me:
                self._set(rank=row.get("rank"), board_row=row)
                return
        self._set(rank=None)
