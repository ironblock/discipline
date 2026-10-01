#!/usr/bin/env python3
"""C0c: one GET /props on the floor's production server (read-only, occupies no slot; that server takes no key).
Keeps build_info's value as capture_c0b.py does: the raw bytes of its JSON token as received and the decoded UTF-8
bytes, each with its sha256, whatever they contain; names every other key (at any depth) whose name says build or
commit, or whose string value is commit-shaped, keeping a value only when it is commit-shaped.
Usage: capture_c0c.py BASE_URL OUTDIR"""
import hashlib, json, re, sys, time, pathlib, urllib.request
url, out = sys.argv[1].rstrip("/"), pathlib.Path(sys.argv[2]); out.mkdir(parents=True, exist_ok=True)
t0 = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
with urllib.request.urlopen(url + "/props", timeout=60) as r: status, body = r.status, r.read()
props = json.loads(body); bi = props.get("build_info")
m = re.search(rb'"build_info"\s*:\s*("(?:[^"\\]|\\.)*")', body); tok = m.group(1) if m else None
COMMIT = re.compile(r'^(b\d+-)?[0-9a-f]{7,40}$'); others = []
def walk(o, p):
    if isinstance(o, dict):
        for k, v in o.items():
            q = f"{p}.{k}" if p else k
            if q == "build_info": continue
            sh = isinstance(v, str) and bool(COMMIT.match(v))
            if re.search(r'build|commit', k, re.I) or sh:
                others.append({"key": q, "type": type(v).__name__, "commit_shaped": sh, **({"value": v} if sh else {})})
            walk(v, q)
    elif isinstance(o, list):
        for i, v in enumerate(o): walk(v, f"{p}[{i}]")
walk(props, "")
rec = {"captured": t0, "request": "GET /props", "http_status": status, "response_bytes": len(body),
       "response_sha256": hashlib.sha256(body).hexdigest(),
       "build_info": {"present": "build_info" in props, "type": type(bi).__name__,
                      "json_token_raw": tok.decode() if tok else None, "json_token_bytes": len(tok) if tok else None,
                      "json_token_sha256": hashlib.sha256(tok).hexdigest() if tok else None,
                      "value_utf8_bytes": len(bi.encode()) if isinstance(bi, str) else None,
                      "value_utf8_sha256": hashlib.sha256(bi.encode()).hexdigest() if isinstance(bi, str) else None,
                      "value": bi if isinstance(bi, str) else None},
       "other_build_or_commit_keys": others}
(out / "build-info-floor.json").write_text(json.dumps(rec, indent=1) + "\n"); (out / "build-info-floor.raw").write_bytes(tok or b"")
print(json.dumps(rec, indent=1))
