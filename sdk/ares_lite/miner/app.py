"""ARES Miner desktop app: a local window (127.0.0.1 only) around the miner engine.

    ares-miner                      # opens the Miner window
    ares-miner --headless --server URL --address <PUBKEY> [--solver builtin:starter]

DEVNET / TEST-ONLY. Season Zero is POINTS-ONLY: no token reward, no airdrop promise.
The app never asks for, stores or sends a private key: mining needs only your
PUBLIC wallet address (a commitment binds it).
"""

from __future__ import annotations

import argparse
import json
import os
import secrets
import shutil
import subprocess
import sys
import threading
import webbrowser
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import urlparse

from .. import solana_client as sc
from ..client import DEVNET_RPC, ClientError, Season, address_from
from .engine import Miner, MinerConfig

CONFIG_DIR = Path(os.environ.get("ARES_LITE_HOME", Path.home() / ".config" / "ares-lite"))
CONFIG_FILE = CONFIG_DIR / "miner.json"
DEFAULT_SEASON_DIR = Path.home() / "ares-lite-season"
MAX_BODY = 16_384


def load_config() -> dict:
    try:
        return json.loads(CONFIG_FILE.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return {}


def save_config(cfg: dict) -> None:
    CONFIG_DIR.mkdir(parents=True, exist_ok=True)
    allowed = ("address", "server", "solver", "workers", "reveal_early", "rpc", "season_dir", "pool", "verify_onchain")
    CONFIG_FILE.write_text(json.dumps({k: cfg[k] for k in allowed if k in cfg}, indent=2), encoding="utf-8")


class App:
    def __init__(self, cfg: dict) -> None:
        self.cfg = {"solver": "builtin:starter", "workers": 1, "reveal_early": False, "rpc": DEVNET_RPC,
                    "season_dir": str(DEFAULT_SEASON_DIR), "verify_onchain": True, **cfg}
        self.miner: Miner | None = None
        self.message = ""

    def make_miner(self) -> Miner:
        c = self.cfg
        return Miner(MinerConfig(Path(c["season_dir"]), c.get("address", ""), c.get("solver", "builtin:starter"),
                                 max(1, min(int(c.get("workers", 1)), os.cpu_count() or 1)), reveal_early=bool(c.get("reveal_early")),
                                 rpc=c.get("rpc") if c.get("verify_onchain") else None, pool=str(c.get("pool", ""))[:64]))

    def state(self) -> dict:
        base = {"config": {k: v for k, v in self.cfg.items()}, "message": self.message,
                "joined": (Path(self.cfg["season_dir"]) / "participant.json").exists()}
        miner = self.miner or (self.make_miner() if base["joined"] else None)
        if miner is not None:
            base["miner"] = miner.state()
        return base

    def action(self, name: str, body: dict) -> dict:
        self.message = ""
        if name == "wallet":
            value = str(body.get("value", "")).strip()
            self.cfg["address"] = sc.b58encode(address_from(value))  # a keypair file is reduced to its public key here
            save_config(self.cfg)
            self.message = f"wallet connected: {self.cfg['address']} (public key only)"
        elif name == "join":
            server = str(body.get("server", "")).strip()
            if not server.startswith(("http://", "https://")):
                raise ClientError("operator URL must start with http:// or https://")
            self.cfg["server"] = server
            self.cfg["verify_onchain"] = bool(body.get("verify_onchain", True))
            season = Season.join(server, Path(self.cfg["season_dir"]), self.cfg["rpc"] if self.cfg["verify_onchain"] else None)
            save_config(self.cfg)
            self.message = f"joined {season.workdir().manifest().name}: challenge recomputed locally" + \
                (" and manifest checked on Solana devnet" if self.cfg["verify_onchain"] else "")
        elif name == "settings":
            for key, cast in (("solver", str), ("workers", int), ("reveal_early", bool), ("pool", str), ("rpc", str)):
                if key in body:
                    self.cfg[key] = cast(body[key])
            save_config(self.cfg)
            self.message = "settings saved (applied on next start)"
        elif name == "start":
            if self.miner and self.miner.running:
                return self.state()
            self.miner = self.make_miner()
            self.miner.start()
            self.message = "mining started"
        elif name == "stop":
            if self.miner:
                self.miner.stop()
            self.message = "stopping after the current iteration (state is saved; you can resume)"
        elif name == "reveal":
            miner = self.miner or self.make_miner()
            miner._reveal_pending(only_if=True)
            self.message = "pending commitments revealed"
        else:
            raise ClientError(f"unknown action {name}")
        return self.state()


def make_handler(app: App, token: str, port_ref: list):
    page = (Path(__file__).with_name("ui.html")).read_text(encoding="utf-8").replace("__TOKEN__", token)

    class Handler(BaseHTTPRequestHandler):
        server_version = "ares-miner/0"

        def _send(self, status: int, body: bytes, ctype: str = "application/json") -> None:
            self.send_response(status)
            self.send_header("Content-Type", ctype)
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Cache-Control", "no-store")
            self.send_header("X-Frame-Options", "DENY")
            self.send_header("Content-Security-Policy", "default-src 'self'; script-src 'unsafe-inline'; style-src 'unsafe-inline'")
            self.end_headers()
            self.wfile.write(body)

        def _origin_ok(self) -> bool:
            # Only this local app: blocks DNS rebinding (Host) and cross-site requests (Origin).
            allowed = {f"127.0.0.1:{port_ref[0]}", f"localhost:{port_ref[0]}"}
            if self.headers.get("Host") not in allowed:
                return False
            origin = self.headers.get("Origin")
            return origin is None or urlparse(origin).netloc in allowed

        def _authorized(self) -> bool:
            return self._origin_ok() and secrets.compare_digest(self.headers.get("X-ARES-Token", ""), token)

        def do_GET(self) -> None:  # noqa: N802
            if not self._origin_ok():
                return self._send(403, b'{"error":"forbidden"}')
            if self.path == "/" or self.path.startswith("/?"):
                return self._send(200, page.encode(), "text/html; charset=utf-8")
            if self.path == "/api/state":
                if not self._authorized():
                    return self._send(403, b'{"error":"forbidden"}')
                return self._send(200, json.dumps(app.state()).encode())
            self._send(404, b'{"error":"not found"}')

        def do_POST(self) -> None:  # noqa: N802
            if not self._authorized():
                return self._send(403, b'{"error":"forbidden"}')
            length = int(self.headers.get("Content-Length", "0") or 0)
            if length > MAX_BODY:
                return self._send(413, b'{"error":"body too large"}')
            try:
                body = json.loads(self.rfile.read(length) or b"{}")
                if not self.path.startswith("/api/") or not isinstance(body, dict):
                    return self._send(404, b'{"error":"not found"}')
                out = app.action(self.path[len("/api/"):], body)
                self._send(200, json.dumps(out).encode())
            except Exception as exc:  # every failure is reported to the window, never a dropped connection
                app.message = f"error: {type(exc).__name__}: {exc}"[:500]
                self._send(400, json.dumps({"error": app.message, **app.state()}).encode())

        def log_message(self, *args) -> None:
            pass

    return Handler


def open_window(url: str) -> None:
    """An app-style window when Edge/Chrome/Chromium is installed, else the default browser."""

    candidates = ["msedge", "chrome", "google-chrome", "chromium", "chromium-browser"]
    if sys.platform == "win32":
        for base in (os.environ.get("PROGRAMFILES(X86)", ""), os.environ.get("PROGRAMFILES", "")):
            candidates.insert(0, str(Path(base) / "Microsoft" / "Edge" / "Application" / "msedge.exe"))
    for exe in candidates:
        path = exe if Path(exe).is_file() else shutil.which(exe)
        if path:
            subprocess.Popen([path, f"--app={url}", "--new-window"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            return
    webbrowser.open(url)


def serve(port: int = 0, open_ui: bool = True) -> tuple[ThreadingHTTPServer, str]:
    app = App(load_config())
    token = secrets.token_urlsafe(24)
    port_ref = [port]
    server = ThreadingHTTPServer(("127.0.0.1", port), make_handler(app, token, port_ref))
    port_ref[0] = server.server_address[1]
    url = f"http://127.0.0.1:{port_ref[0]}/"
    if open_ui:
        threading.Timer(0.3, open_window, args=(url,)).start()
    return server, token


def headless(args) -> None:
    season_dir = Path(args.season_dir)
    if args.server:
        Season.join(args.server, season_dir, None if args.no_onchain_check else args.rpc)
    miner = Miner(MinerConfig(season_dir, sc.b58encode(address_from(args.address)), args.solver, args.workers,
                              poll_seconds=args.poll, reveal_early=args.reveal_early,
                              rpc=None if args.no_onchain_check else args.rpc, max_iterations=args.iterations))
    miner.start()
    seen = 0
    try:
        while miner.running:
            miner.join(1.0)
            for event in miner.state()["events"][seen:]:
                print(f"[{event['kind']}] {event['message']}", flush=True)
            seen = len(miner.state()["events"])
            if args.until_revealed and miner.state()["submissions"] and all(s["revealed"] for s in miner.state()["submissions"]):
                miner.stop()
    except KeyboardInterrupt:
        miner.stop()
        miner.join()
    print(json.dumps({k: v for k, v in miner.state().items() if k != "events"}, indent=2, default=str))


def main(argv=None) -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--headless", action="store_true")
    ap.add_argument("--port", type=int, default=0)
    ap.add_argument("--no-browser", action="store_true")
    ap.add_argument("--server")
    ap.add_argument("--address", help="your PUBLIC wallet address, or a Solana CLI keypair file (only its public half is read)")
    ap.add_argument("--solver", default="builtin:starter")
    ap.add_argument("--workers", type=int, default=1)
    ap.add_argument("--season-dir", default=str(DEFAULT_SEASON_DIR))
    ap.add_argument("--rpc", default=DEVNET_RPC)
    ap.add_argument("--no-onchain-check", action="store_true")
    ap.add_argument("--reveal-early", action="store_true")
    ap.add_argument("--iterations", type=int)
    ap.add_argument("--poll", type=int, default=10)
    ap.add_argument("--until-revealed", action="store_true", help="exit once every commitment has been revealed")
    args = ap.parse_args(argv)
    if args.headless:
        if not args.address:
            raise SystemExit("--address is required in headless mode")
        return headless(args)
    server, token = serve(args.port, not args.no_browser)
    url = f"http://127.0.0.1:{server.server_address[1]}/"
    print(f"ARES Miner running at {url} (local only; close this window or press Ctrl+C to quit)", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
