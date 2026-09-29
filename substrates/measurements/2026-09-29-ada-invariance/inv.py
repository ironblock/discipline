#!/usr/bin/env python3
"""On/off/off output invariance, one arm: k greedy completions of one fixed prompt
against the server on PORT, recording the generated token ids. The key is read
from the launch file and never printed or written. Usage: inv.py PORT ARM K OUTDIR"""
import hashlib, json, os, re, sys, time, urllib.request, pathlib
port, arm, k, out = int(sys.argv[1]), sys.argv[2], int(sys.argv[3]), pathlib.Path(sys.argv[4])
out.mkdir(parents=True, exist_ok=True)
cmd = open(os.path.expanduser('~/setup/prod-cmdline.txt')).read()
key = re.search(r'--api-key (\S+)', cmd).group(1)
PROMPT = ("Write a Python function that returns the n-th prime number, then explain its time "
          "complexity and one way to make it faster for large n.")
N_PREDICT = 1024
def call(path, body=None):
    req = urllib.request.Request(f"http://127.0.0.1:{port}{path}", data=json.dumps(body).encode() if body is not None else None,
                                 headers={"Content-Type": "application/json", "Authorization": f"Bearer {key}"})
    with urllib.request.urlopen(req, timeout=900) as r: return json.load(r)
rendered = call("/apply-template", {"messages": [{"role": "user", "content": PROMPT}], "add_generation_prompt": True})["prompt"]
meta = {"arm": arm, "port": port, "prompt": PROMPT, "prompt_sha256": hashlib.sha256(PROMPT.encode()).hexdigest(),
        "rendered_sha256": hashlib.sha256(rendered.encode()).hexdigest(), "n_predict": N_PREDICT,
        "request": {"temperature": 0, "top_k": 1, "cache_prompt": False, "seed": 924, "return_tokens": True}}
rows = []
for i in range(k):
    body = {"prompt": rendered, "n_predict": N_PREDICT, **meta["request"]}
    t0 = time.time(); r = call("/completion", body); dt = round(time.time() - t0, 2)
    toks = r.get("tokens")
    rows.append({"arm": arm, "rep": i, "latency_s": dt, "tokens": toks, "n_tokens": len(toks) if toks is not None else None,
                 "content_sha256": hashlib.sha256((r.get("content") or "").encode()).hexdigest(),
                 "stop_type": r.get("stop_type"), "timings": r.get("timings")})
    print(f"{arm} rep {i}: {rows[-1]['n_tokens']} tokens, {dt}s, draft_n={(r.get('timings') or {}).get('draft_n')}", flush=True)
(out / f"inv-{arm}.json").write_text(json.dumps({"meta": meta, "rows": rows}, indent=1))
