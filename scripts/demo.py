"""Synthetic local demo; never reads or changes real account configuration."""
import http.server
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / "target/debug/token-manager"

class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/v1/credits":
            value = {"balance": "18.42", "total_used": "7.58"}
        elif self.path == "/v2/quotas":
            value = {"subscription": {"requests": 27, "limit": 135,
                                      "renewsAt": "2026-12-01T15:00:00Z"}}
        else:
            value = {"usage": {"rolling": {"percent": 28, "resetsAt": "2026-12-01T15:00:00Z"},
                               "weekly": {"percent": 46, "resetsAt": "2026-12-07T00:00:00Z"}}}
        body = json.dumps(value).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    def log_message(self, *_):
        pass

with tempfile.TemporaryDirectory(prefix="token-manager-demo-") as tmp:
    root = Path(tmp)
    config = root / "config"
    (config / "providers").mkdir(parents=True)
    key = root / "fixture.key"
    key.write_text("synthetic-demo-credential")
    key.chmod(0o600)
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        origins = {"vercel": "https://ai-gateway.vercel.sh",
                   "opencode-go": "https://opencode.ai",
                   "synthetic": "https://api.synthetic.new"}
        for provider, original in origins.items():
            definition = (ROOT / "providers" / f"{provider}.toml").read_text()
            definition = definition.replace(f'base_url = "{original}"', f'base_url = "http://127.0.0.1:{server.server_port}"')
            (config / "providers" / f"{provider}.toml").write_text(definition)
        args = [str(BINARY), "--config-dir", str(config), "--allow-local-http"]
        for provider in origins:
            subprocess.run(args + ["add", provider, "--key-file", str(key)], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        subprocess.run(args + ["add", "opencode-zen"], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        print("SYNTHETIC DEMO — local HTTP fixtures; no real account traffic", flush=True)
        result = subprocess.run(args + ["status", "--refresh"], check=False)
        if result.returncode != 2:
            raise SystemExit("Expected partial status: Zen balance is deliberately unsupported")
        # Check actual normalized output, not only a screenshot-like table.
        output = subprocess.run(args + ["status", "--cached", "--json"], capture_output=True, text=True)
        rows = json.loads(output.stdout)
        assert rows[0]["data"]["balances"][0]["remaining"] == "18.42"
        assert rows[1]["data"]["quotas"][0]["used_percent"] == "28"
        assert rows[2]["data"]["quotas"][0]["used"] == "27"
        assert rows[2]["data"]["quotas"][0]["limit"] == "135"
        assert rows[2]["data"]["quotas"][0]["used_percent"] == "20"
        assert rows[3]["state"] == "unsupported" and rows[3]["data"] is None
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=3)
