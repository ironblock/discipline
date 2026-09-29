#!/usr/bin/env python3
"""The top logprobs at the first on/off divergence, read with no downtime: the
rendered prompt plus the arms' shared first tokens is prefilled once (n_predict 1),
so the position's distribution comes from the prefill, never from a draft. The
logprob difference of the top two equals their logit difference (softmax is
shift-invariant). Key read from the launch file, never printed. Usage: nprobs.py INVDIR"""
import json, os, re, sys, urllib.request
d = sys.argv[1]
cmd = open(os.path.expanduser('~/setup/prod-cmdline.txt')).read()
key = re.search(r'--api-key (\S+)', cmd).group(1)
def call(path, body):
    req = urllib.request.Request(f"http://127.0.0.1:8080{path}", data=json.dumps(body).encode(),
                                 headers={"Content-Type": "application/json", "Authorization": f"Bearer {key}"})
    with urllib.request.urlopen(req, timeout=300) as r: return json.load(r)
off = json.load(open(f"{d}/inv-off.json")); on = json.load(open(f"{d}/inv-on.json"))
t_off, t_on = off["rows"][0]["tokens"], on["rows"][0]["tokens"]
k = next(i for i, (a, b) in enumerate(zip(t_off, t_on)) if a != b)
assert t_off[:k] == t_on[:k]
rendered = call("/apply-template", {"messages": [{"role": "user", "content": off["meta"]["prompt"]}], "add_generation_prompt": True})["prompt"]
res = {"position": k, "off_token": t_off[k], "on_token": t_on[k], "reads": []}
for rep in range(3):
    r = call("/completion", {"prompt": [rendered] + t_off[:k], "n_predict": 1, "n_probs": 5, "post_sampling_probs": False,
                             "temperature": 0, "top_k": 1, "cache_prompt": False, "return_tokens": True})
    cp = r["completion_probabilities"][0]
    top = cp.get("top_logprobs") or cp.get("probs")
    res["reads"].append({"predicted": r.get("tokens"), "prompt_n": (r.get("timings") or {}).get("prompt_n"),
                         "top": [{"id": t.get("id"), "token": t.get("token"), "logprob": t.get("logprob")} for t in top]})
detok = call("/detokenize", {"tokens": [t_off[k]]}), call("/detokenize", {"tokens": [t_on[k]]})
res["off_text"], res["on_text"] = detok[0].get("content"), detok[1].get("content")
res["context_tail"] = call("/detokenize", {"tokens": t_off[max(0, k - 12):k]}).get("content")
top = res["reads"][0]["top"]
res["top2_logit_margin"] = round(top[0]["logprob"] - top[1]["logprob"], 6)
print(json.dumps(res, indent=1, ensure_ascii=False))
