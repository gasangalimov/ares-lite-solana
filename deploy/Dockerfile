# ARES Lite Season Zero operator (DEVNET / TEST-ONLY). Serves one season workdir over HTTP;
# Caddy (docker-compose.yml) terminates HTTPS in front of it. Build from the repo root:
#   docker build -f deploy/Dockerfile -t ares-lite-operator .
FROM python:3.12-slim
ENV PYTHONDONTWRITEBYTECODE=1 PYTHONUNBUFFERED=1
COPY sdk /src/sdk
RUN pip install --no-cache-dir /src/sdk && rm -rf /src && useradd --system --uid 10001 ares
USER ares
EXPOSE 8787
# /season: the season workdir (read/write: receipts are appended). /keys: receipt key (read-only).
HEALTHCHECK --interval=10s --start-period=5s --timeout=5s --retries=3 \
  CMD python -c "import urllib.request,sys; sys.exit(0 if urllib.request.urlopen('http://127.0.0.1:8787/healthz', timeout=4).status == 200 else 1)"
CMD ["python", "-m", "ares_lite.operator_server", "--workdir", "/season", "--host", "0.0.0.0", "--port", "8787", "--receipt-key", "/keys/receipts.json"]
