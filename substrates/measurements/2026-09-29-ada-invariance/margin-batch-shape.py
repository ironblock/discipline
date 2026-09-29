#!/usr/bin/env python3
"""Batch-shape probe at the divergence position, no downtime: warm a pinned slot with
the prompt plus the first (k - b) shared tokens, then ask for position k with the
prompt plus the first k tokens, so the server evaluates exactly b tokens in one batch
(b = 1 decode-like, b = 4 the size of an n_max 3 verify batch). Reads the top
logprobs at position k from that b-token batch. Key never printed. Usage: nprobs_batch.py INVDIR"""
import json, os, re, sys, urllib.request
d = sys.argv[1]
cmd = open(os.path.expanduser('~/setup/prod-cmdline.txt')).read()
key = re.search(r'--api-key (\S+)', cmd).group(1)
def call(path, body):
    req = urllib.request.Request(f"http://127.0.0.1:8080{path}", data=json.dumps(body).encode(),
                                 headers={"Content-Type": "application/json", "Authorization": f"Bearer {key}"})
    with urllib.request.urlopen(req, timeout=300) as r: return json.load(r)
off = json.load(open(f"{d}/inv-off.json")); on = json.load(open(f"{d}/inv-on.json"))
t = off["rows"][0]["tokens"]; k = next(i for i, (a, b) in enumerate(zip(t, on["rows"][0]["tokens"])) if a != b)
rendered = call("/apply-template", {"messages": [{"role": "user", "content": off["meta"]["prompt"]}], "add_generation_prompt": True})["prompt"]
out = {"position": k, "probes": []}
for b in (1, 4, 1, 4):
    call("/completion", {"prompt": [rendered] + t[:k - b], "n_predict": 1, "temperature": 0, "top_k": 1,
                         "cache_prompt": True, "id_slot": 0})
    r = call("/completion", {"prompt": [rendered] + t[:k], "n_predict": 1, "n_probs": 5, "post_sampling_probs": False,
                             "temperature": 0, "top_k": 1, "cache_prompt": True, "id_slot": 0})
    cp = r["completion_probabilities"][0]; top = cp.get("top_logprobs") or cp.get("probs")
    out["probes"].append({"batch_tokens_requested": b, "prompt_n": r["timings"]["prompt_n"], "cache_n": r["timings"].get("cache_n"),
                          "top": [[x["id"], x["token"], round(x["logprob"], 6)] for x in top]})
print(json.dumps(out, indent=1, ensure_ascii=False))
