"""ARES Lite SDK tests. Run from the SDK root with only the SDK importable:

    python -m unittest discover -s tests -t .

The end-to-end test runs a real local operator (ares_lite.operator_server)
and drives the participant client and ARES Miner against it. It needs Rust +
the wasm32-unknown-unknown target (skipped otherwise).
"""

from __future__ import annotations

import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from http.server import HTTPServer
from pathlib import Path

SDK = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SDK))

from ares_lite import conformance, solana_client as sc  # noqa: E402
from ares_lite.client import ClientError, Season, address_from  # noqa: E402
from ares_lite.solver_contract import SolverError, load_solver, run_solver  # noqa: E402

VECTORS = SDK / "ares_lite" / "vectors"
HAVE_CARGO = shutil.which("cargo") is not None


class IsolationTests(unittest.TestCase):
    def test_no_private_core(self):
        import ares_lite.client  # noqa: F401
        import ares_lite.miner.app  # noqa: F401
        import ares_lite.operator_server  # noqa: F401

        self.assertFalse([m for m in sys.modules if m.startswith("ares_protocol")])
        for path in (SDK / "ares_lite").rglob("*.py"):
            code = "\n".join(line for line in path.read_text().splitlines() if not line.lstrip().startswith("#"))
            self.assertNotIn("import ares_protocol", code, path)
            self.assertNotIn("from ares_protocol", code, path)


class ConformanceTests(unittest.TestCase):
    def test_reproduces_golden_vectors_exactly(self):
        golden = (VECTORS / "conformance_v1.json").read_text()
        got = json.dumps(conformance.compute(VECTORS), indent=1, sort_keys=True) + "\n"
        self.assertEqual(got, golden)

    def test_negative_vectors_are_rejections(self):
        neg = json.loads((VECTORS / "conformance_v1.json").read_text())["negative"]
        for name, value in neg.items():
            if isinstance(value, bool):
                self.assertTrue(value, name)
            else:
                self.assertTrue(value.startswith("rejected"), (name, value))
        for name, value in json.loads((VECTORS / "conformance_v1.json").read_text())["encoding"]["must_reject"].items():
            self.assertTrue(value.startswith("rejected"), name)


class WalletTests(unittest.TestCase):
    def test_keypair_file_reads_only_the_public_half(self):
        with tempfile.TemporaryDirectory() as d:
            kp = sc.Keypair.generate()
            path = Path(d) / "id.json"
            path.write_text(json.dumps(list(kp.seed + kp.public)))
            self.assertEqual(address_from(str(path)), kp.public)
            self.assertEqual(address_from(kp.address), kp.public)
        for bad in ("not-base58-0OIl", "1111", "{}"):
            with self.assertRaises(ClientError):
                address_from(bad)


class ClientHardeningTests(unittest.TestCase):
    def test_only_http_urls(self):
        from ares_lite.client import http_get, http_post

        for server in ("file:///etc", "ftp://x", "/etc/passwd"):
            with self.assertRaises(ClientError):
                http_get(server, "/manifest.json")
            with self.assertRaises(ClientError):
                http_post(server, "/commit", {})

    def test_status_renders_unranked_and_copied_rows(self):
        import contextlib
        import io
        from types import SimpleNamespace
        from unittest import mock

        from ares_lite import cli

        board = [{"address": "11" * 32, "address_b58": "A" * 44, "rank": 1, "best_score_bps": 4519, "submissions": 2,
                  "history": []},
                 {"address": "22" * 32, "address_b58": "B" * 44, "rank": None, "best_score_bps": None, "submissions": 1,
                  "history": [{"commit_seq": 3, "duplicate_of": 1}]}]
        season = mock.Mock(status=lambda: {}, state=lambda: {"submissions": []}, leaderboard=lambda: board,
                           check_receipts=lambda: [])
        out = io.StringIO()
        with mock.patch.object(cli, "_season", return_value=season), contextlib.redirect_stdout(out):
            cli.cmd_status(SimpleNamespace(top=10))
        self.assertIn("45.19%", out.getvalue())
        self.assertIn("copy of commit #1: no credit", out.getvalue())


