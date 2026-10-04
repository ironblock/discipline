#!/usr/bin/env python3
"""#143 middle-rung profile: decode/prefill at depth on a local candidate server (no key).
  deep PORT TAG OUT PHASES   PHASES = "D:c,D:c,..." (D = prompt tokens, c = concurrent streams, each its own
                             corpus offset). rep 0 = cold prefill, reps 1-2 = warm (same prompt, cached).
Corpus: concatenated C/C++ sources from ~/git/llama.cpp (a stale checkout used as text only)."""
import glob, json, os, sys, threading, time, urllib.request, pathlib
def call(port, path, body=None, timeout=7200):
    req = urllib.request.Request(f"http://127.0.0.1:{port}{path}", data=json.dumps(body).encode() if body is not None else None,
                                 headers={"Content-Type": "application/json"}, method="POST" if body is not None else "GET")
    with urllib.request.urlopen(req, timeout=timeout) as r: return json.load(r)
home = os.path.expanduser("~")
files = sorted(set(f for pat in ["src/*.cpp", "src/*.h", "common/*.cpp", "common/*.h", "tools/**/*.cpp", "ggml/src/**/*.c",
                                   "ggml/src/**/*.cpp", "ggml/src/**/*.cu", "ggml/src/**/*.cuh", "ggml/src/**/*.h", "ggml/include/*.h"]
                   for f in glob.glob(home + "/git/llama.cpp/" + pat, recursive=True)))
corpus = "".join(f"\n\n// ===== {f.split('/git/llama.cpp/')[1]} =====\n" + open(f, errors="replace").read() for f in files)
Q = "\n\nReview the code above. Identify the three most likely bugs, explain each, and propose a fix with code."
port, tag, out, phases = int(sys.argv[2]), sys.argv[3], pathlib.Path(sys.argv[4]), sys.argv[5]
cpt = len(corpus[:60000]) / len(call(port, "/tokenize", {"content": corpus[:60000]})["tokens"])
def prompt(i, depth):
    # size by the server's own tokenizer: chars-per-token varies across the corpus, so rescale until within 1%
    n = int(depth * cpt)
    off = (i * 1_500_000) % max(1, len(corpus) - 2 * n - 1)
    for _ in range(5):
        t = len(call(port, "/tokenize", {"content": corpus[off:off + n] + Q})["tokens"])
        if abs(t - depth) <= depth * 0.01: break
        n = int(n * depth / t)
    return corpus[off:off + n] + Q
def erase():
    for s in range(4):
        try: call(port, f"/slots/{s}?action=erase", {})
        except Exception: pass
def one(i, depth, res):
    body = {"messages": [{"role": "user", "content": prompt(i, depth)}], "max_tokens": 256, "temperature": 0, "top_k": 1}
    t0 = time.time()
    try:
        r = call(port, "/v1/chat/completions", body); t = r.get("timings", {})
        res[i] = {"prompt_n": t.get("prompt_n"), "pp_tps": round(t.get("prompt_per_second") or 0, 1), "n": t.get("predicted_n"),
                  "tg_tps": round(t.get("predicted_per_second") or 0, 2), "draft_n": t.get("draft_n"), "draft_acc": t.get("draft_n_accepted"),
                  "wall_s": round(time.time() - t0, 1), "cached": (r.get("usage", {}).get("prompt_tokens_details") or {}).get("cached_tokens")}
    except Exception as e: res[i] = {"error": repr(e)[:200], "wall_s": round(time.time() - t0, 1)}
rows = []
for ph in phases.split(","):
    depth, c = (int(x) for x in ph.split(":"))
    erase()
    for rep in range(3):
        res = [None] * c; t0 = time.time()
        th = [threading.Thread(target=one, args=(i, depth, res)) for i in range(c)]
        [t.start() for t in th]; [t.join() for t in th]
        wall = time.time() - t0; ok = [r for r in res if r and "error" not in r]
        row = {"tag": tag, "depth": depth, "conc": c, "rep": rep, "wall_s": round(wall, 1), "streams": res,
               "agg_tok_s": round(sum(r["n"] or 0 for r in ok) / wall, 1) if ok else None}
        rows.append(row); print(json.dumps(row), flush=True)
out.write_text(json.dumps(rows, indent=1))
