# Deploying the Season Zero operator (HTTPS)

> DEVNET / TEST-ONLY · NO MAINNET · SEASON ZERO POINTS-ONLY.

The operator is the existing reference server, `ares_lite.operator_server`. It serves one season working directory: the challenge, the baseline, the receipt log and the leaderboard. It accepts commits and reveals.

It holds no wallet keys. The only secret is the **receipt-signing key** (`receipts.json`), whose public half is pinned in the season manifest.

## 1. Host
- Any Linux VM: 1 vCPU, 1–2 GB RAM, 10 GB disk.
- Ports 80 and 443 open.
- A DNS **A/AAAA record**, for example `season-zero.example.org`, pointing at the VM.
- Docker with the Compose plugin.

## 2. Data layout on the host
```
/srv/ares-lite/
  season/            # season workdir (manifest.json, challenge.json, receipts.jsonl, ...)
  keys/receipts.json # receipt-signing key, chmod 600 — NEVER commit it
```

`chown -R 10001 /srv/ares-lite` makes the directory writable by the container user (uid 10001).

The season workdir is produced by the operator's round tooling (see §6). Copy it to the host with `rsync -a`.

## 3. Start
```
git clone https://github.com/gasangalimov/ares-lite-solana && cd ares-lite-solana
cp deploy/.env.example deploy/.env      # set ARES_DOMAIN and ARES_DATA
docker compose -f deploy/docker-compose.yml --env-file deploy/.env up -d --build
```

Caddy obtains the TLS certificate automatically. It redirects HTTP to HTTPS and adds HSTS. Request bodies are limited to 256 KB.

## 4. Health and diagnostics
| Check | Expect |
|---|---|
| `curl https://$DOMAIN/healthz` | `{"ok": true, "uptime_s": …, "phase_code": …, "head": …}` |
| `curl https://$DOMAIN/status` | phase, counts, `manifest_hash`, `points_only: true`, `schedule`, `server_unix`, `board_pending_evaluations` |
| `docker compose -f deploy/docker-compose.yml ps` | `operator` is `healthy` (Docker healthcheck every 10 s) |
| `docker compose -f deploy/docker-compose.yml logs -f operator` | request errors |

**Uptime monitor:** point any external monitor (for example UptimeRobot) at `/healthz`.

**Integrity check from anywhere:** `ares-lite --dir /tmp/obs join --server https://$DOMAIN` recomputes the challenge and checks the manifest hash against Solana devnet.

## 5. Restart, upgrade, backup
- **Restart:** `docker compose -f deploy/docker-compose.yml restart operator`. State is only the files in `season/`, so a restart loses nothing. The live practice board is recomputed in the background.
- **Upgrade the code:** `git pull && docker compose -f deploy/docker-compose.yml up -d --build`.
- **Backup:** copy `season/receipts.jsonl`, `receipts_signed.jsonl`, `received.jsonl` and `submission_metadata.jsonl` off the host at least hourly while a round is open. The receipt log is append-only, and participants hold signed receipts for every entry.
- **Never** edit `receipts.jsonl` by hand. Never restore an older copy over a newer one: that is an equivocation, and participants' receipts would expose it.

## 6. Round operations
The round lifecycle (create epoch → OPEN → close commits → reveals → close/finalize/publish on devnet) is run by the operator's round tooling against the same `season/` directory on this host. The reference server serves whatever the directory contains, so restart it after a new season directory is installed.

Commits and reveals are accepted only in the matching phase; the log enforces this.

The public schedule is `season/schedule.json`. The server publishes it at `/schedule.json` and inside `/status`.

## 7. Alternative without Docker
`deploy/ares-operator.service` is a hardened systemd unit for `127.0.0.1:8787`. Put Caddy or nginx with TLS in front of it.