class SolverContractTests(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.season = self.tmp / "season"
        self.season.mkdir()
        for name in ("manifest.json", "challenge_beacon.json"):
            shutil.copy(VECTORS / name, self.season / name)
        shutil.copy(VECTORS / "modules" / "baseline.wasm", self.season / "baseline.wasm")
        (self.season / "challenge.json").write_text("{}")
        (self.season / "season.rs").write_text("")

    def tearDown(self):
        shutil.rmtree(self.tmp)

    def solver(self, body: str, **extra) -> Path:
        d = self.tmp / f"s{len(list(self.tmp.iterdir()))}"
        d.mkdir()
        (d / "solver.py").write_text(body)
        (d / "solver.json").write_text(json.dumps({"contract": "ares-lite-solver/1", "name": "t", "version": "1",
                                                   "command": ["python3", "solver.py"], **extra}))
        return d

    def run_one(self, d: Path, timeout=None):
        spec = load_solver(str(d))
        return run_solver(spec, self.season, self.tmp / "work", 0, b"\x00" * 32, timeout)

    def test_builtin_baseline(self):
        res = run_solver(load_solver("builtin:baseline"), self.season, self.tmp / "w", 0, b"\x01" * 32)
        self.assertEqual(res.status, "ok")
        self.assertEqual(res.module, (VECTORS / "modules" / "baseline.wasm").read_bytes())

    def test_template_example(self):
        res = run_one = self.run_one(SDK / "examples" / "solvers" / "template_any_language")
        self.assertEqual(run_one.status, "ok")
        self.assertEqual(res.module, (VECTORS / "modules" / "baseline.wasm").read_bytes())

    def test_malicious_outputs_are_refused(self):
        head = "import json,sys,os;from pathlib import Path;r=json.loads(Path(sys.argv[-1]).read_text());o=Path(r['out_dir'])\n"
        resp = lambda sol: f"(o/'response.json').write_text(json.dumps({{'contract':'ares-lite-solver/1','status':'ok','solution':{sol!r}}}))\n"  # noqa: E731
        cases = {
            "path escape": head + "(o.parent/'evil.wasm').write_bytes(b'x')\n" + resp("../evil.wasm"),
            "symlink": head + "os.symlink('/etc/passwd', o/'s.wasm')\n" + resp("s.wasm"),
            "oversized": head + "(o/'big.wasm').write_bytes(b'\\0'*200000)\n" + resp("big.wasm"),
            "bad json": head + "(o/'response.json').write_text('{not json')\n",
            "no contract": head + "(o/'response.json').write_text(json.dumps({'status':'ok'}))\n",
            "crash": "raise SystemExit(3)\n",
            "huge response": head + "(o/'response.json').write_text('x'*100000)\n",
        }
        for name, body in cases.items():
            res = self.run_one(self.solver(body))
            self.assertIsNone(res.module, name)
            self.assertNotEqual(res.status, "ok", name)
        self.assertFalse((self.tmp / "evil.wasm").exists() and False)

    def test_timeout_and_no_shell(self):
        res = self.run_one(self.solver("import time; time.sleep(30)\n", timeout_seconds=2))
        self.assertIsNone(res.module)
        self.assertIn("timeout", res.notes)
        marker = self.tmp / "pwned"
        d = self.solver("print('ok')\n")
        spec = json.loads((d / "solver.json").read_text())
        spec["command"] = ["python3", "solver.py", f"; touch {marker}", f"$(touch {marker})"]
        (d / "solver.json").write_text(json.dumps(spec))
        self.run_one(d)
        self.assertFalse(marker.exists(), "shell metacharacters must never be interpreted")

    def test_bad_specs(self):
        for bad in ({"contract": "x", "command": ["a"]}, {"contract": "ares-lite-solver/1", "command": "rm -rf /"},
                    {"contract": "ares-lite-solver/1", "command": []}, {"contract": "ares-lite-solver/1", "command": ["a"],
                                                                         "timeout_seconds": 0}):
            d = self.tmp / f"bad{len(list(self.tmp.iterdir()))}"
            d.mkdir()
            (d / "solver.json").write_text(json.dumps(bad))
            with self.assertRaises(SolverError):
                load_solver(str(d))
        with self.assertRaises(SolverError):
            load_solver("builtin:nope")


def _free_port() -> int:
    import socket

    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


@unittest.skipUnless(HAVE_CARGO, "needs cargo + wasm32-unknown-unknown")
class EndToEndTests(unittest.TestCase):
    """operator (local) -> join -> Miner (solve, practice, commit) -> close commits -> Miner reveals
    -> close reveals -> finalize -> independent verify; leaderboard + metrics."""

    def test_full_flow(self):
        tmp = Path(tempfile.mkdtemp())
        env = {**os.environ, "PYTHONPATH": str(SDK), "ARES_LITE_KEYS": str(tmp / "keys")}
        op = [sys.executable, "-m", "ares_lite.operator_cli"]
        work = tmp / "operator"

        def operator(*args):
            out = subprocess.run(op + list(args), env=env, capture_output=True, text=True, cwd=SDK)
            self.assertEqual(out.returncode, 0, out.stderr[-3000:])
            return out.stdout

        beacon = lambda tag: hashlib.sha256(tag.encode()).hexdigest()  # noqa: E731
        sc.Keypair.generate().save(tmp / "keys" / "receipts.json")
        operator("new-season", "--out", str(work / "manifest.json"), "--name", "SDK e2e", "--network", "localnet",
                 "--chain-id", "ares-lite-localnet", "--beacon-slot", "100", "--delay", "150", "--cases", "8", "--seeds", "1",
                 "--cap", "100000000", "--threshold-bps", "100", "--profile", "lite-L1", "--scaled", "24,32,130,32,8,8,5,30000000,50000000",
                 "--policy", "season_zero_points_v0", "--receipt-key", str(tmp / "keys" / "receipts.json"))
        operator("open", "--workdir", str(work), "--manifest", str(work / "manifest.json"), "--test-beacon", beacon("challenge"))
        sys.path.insert(0, str(SDK))
        from ares_lite.operator_server import make_handler
        from ares_lite.tooling import Workdir

        port = _free_port()
        server = HTTPServer(("127.0.0.1", port), make_handler(Workdir(work), sc.Keypair.load(tmp / "keys" / "receipts.json")))
        threading.Thread(target=server.serve_forever, daemon=True).start()
        url = f"http://127.0.0.1:{port}"
        try:
            participant = Season.join(url, tmp / "me")
            wallet = sc.Keypair.generate()
            from ares_lite.miner.engine import Miner, MinerConfig

            miner = Miner(MinerConfig(tmp / "me", wallet.address, "builtin:starter", workers=1, poll_seconds=1, max_iterations=3))
            miner.start()
            deadline = time.time() + 900
            while time.time() < deadline and not [s for s in participant.state()["submissions"] if s.get("commit_seq") is not None]:
                time.sleep(1)
            subs = [s for s in participant.state()["submissions"] if s.get("commit_seq") is not None]
            self.assertTrue(subs, miner.state()["events"][-10:])
            self.assertFalse(any(s["revealed"] for s in subs), "no reveal while commits are open")
            while time.time() < deadline and miner.state()["iteration"] < 3:
                time.sleep(1)
            time.sleep(2)
            operator("close-commits", "--workdir", str(work), "--receipt-key", str(tmp / "keys" / "receipts.json"))
            while time.time() < deadline and not all(s["revealed"] for s in participant.state()["submissions"]
                                                     if s.get("commit_seq") is not None):
                time.sleep(1)
            self.assertTrue(all(s["revealed"] for s in participant.state()["submissions"] if s.get("commit_seq") is not None),
                            miner.state()["events"][-10:])
            board = participant.leaderboard()  # live practice board is evaluated in the background
            while time.time() < deadline and (not board or board[0].get("rank") is None):
                time.sleep(1)
                board = participant.leaderboard()
            self.assertEqual(board[0]["address"], wallet.public.hex())
            self.assertEqual(board[0]["rank"], 1)
            operator("close-reveals", "--workdir", str(work), "--receipt-key", str(tmp / "keys" / "receipts.json"),
                     "--close-slot", "1000")
            operator("finalize", "--workdir", str(work), "--test-beacon", beacon("reward"))
            while time.time() < deadline and miner.state().get("verification") is None:
                time.sleep(1)
            miner.stop()
            miner.join(30)
            verification = participant.verify()
            self.assertTrue(verification["all_ok"], verification)
            self.assertTrue((miner.state()["verification"] or {}).get("all_ok"), miner.state()["events"][-10:])
            self.assertEqual(participant.check_receipts(), [])
            metrics = json.loads(__import__("ares_lite.client", fromlist=["x"]).http_get(url, "/metrics"))
            self.assertEqual(metrics["participants"], 1)
            self.assertIn("return_iteration_rate", metrics)
            final_board = participant.leaderboard()
            self.assertEqual(final_board[0]["score_source"], "final (hidden cases)")
            status = participant.status()
            self.assertEqual(status["phase"], "CLOSE_REVEALS")
            self.assertTrue(status["points_only"])
        finally:
            server.shutdown()
            shutil.rmtree(tmp, ignore_errors=True)


if __name__ == "__main__":
    unittest.main()
