"""Does the floor's paged cache share a fork's prefix pages with the trunk's?
Warm a ~90k-token prefix, then run two concurrent requests on it (A, B), and a
control: A beside C, whose 90k prefix differs. Prints timings only."""
import json, os, random, sys, threading, time, urllib.request
base = sys.argv[1].rstrip('/')
url = base if base.endswith('/chat/completions') else base + ('/chat/completions' if base.endswith('/v1') else '/v1/chat/completions')
model = sys.argv[2]
n_lines = int(sys.argv[3]) if len(sys.argv) > 3 else 5000  # 3500 gave ~70k tokens, 5000 ~100k
seed = int(sys.argv[4]) if len(sys.argv) > 4 else 3
def filler(seed, n_lines):
    r = random.Random(seed); words = "alpha beta gamma delta render buffer vertex shader terrain chunk voxel player weapon bot spawn mesh light frame input camera".split()
    return "\n".join(f"{i:05d} " + " ".join(r.choice(words) for _ in range(14)) for i in range(n_lines))
P1 = filler(seed, n_lines); P2 = filler(seed + 1, n_lines)
def run(name, prefix, q, max_tokens, out):
    body = {"model": model, "stream": True, "max_tokens": max_tokens, "temperature": 0,
            "chat_template_kwargs": {"reasoning_effort": "low"},
            "messages": [{"role": "user", "content": prefix + "\n\nQuestion: " + q}]}
    t0 = time.time(); first = None; last = None; usage = None; timings = None
    req = urllib.request.Request(url, json.dumps(body).encode(), {"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=900) as r:
            for raw in r:
                line = raw.decode().strip()
                if not line.startswith("data:") or line == "data: [DONE]": continue
                d = json.loads(line[5:])
                if d.get("choices") and d["choices"][0].get("delta", {}).get("content") or (d.get("choices") and d["choices"][0].get("delta", {}).get("reasoning_content")):
                    if first is None: first = time.time()
                    last = time.time()
                usage = d.get("usage") or usage; timings = d.get("timings") or timings
        err = None
    except Exception as e:
        err = str(e)[:200]
    out[name] = {"start": t0, "first": first, "end": time.time(), "usage": usage, "timings": timings, "err": err}
def show(tag, res, t_zero):
    for k, v in res.items():
        f = None if v["first"] is None else round(v["first"] - t_zero, 1)
        print(f"{tag} {k}: start {round(v['start']-t_zero,1)}s first-token {f}s end {round(v['end']-t_zero,1)}s err={v['err']}")
        tm = v["timings"] or {}; us = v["usage"] or {}
        print(f"   prompt_tokens={us.get('prompt_tokens')} cached={us.get('cached_tokens')} cache_n={tm.get('cache_n')} prompt_n={tm.get('prompt_n')} prompt_ms={tm.get('prompt_ms')} predicted_n={tm.get('predicted_n')}")
# 1. warm P1 alone
res = {}; t = time.time(); run("warm", P1, "Reply with the word ok.", 1, res); show("warm", res, t)
# 2. A and B concurrently on the warm P1
res = {}; t = time.time()
ta = threading.Thread(target=run, args=("A", P1, "Name three words that appear in the text.", 300, res)); ta.start(); time.sleep(2)
tb = threading.Thread(target=run, args=("B", P1, "How many lines does the text have, roughly?", 300, res)); tb.start(); ta.join(); tb.join(); show("shared", res, t)
# 3. control: A on P1 (warm) beside C on a different 90k prefix
res = {}; t = time.time()
ta = threading.Thread(target=run, args=("A2", P1, "Name two words from the text.", 300, res)); ta.start(); time.sleep(2)
tc = threading.Thread(target=run, args=("C", P2, "Name two words from the text.", 300, res)); tc.start(); ta.join(); tc.join(); show("control", res, t)
