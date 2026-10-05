"""End-to-end smoke test of the INSTALLED ARES Miner (any OS, no devnet, no keys of yours).

    python -m ares_lite.smoke [--solver builtin:starter] [--out smoke_report.json]

Runs a throw-away local practice season (reference operator on 127.0.0.1), then
drives the real `ares-miner` program through its local window API exactly like
the window does: connect a PUBLIC address -> join -> START MINING -> wait for a
committed improvement -> close commits -> Miner reveals -> close reveals ->
finalize -> Miner verifies -> leaderboard. Also runs `ares-lite verify/status`.
Exit code 0 = PASS. Needs Rust + wasm32-unknown-unknown for builtin:starter.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import re
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
import urllib.request
from http.server import ThreadingHTTPServer
from pathlib import Path

SCALED = "24,32,130,32,8,8,5,30000000,50000000"


def _free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def _exe(name: str) -> list[str]:
    """The installed console script (ares-miner.exe on Windows), else the module."""

    found = shutil.which(name, path=os.pathsep.join([str(Path(sys.executable).parent), os.environ.get("PATH", "")]))
    if found:
        return [found]
    return [sys.executable, "-m", {"ares-miner": "ares_lite.miner.app", "ares-lite": "ares_lite.cli"}[name]]


class Step:
    def __init__(self, report: dict) -> None:
        self.report = report

    def __call__(self, name: str, ok: bool, detail="") -> None:
        self.report["steps"].append({"step": name, "ok": bool(ok), "detail": detail if isinstance(detail, (dict, list)) else str(detail)[:2000]})
        print(f"[{'PASS' if ok else 'FAIL'}] {name}" + (f": {detail}" if detail and not ok else ""), flush=True)
        if not ok:
            raise SystemExit(1)


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--solver", default="builtin:starter")
    ap.add_argument("--out", default="smoke_report.json")
    ap.add_argument("--timeout", type=int, default=1500)
    args = ap.parse_args(argv)

    from . import solana_client as sc
    from .operator_server import make_handler
    from .tooling import Workdir

    report = {"platform": platform.platform(), "python": sys.version.split()[0], "solver": args.solver, "steps": []}
    step = Step(report)
    tmp = Path(tempfile.mkdtemp(prefix="ares-smoke-"))
    home = tmp / "home"
    home.mkdir()
    env = {**os.environ, "ARES_LITE_KEYS": str(tmp / "keys"), "ARES_LITE_HOME": str(home / "config")}
    work = tmp / "operator"
    miner_proc = None
    server = None
    deadline = time.time() + args.timeout
    try:
        def operator(*a):
            out = subprocess.run([sys.executable, "-m", "ares_lite.operator_cli", *a], env=env, capture_output=True, text=True)
            if out.returncode != 0:
                step(f"operator {a[0]}", False, out.stderr[-1500:])
            return out.stdout

        key = tmp / "keys" / "receipts.json"
        sc.Keypair.generate().save(key)
        beacon = lambda tag: hashlib.sha256(tag.encode()).hexdigest()  # noqa: E731
        operator("new-season", "--out", str(work / "manifest.json"), "--name", "smoke", "--network", "localnet",
                 "--chain-id", "ares-lite-localnet", "--beacon-slot", "100", "--delay", "150", "--cases", "8", "--seeds", "1",
                 "--cap", "100000000", "--threshold-bps", "100", "--profile", "lite-L1", "--scaled", SCALED,
                 "--policy", "season_zero_points_v0", "--receipt-key", str(key))
        operator("open", "--workdir", str(work), "--manifest", str(work / "manifest.json"), "--test-beacon", beacon("challenge"))
        server = ThreadingHTTPServer(("127.0.0.1", 0), make_handler(Workdir(work), sc.Keypair.load(key)))
        threading.Thread(target=server.serve_forever, daemon=True).start()
        op_url = f"http://127.0.0.1:{server.server_address[1]}"
        step("local operator running", True, op_url)

        port = _free_port()
        log = open(tmp / "miner.log", "w")
        miner_env = {**env, "HOME": str(home), "USERPROFILE": str(home)}
        miner_proc = subprocess.Popen(_exe("ares-miner") + ["--no-browser", "--port", str(port)], env=miner_env,
                                      stdout=log, stderr=subprocess.STDOUT)
        base = f"http://127.0.0.1:{port}"
        page = None
        while time.time() < deadline and page is None:
            try:
                page = urllib.request.urlopen(base + "/", timeout=5).read().decode()
            except OSError:
                time.sleep(0.5)
        step("ares-miner window served on 127.0.0.1", page is not None and "ARES Miner" in page)
        token = re.search(r'const TOKEN="([^"]+)"', page).group(1)

        def api(method: str, name: str, body: dict | None = None) -> dict:
            req = urllib.request.Request(base + ("/api/state" if method == "GET" else "/api/" + name),
                                         json.dumps(body).encode() if body is not None else None,
                                         {"X-ARES-Token": token, "Content-Type": "application/json"}, method=method)
            try:
                with urllib.request.urlopen(req, timeout=600) as r:
                    return json.loads(r.read())
            except urllib.error.HTTPError as err:
                return json.loads(err.read())

        wallet = sc.Keypair.generate()  # throw-away; only its PUBLIC address is given to the Miner
        s = api("POST", "wallet", {"value": wallet.address})
        step("connect public address", s.get("config", {}).get("address") == wallet.address, s.get("error") or s.get("message"))
        api("POST", "settings", {"solver": args.solver, "workers": 1, "reveal_early": False, "pool": "", "rpc": ""})
        s = api("POST", "join", {"server": op_url, "verify_onchain": False})
        step("join season (challenge recomputed locally)", s.get("joined") and not s.get("error"), s.get("error") or s.get("message"))
        s = api("POST", "start", {})
        step("START MINING", s.get("miner", {}).get("running") or s.get("message") == "mining started", s.get("error"))

        def miner_state() -> dict:
            return api("GET", "state").get("miner", {})

        def wait(pred, what: str):
            last = {}
            while time.time() < deadline:
                last = miner_state()
                if pred(last):
                    return last
                time.sleep(2)
            step(what, False, {"events": last.get("events", [])[-12:], "error": last.get("error")})

        m = wait(lambda m: any(x.get("commit_seq") is not None for x in m.get("submissions", [])), "improvement committed")
        step("improvement committed", True, [e["message"] for e in m["events"] if e["kind"] in ("commit", "info")][-4:])
        step("no reveal while commits are open", not any(x["revealed"] for x in m["submissions"]))
        operator("close-commits", "--workdir", str(work), "--receipt-key", str(key))
        m = wait(lambda m: m.get("submissions") and all(x["revealed"] for x in m["submissions"] if x.get("commit_seq") is not None),
                 "Miner revealed automatically")
        step("Miner revealed automatically", True, [e["message"] for e in m["events"] if e["kind"] == "reveal"])
        operator("close-reveals", "--workdir", str(work), "--receipt-key", str(key), "--close-slot", "1000")
        operator("finalize", "--workdir", str(work), "--test-beacon", beacon("reward"))
        m = wait(lambda m: m.get("verification") is not None, "Miner verified results")
        step("Miner verified results independently", m["verification"].get("all_ok"), m["verification"].get("checks"))
        step("rank shown in Miner", m.get("rank") == 1, m.get("rank"))
        api("POST", "stop", {})

        season_dir = Path(json.loads((home / "config" / "miner.json").read_text())["season_dir"])
        cli = subprocess.run(_exe("ares-lite") + ["--dir", str(season_dir), "verify", "--no-onchain-check"], env=miner_env, capture_output=True, text=True)
        out = cli.stdout
        step("ares-lite verify", cli.returncode == 0 and json.loads(out[out.index("{"):])["all_ok"], (cli.stderr or out)[-1500:])
        cli = subprocess.run(_exe("ares-lite") + ["--dir", str(season_dir), "status"], env=miner_env, capture_output=True, text=True)
        step("ares-lite status (leaderboard)", cli.returncode == 0 and "<- you" in cli.stdout, (cli.stderr or cli.stdout)[-1500:])
        step("no private key stored by the Miner", wallet.seed.hex() not in (home / "config" / "miner.json").read_text())
        report["result"] = "PASS"
        return 0
    except SystemExit:
        report["result"] = "FAIL"
        return 1
    finally:
        if miner_proc is not None:
            miner_proc.terminate()
            try:
                miner_proc.wait(10)
            except subprocess.TimeoutExpired:
                miner_proc.kill()
        if server is not None:
            server.shutdown()
        try:
            report["miner_log_tail"] = (tmp / "miner.log").read_text(errors="replace")[-3000:]
        except OSError:
            pass
        Path(args.out).write_text(json.dumps(report, indent=2))
        print(f"report: {args.out} -> {report.get('result', 'FAIL')}")
        shutil.rmtree(tmp, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
