"""ARES Miner local app: only this machine's own window can drive it, and no key ever leaves."""

from __future__ import annotations

import http.client
import json
import os
import sys
import tempfile
import threading
import unittest
from pathlib import Path

SDK = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SDK))


class MinerAppSecurityTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.home = tempfile.mkdtemp()
        os.environ["ARES_LITE_HOME"] = cls.home
        import importlib

        import ares_lite.miner.app as app

        importlib.reload(app)  # pick up ARES_LITE_HOME
        cls.app = app
        cls.server, cls.token = app.serve(0, open_ui=False)
        cls.port = cls.server.server_address[1]
        threading.Thread(target=cls.server.serve_forever, daemon=True).start()

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()

    def req(self, method, path, body=None, headers=None):
        conn = http.client.HTTPConnection("127.0.0.1", self.port, timeout=10)
        h = {"Host": f"127.0.0.1:{self.port}", "Content-Type": "application/json", **(headers or {})}
        conn.request(method, path, json.dumps(body).encode() if body is not None else None, h)
        r = conn.getresponse()
        return r.status, r.read()

    def test_binds_localhost_only(self):
        self.assertEqual(self.server.server_address[0], "127.0.0.1")

    def test_token_host_and_origin_are_enforced(self):
        self.assertEqual(self.req("GET", "/api/state")[0], 403)                                       # no token
        self.assertEqual(self.req("GET", "/api/state", headers={"X-ARES-Token": "wrong"})[0], 403)
        ok = {"X-ARES-Token": self.token}
        self.assertEqual(self.req("GET", "/api/state", headers=ok)[0], 200)
        self.assertEqual(self.req("GET", "/api/state", headers={**ok, "Host": "evil.example:80"})[0], 403)  # DNS rebinding
        self.assertEqual(self.req("POST", "/api/start", {}, {**ok, "Origin": "https://evil.example"})[0], 403)  # CSRF
        self.assertEqual(self.req("GET", "/", headers={"Host": "evil.example"})[0], 403)
        self.assertEqual(self.req("POST", "/api/stop", {})[0], 403)

    def test_body_limit_and_unknown_action(self):
        ok = {"X-ARES-Token": self.token}
        self.assertEqual(self.req("POST", "/api/settings", {"pool": "x" * 20_000}, ok)[0], 413)
        self.assertEqual(self.req("POST", "/api/rm-rf", {}, ok)[0], 400)

    def test_keypair_file_is_reduced_to_its_public_key(self):
        from ares_lite import solana_client as sc

        kp = sc.Keypair.generate()
        with tempfile.TemporaryDirectory() as d:
            path = Path(d) / "id.json"
            path.write_text(json.dumps(list(kp.seed + kp.public)))
            status, body = self.req("POST", "/api/wallet", {"value": str(path)}, {"X-ARES-Token": self.token})
        self.assertEqual(status, 200)
        self.assertEqual(json.loads(body)["config"]["address"], kp.address)
        saved = (Path(self.home) / "miner.json").read_text()
        self.assertIn(kp.address, saved)
        self.assertNotIn(kp.seed.hex(), saved)
        self.assertNotIn(str(list(kp.seed))[1:40], saved)
        self.assertNotIn("id.json", saved)  # not even the key file path is kept


class MinerResilienceTests(unittest.TestCase):
    def test_transient_operator_error_does_not_stop_reveals(self):
        from unittest import mock

        from ares_lite.client import ClientError
        from ares_lite.miner.engine import Miner, MinerConfig

        with tempfile.TemporaryDirectory() as d:
            miner = Miner(MinerConfig(Path(d), "11111111111111111111111111111111", "builtin:baseline", poll_seconds=0))
            miner.snapshot["baseline_fuel"] = 1000
            calls = {"status": 0, "reveal": []}

            def status():
                calls["status"] += 1
                if calls["status"] == 1:
                    raise ClientError("GET /status: connection refused")
                return {"phase_code": 3}  # CLOSE_COMMITS

            def reveal(sub_id):
                calls["reveal"].append(sub_id)
                miner.stop()

            season = mock.Mock()
            season.path = Path(d)
            season.status = status
            season.reveal = reveal
            season.state = lambda: {"submissions": [{"id": 7, "commit_seq": 1, "revealed": False, "module_sha256": "x"}]}
            season.leaderboard = lambda: []
            season.workdir.return_value.manifest.return_value.name = "t"
            season.workdir.return_value.manifest.return_value.season_id = 1
            season.workdir.return_value.challenge.return_value.challenge_id = b"\0" * 32
            miner.season = season
            miner.start()
            miner.join(20)
            self.assertEqual(calls["reveal"], [7])
            self.assertTrue(any("retrying" in e["message"] for e in miner.state()["events"]))


if __name__ == "__main__":
    unittest.main()
