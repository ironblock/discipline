#!/usr/bin/env python3
# Three-needle retrieval at a target depth through a TabbyAPI endpoint (no auth): wikitext-2 train prose as the haystack,
# needles at 10% / 50% / 90%, greedy, the answer read from the reply content. Usage: needle.py PORT TOKENS TAG OUT
import json, os, re, sys, time, urllib.request
port, target, tag, out = int(sys.argv[1]), int(sys.argv[2]), sys.argv[3], sys.argv[4]
H = os.path.expanduser("~/setup/wikitext-2-raw/wiki.train.raw")
text = re.sub(r"\n+", " ", open(H, errors="replace").read())
def call(path, body, timeout=3600):
    r = urllib.request.Request(f"http://127.0.0.1:{port}{path}", data=json.dumps(body).encode(), headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(r, timeout=timeout) as f: return json.load(f)
ntok = lambda s: len(call("/v1/token/encode", {"text": s})["tokens"])
cpt = 200000 / ntok(text[:200000])
needles = {"red": "417093", "green": "862514", "blue": "305768"}
Q = "\n\nThree access codes are stated somewhere in the text above. What are the access codes for the red, green and blue vaults? Answer exactly in the form red=NNNNNN, green=NNNNNN, blue=NNNNNN."
n = int(target * cpt)
for _ in range(4):
    hay = text[:n]
    for frac, (color, code) in zip((0.1, 0.5, 0.9), needles.items()):
        i = int(len(hay) * frac); i = hay.find(". ", i) + 2
        hay = hay[:i] + f"Important: the access code for the {color} vault is {code}. " + hay[i:]
    t = ntok(hay + Q)
    if abs(t - target) <= target * 0.01: break
    n = int(n * target / t)
body = {"messages": [{"role": "user", "content": hay + Q}], "max_tokens": 4096, "temperature": 0, "top_k": 1}
t0 = time.time()
try:
    r = call("/v1/chat/completions", body); err = None
except Exception as e:
    r, err = {}, repr(e)[:300]
content = ((r.get("choices") or [{}])[0].get("message") or {}).get("content") or ""
found = {c: (code in content) for c, code in needles.items()}
rec = {"tag": tag, "target": target, "prompt_tokens": (r.get("usage") or {}).get("prompt_tokens"), "wall_s": round(time.time() - t0, 1),
       "found": found, "score": sum(found.values()), "content": content[-400:], "timings": r.get("timings"), "error": err}
json.dump(rec, open(out, "w"), indent=1); print(json.dumps({k: rec[k] for k in ("tag", "target", "prompt_tokens", "score", "found", "wall_s", "error")}))
