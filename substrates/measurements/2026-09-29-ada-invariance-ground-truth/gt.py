#!/usr/bin/env python3
"""Ground-truth reads for the invariance result, all against production with no downtime.
  on PROMPT_ID K N_PROBS OUT      k greedy spec-on completions (1024 tokens), n_probs as asked
  scan PROMPT_ID TOKENS.json OUT  teacher-forced spec-off decode along a given token sequence:
                                  one n_predict-1 request per position on a pinned slot (draft budget 0,
                                  so the non-speculative path samples it, probs filled); records the
                                  argmax and top-2 logprobs at every position
  steps PROMPT_ID NMAX OUT        n_predict = 1..NMAX spec-on runs, recording draft_n / draft_n_accepted
The key is read from the launch file and never printed or written."""
import hashlib, json, os, re, sys, time, urllib.request
cmd = open(os.path.expanduser('~/setup/prod-cmdline.txt')).read()
KEY = re.search(r'--api-key (\S+)', cmd).group(1)
PROMPTS = {
    "p0": ("Write a Python function that returns the n-th prime number, then explain its time "
           "complexity and one way to make it faster for large n."),
    "p1": "A tank fills at 3 litres a minute and drains at 1.25 litres a minute. Starting empty, how long until it holds 42 litres? Show the reasoning.",
    "p2": "Write a short, vivid paragraph describing a harbour town at dawn, from the point of view of a fisherman's dog.",
    "p3": "Explain the difference between a mutex and a semaphore, with one example of when each is the right tool.",
    "p4": "List three causes of the French Revolution and give one sentence on how each contributed.",
}
def call(path, body):
    req = urllib.request.Request(f"http://127.0.0.1:8080{path}", data=json.dumps(body).encode(),
                                 headers={"Content-Type": "application/json", "Authorization": f"Bearer {KEY}"})
    with urllib.request.urlopen(req, timeout=900) as r: return json.load(r)
def rendered(pid):
    return call("/apply-template", {"messages": [{"role": "user", "content": PROMPTS[pid]}], "add_generation_prompt": True})["prompt"]
GREEDY = {"temperature": 0, "top_k": 1, "seed": 924, "return_tokens": True}
mode, pid = sys.argv[1], sys.argv[2]
R = rendered(pid)
meta = {"prompt_id": pid, "prompt": PROMPTS[pid], "rendered_sha256": hashlib.sha256(R.encode()).hexdigest()}
if mode == "on":
    k, nprobs, out = int(sys.argv[3]), int(sys.argv[4]), sys.argv[5]
    rows = []
    for i in range(k):
        body = {"prompt": R, "n_predict": 1024, "cache_prompt": False, **GREEDY}
        if nprobs: body.update({"n_probs": nprobs, "post_sampling_probs": False})
        r = call("/completion", body)
        cps = r.get("completion_probabilities") or []
        rows.append({"rep": i, "tokens": r.get("tokens"), "timings": r.get("timings"),
                     "probs_filled": [bool(c.get("top_logprobs") or c.get("probs")) for c in cps] if nprobs else None})
        print(pid, "on", i, len(r.get("tokens") or []), (r.get("timings") or {}).get("draft_n"), flush=True)
    json.dump({"meta": meta, "rows": rows}, open(out, "w"))
elif mode == "scan":
    toks, out = json.load(open(sys.argv[3])), sys.argv[4]
    call("/completion", {"prompt": R, "n_predict": 1, "cache_prompt": True, "id_slot": 3, **GREEDY})  # warm the slot on the prompt
    rows = []
    for q in range(len(toks)):
        r = call("/completion", {"prompt": [R] + toks[:q], "n_predict": 1, "cache_prompt": True, "id_slot": 3,
                                 "n_probs": 2, "post_sampling_probs": False, **GREEDY})
        top = (r["completion_probabilities"][0].get("top_logprobs") or r["completion_probabilities"][0].get("probs"))
        t = r.get("timings") or {}
        rows.append([q, r["tokens"][0], toks[q], round(top[0]["logprob"], 6), top[1]["id"], round(top[1]["logprob"], 6),
                     t.get("prompt_n"), t.get("draft_n")])
    json.dump({"meta": meta, "columns": ["pos", "argmax", "given", "argmax_logprob", "second_id", "second_logprob", "prompt_n", "draft_n"],
               "rows": rows}, open(out, "w"))
    mism = [r for r in rows if r[1] != r[2]]
    print(pid, "scan", len(rows), "positions; argmax != given at", [r[0] for r in mism][:20], flush=True)
elif mode == "steps":
    nmax, out = int(sys.argv[3]), sys.argv[4]
    rows = []
    for n in range(1, nmax + 1):
        r = call("/completion", {"prompt": R, "n_predict": n, "cache_prompt": False, **GREEDY})
        t = r.get("timings") or {}
        rows.append({"n_predict": n, "tokens": r.get("tokens"), "draft_n": t.get("draft_n"), "draft_n_accepted": t.get("draft_n_accepted")})
    json.dump({"meta": meta, "rows": rows}, open(out, "w"))
    print(pid, "steps done", flush=True)
