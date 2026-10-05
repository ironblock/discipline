#!/usr/bin/env python3
"""Selftest for tabby_capture421.py against a mock server: a 4xx on the llama.cpp-only key is kept and re-sent without it, a tool call is
assembled from a streamed reply, the openai-shape re-send is built from it, GETs are kept as bytes, and the key appears in no kept file."""
import json, os, pathlib, subprocess, sys, tempfile, threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
HERE = pathlib.Path(__file__).resolve().parent; I0 = HERE.parents[1] / "measurements/2026-10-02-i0-tool-call-captures"
KEY = "selftest-key-421"; seen = []
SSE = ('data: {"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"c1","type":"function","function":{"name":"bash","arguments":"{\\"command\\":\\"ls | wc -l\\"}"}}]}}]}\n\n'
       'data: {"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}\n\ndata: [DONE]\n\n').encode()
class H(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.0"
    def log_message(self, *x): pass
    def reply(self, code, body, ct="application/json"):
        self.send_response(code); self.send_header("Content-Type", ct); self.send_header("Content-Length", str(len(body))); self.end_headers(); self.wfile.write(body)
    def do_GET(self):
        seen.append(("GET", self.path, self.headers.get("Authorization"))); self.reply(200, json.dumps({"id": "m"}).encode())
    def do_POST(self):
        b = json.loads(self.rfile.read(int(self.headers["Content-Length"]))); seen.append(("POST", self.path, self.headers.get("Authorization"), b))
        if "return_progress" in b: return self.reply(400, b'{"detail":"extra key"}')
        self.reply(200, SSE, "text/event-stream")
def main() -> int:
    srv = ThreadingHTTPServer(("127.0.0.1", 0), H); threading.Thread(target=srv.serve_forever, daemon=True).start()
    ok = True
    def check(n, c):
        nonlocal ok; ok &= bool(c); print(("ok   " if c else "FAIL ") + n)
    with tempfile.TemporaryDirectory() as t:
        env = dict(os.environ, TABBY_API_KEY=KEY); out = pathlib.Path(t) / "c"
        r = subprocess.run([sys.executable, str(HERE / "tabby_capture421.py"), "capture", "--endpoint", f"http://127.0.0.1:{srv.server_address[1]}", "--i0-dir", str(I0), "--out", str(out)], env=env, capture_output=True, text=True)
        check("capture ran", r.returncode == 0)
        n = json.loads((out / "notes.json").read_text())
        check("the refused body is kept and the re-send is 200", n["turn1-off.refused"]["status"] == 400 and n["turn1-off"]["status"] == 200 and "return_progress" not in (out / "turn1-off.request.json").read_text() and "return_progress" in (out / "turn1-off.refused.request.json").read_text())
        check("the tool call is assembled and re-sent in the openai shape", n["turn1-off_assembled"]["tool_calls"][0]["name"] == "bash" and '"role":"tool"' in (out / "turn2-openai.request.json").read_text())
        check("thinking on and off differ in the request", '"enable_thinking":true' in (out / "turn1-on.request.json").read_text() and '"enable_thinking":false' in (out / "turn1-off.request.json").read_text())
        check("three GETs kept as bytes", all((out / f"{x}.http").exists() for x in ("get-v1-model", "get-v1-models", "get-health")))
        check("every request carried the bearer", all(s[2] == f"Bearer {KEY}" for s in seen))
        check("verify is green and the key is in no kept file", subprocess.run([sys.executable, str(HERE / "tabby_capture421.py"), "verify", str(out)], env=env).returncode == 0)
        (out / "turn1-off.http").write_bytes(b"x"); check("verify refuses a tampered reply", subprocess.run([sys.executable, str(HERE / "tabby_capture421.py"), "verify", str(out)], env=env, capture_output=True).returncode == 1)
    srv.shutdown(); return 0 if ok else 1
if __name__ == "__main__": sys.exit(main())
