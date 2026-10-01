#!/usr/bin/env python3
"""C0b: one GET /props on the running production server (read-only, occupies no slot).
Keeps build_info's value: the raw bytes of its JSON token as received, and the decoded
UTF-8 bytes, each with its sha256. Names every other key (at any depth) whose name says
build or commit, or whose string value is commit-shaped; a value is kept only when it is
commit-shaped (hex, or b<num>-<hex>). The key is read from the launch file and never printed.
Usage: capture_c0b.py OUTDIR"""
import hashlib, json, os, re, sys, time, pathlib, urllib.request
out = pathlib.Path(sys.argv[1]); out.mkdir(parents=True, exist_ok=True)
cmd = open(os.path.expanduser('~/setup/prod-cmdline.txt')).read()
PORT = int(re.search(r'--port (\d+)', cmd).group(1)); KEY = re.search(r'--api-key (\S+)', cmd).group(1)
t0 = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
req = urllib.request.Request(f"http://127.0.0.1:{PORT}/props", headers={"Authorization": f"Bearer {KEY}"})
with urllib.request.urlopen(req, timeout=60) as r: status, body = r.status, r.read()
props = json.loads(body)
bi = props.get("build_info")
m = re.search(rb'"build_info"\s*:\s*("(?:[^"\\]|\\.)*")', body)
token = m.group(1) if m else None
COMMIT = re.compile(r'^(b\d+-)?[0-9a-f]{7,40}$')
others = []
def walk(o, p):
    if isinstance(o, dict):
        for k, v in o.items():
            q = f"{p}.{k}" if p else k
            if q == "build_info": continue
            named = re.search(r'build|commit', k, re.I) is not None
            shaped = isinstance(v, str) and COMMIT.match(v) is not None
            if named or shaped:
                others.append({"key": q, "type": type(v).__name__, "commit_shaped": shaped, **({"value": v} if shaped else {})})
            walk(v, q)
    elif isinstance(o, list):
        for i, v in enumerate(o): walk(v, f"{p}[{i}]")
walk(props, "")
rec = {"captured": t0, "request": "GET /props", "http_status": status, "response_bytes": len(body),
       "response_sha256": hashlib.sha256(body).hexdigest(),
       "build_info": {"present": "build_info" in props, "type": type(bi).__name__,
                      "json_token_raw": token.decode() if token else None,
                      "json_token_bytes": len(token) if token else None,
                      "json_token_sha256": hashlib.sha256(token).hexdigest() if token else None,
                      "value_utf8_bytes": len(bi.encode()) if isinstance(bi, str) else None,
                      "value_utf8_sha256": hashlib.sha256(bi.encode()).hexdigest() if isinstance(bi, str) else None,
                      "value": bi if isinstance(bi, str) else None},
       "other_build_or_commit_keys": others}
(out / "C0b-build-info.json").write_text(json.dumps(rec, indent=1) + "\n")
(out / "build_info.raw").write_bytes(token or b"")
print(json.dumps(rec, indent=1))
