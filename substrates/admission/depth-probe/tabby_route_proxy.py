#!/usr/bin/env python3
"""The routing check of a parity fire on a TabbyAPI endpoint (#393 ruling 3, 5986337117). A pass-through proxy: before it forwards
each POST it reads GET /v1/model upstream and appends the id it saw to a check list; a request whose model read fails records
`null`, which apply.py's identity routing reads as another id. One proxy per seat, so the lists are per seat.
  tabby_route_proxy.py serve --upstream URL --listen PORT --out checks-A.json
  tabby_route_proxy.py box --a checks-A.json --b checks-B.json --model-id ID --instance DATE   -> prints the box's "routing" object
The key, when the upstream has one, is whatever the client sends; it is forwarded and never recorded."""
from __future__ import annotations
import argparse, json, sys, threading, urllib.request, urllib.error
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

def make_server(upstream, port, out):
    checks, lock = [], threading.Lock()
    def flush():
        with open(out, "w") as f: json.dump(checks, f)
    class H(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"
        def log_message(self, *a): pass
        def _fwd(self, method):
            n = int(self.headers.get("Content-Length") or 0); body = self.rfile.read(n) if n else None
            hdr = {k: v for k, v in self.headers.items() if k.lower() not in ("host", "connection", "content-length")}
            if method == "POST":
                try:
                    r = urllib.request.Request(upstream + "/v1/model", headers={k: v for k, v in hdr.items() if k.lower() == "authorization"})
                    mid = json.load(urllib.request.urlopen(r, timeout=30)).get("id")
                except Exception:
                    mid = None
                with lock: checks.append(mid); flush()
            req = urllib.request.Request(upstream + self.path, data=body, headers=hdr, method=method)
            try:
                resp = urllib.request.urlopen(req, timeout=7200); code = resp.status
            except urllib.error.HTTPError as e:
                resp, code = e, e.code
            data = resp.read(); self.send_response(code)
            for k, v in resp.headers.items():
                if k.lower() not in ("transfer-encoding", "connection", "content-length"): self.send_header(k, v)
            self.send_header("Content-Length", str(len(data))); self.end_headers(); self.wfile.write(data)
        def do_GET(self): self._fwd("GET")
        def do_POST(self): self._fwd("POST")
    flush()
    return ThreadingHTTPServer(("127.0.0.1", port), H)

def cmd_box(a):
    rd = lambda p: json.load(open(p))
    print(json.dumps({"model_id": a.model_id, "instance": a.instance, "checks": {"A": rd(a.a), "B": rd(a.b)}}, indent=1)); return 0

def cmd_selftest(a):
    import tempfile, os
    seen = []
    class U(BaseHTTPRequestHandler):
        def log_message(self, *x): pass
        def _s(self, o):
            b = json.dumps(o).encode(); self.send_response(200); self.send_header("Content-Length", str(len(b))); self.end_headers(); self.wfile.write(b)
        def do_GET(self): seen.append(("GET", self.path, self.headers.get("Authorization"))); self._s({"id": "m-1"})
        def do_POST(self): self.rfile.read(int(self.headers["Content-Length"])); seen.append(("POST", self.path, self.headers.get("Authorization"))); self._s({"ok": 1})
    up = ThreadingHTTPServer(("127.0.0.1", 0), U); threading.Thread(target=up.serve_forever, daemon=True).start()
    out = os.path.join(tempfile.mkdtemp(), "c.json")
    px = make_server(f"http://127.0.0.1:{up.server_address[1]}", 0, out); threading.Thread(target=px.serve_forever, daemon=True).start()
    base = f"http://127.0.0.1:{px.server_address[1]}"; ok = True
    def check(n, c):
        nonlocal ok; print(("ok   " if c else "FAIL ") + n); ok &= bool(c)
    for _ in range(3):
        urllib.request.urlopen(urllib.request.Request(base + "/v1/chat/completions", data=b"{}", headers={"Authorization": "Bearer k"}))
    check("one /v1/model read per POST, recorded", json.load(open(out)) == ["m-1"] * 3)
    check("a GET is forwarded and not counted", (urllib.request.urlopen(base + "/health").read() and json.load(open(out)) == ["m-1"] * 3))
    check("the client's key is forwarded to the model read and the request", all(x[2] == "Bearer k" for x in seen if x[0] == "POST") and seen[0][2] == "Bearer k")
    check("the key is not in the checks file", "Bearer" not in open(out).read() and '"k"' not in open(out).read())
    up.shutdown(); px.shutdown()
    return 0 if ok else 1

def main():
    p = argparse.ArgumentParser(); sub = p.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("serve"); s.add_argument("--upstream", required=True); s.add_argument("--listen", type=int, required=True); s.add_argument("--out", required=True)
    b = sub.add_parser("box"); [b.add_argument(f"--{x}", required=True) for x in ("a", "b", "model-id", "instance")]
    sub.add_parser("selftest"); a = p.parse_args()
    if a.cmd == "serve":
        make_server(a.upstream.rstrip("/"), a.listen, a.out).serve_forever()
    return {"box": cmd_box, "selftest": cmd_selftest}[a.cmd](a)

if __name__ == "__main__":
    sys.exit(main())
