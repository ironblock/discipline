#!/usr/bin/env python3
"""#143 middle-rung window probes against a local candidate server (no key on this box).
  fill PORT NP OUT      prefill NP slots concurrently to fill the unified context, sampling
                        nvidia-smi every 1 s; records peak VRAM and host memory
  kw PORT OUT           paired kwarg control, rendered effort per level, /props template digest
  inv PORT ARM K OUT    k greedy completions of one fixed prompt, token ids kept
  deep PORT TOKENS OUT  one decode sample after a cold TOKENS-token prompt
  gguf PATH             sha256 of the GGUF header's tokenizer.chat_template"""
import hashlib, json, struct, subprocess, sys, threading, time, urllib.request, pathlib

def call(port, path, body=None, timeout=3600):
    req = urllib.request.Request(f"http://127.0.0.1:{port}{path}", data=json.dumps(body).encode() if body is not None else None,
                                 headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=timeout) as r: return json.load(r)
def sha(s): return hashlib.sha256(s.encode() if isinstance(s, str) else s).hexdigest()
def ntok(port, text): return len(call(port, "/tokenize", {"content": text})["tokens"])
def smi():
    q = "memory.used,memory.total,utilization.gpu,power.draw"
    return subprocess.run(["nvidia-smi", f"--query-gpu={q}", "--format=csv,noheader,nounits"], capture_output=True, text=True).stdout.strip()
def filler(port, target, tag):
    unit = f"Ledger line for {tag}: the quick brown fox jumps over the lazy dog near the riverbank. "
    per = ntok(port, unit * 200) / 200
    n = int(target / per); text = unit * n
    while ntok(port, text) > target: n -= 50; text = unit * n
    return text

cmd = sys.argv[1]
if cmd == "fill":
    port, np_, out = int(sys.argv[2]), int(sys.argv[3]), pathlib.Path(sys.argv[4])
    props = call(port, "/props"); n_ctx = props["default_generation_settings"]["n_ctx"]
    per_slot = n_ctx // np_ - 512
    samples, stop = [], threading.Event()
    def sampler():
        while not stop.is_set():
            samples.append((time.time(), smi())); time.sleep(1)
    texts = [filler(port, per_slot, f"slot{i}-{time.time_ns()}") for i in range(np_)]
    res = [None] * np_
    def one(i):
        t0 = time.time()
        try:
            r = call(port, "/completion", {"prompt": texts[i], "n_predict": 1, "cache_prompt": True, "temperature": 0})
            res[i] = {"prompt_n": r["timings"]["prompt_n"], "prompt_ms": r["timings"]["prompt_ms"], "wall_s": round(time.time() - t0, 1), "id_slot": r.get("id_slot")}
        except Exception as e:
            res[i] = {"error": repr(e)[:300], "wall_s": round(time.time() - t0, 1)}
    st = threading.Thread(target=sampler); st.start()
    ths = [threading.Thread(target=one, args=(i,)) for i in range(np_)]
    [t.start() for t in ths]; [t.join() for t in ths]
    time.sleep(3); stop.set(); st.join()
    used = [int(s.split(",")[0]) for _, s in samples if s]
    total = int(samples[0][1].split(",")[1])
    mem = subprocess.run(["free", "-m"], capture_output=True, text=True).stdout
    rep = {"n_ctx": n_ctx, "np": np_, "per_slot_target": per_slot, "requests": res, "vram_total_mib": total,
           "vram_peak_mib": max(used), "vram_after_mib": used[-1], "vram_free_at_peak_mib": total - max(used),
           "samples": len(used), "free_m_after": mem, "passes_1gib": (total - max(used)) >= 1024 and all("error" not in r for r in res)}
    out.write_text(json.dumps(rep, indent=1)); print(json.dumps({k: rep[k] for k in ("vram_peak_mib", "vram_free_at_peak_mib", "passes_1gib")}))
elif cmd == "kw":
    port, out = int(sys.argv[2]), pathlib.Path(sys.argv[3])
    props = call(port, "/props"); tmpl = props.get("chat_template") or ""
    rep = {"props_template_sha256": sha(tmpl), "props_template_chars": len(tmpl),
           "template_mentions": {w: (w in tmpl) for w in ("enable_thinking", "reasoning_effort", "reasoning_strength", "xhigh", "preserve_thinking")}}
    q = [{"role": "user", "content": "What is 12 times 12? Answer with the number."}]
    for label, kw in (("thinking_disabled", {"enable_thinking": False}), ("thinking_enabled", {"enable_thinking": True}), ("default", None)):
        body = {"messages": q, "max_tokens": 1024, "temperature": 0.6}
        if kw is not None: body["chat_template_kwargs"] = kw
        r = call(port, "/v1/chat/completions", body); m = r["choices"][0]["message"]
        rep[label] = {"reasoning_chars": len(m.get("reasoning_content") or ""), "content": (m.get("content") or "")[:60], "completion_tokens": r["usage"]["completion_tokens"]}
    rep["negative_control"] = rep["thinking_disabled"]["reasoning_chars"] == 0
    rep["positive_control"] = rep["thinking_enabled"]["reasoning_chars"] > 0
    renders = {}
    for name in ("reasoning_effort", "reasoning_strength"):
        for level in (None, "none", "low", "medium", "high", "xhigh", "max"):
            body = {"messages": q, "add_generation_prompt": True}
            if level is not None: body["chat_template_kwargs"] = {name: level}
            p = call(port, "/apply-template", body)["prompt"]
            renders[f"{name}={level}"] = p
    base = renders["reasoning_effort=None"]
    def tail(p):
        i = 0
        while i < min(len(p), len(base)) and p[i] == base[i]: i += 1
        return p[max(0, i - 40):i + 200]
    rep["rendered_effort"] = {k: {"sha256": sha(v), "same_as_default": v == base, "differs_at": tail(v) if v != base else None} for k, v in renders.items()}
    out.write_text(json.dumps(rep, indent=1, ensure_ascii=False))
    print(json.dumps({k: rep[k] for k in ("props_template_sha256", "negative_control", "positive_control")}))
elif cmd == "inv":
    port, arm, k, out = int(sys.argv[2]), sys.argv[3], int(sys.argv[4]), pathlib.Path(sys.argv[5])
    PROMPT = ("Write a Python function that returns the n-th prime number, then explain its time "
              "complexity and one way to make it faster for large n.")
    rendered = call(port, "/apply-template", {"messages": [{"role": "user", "content": PROMPT}], "add_generation_prompt": True})["prompt"]
    reqp = {"temperature": 0, "top_k": 1, "cache_prompt": False, "seed": 924, "return_tokens": True}
    rows = []
    for i in range(k):
        t0 = time.time(); r = call(port, "/completion", {"prompt": rendered, "n_predict": 1024, **reqp})
        rows.append({"arm": arm, "rep": i, "latency_s": round(time.time() - t0, 2), "tokens": r.get("tokens"),
                     "n_tokens": len(r.get("tokens") or []), "timings": r.get("timings"), "stop_type": r.get("stop_type")})
        print(arm, i, rows[-1]["n_tokens"], rows[-1]["latency_s"], (r.get("timings") or {}).get("draft_n"), flush=True)
    out.mkdir(parents=True, exist_ok=True)
    (out / f"inv-{arm}.json").write_text(json.dumps({"meta": {"arm": arm, "prompt": PROMPT, "prompt_sha256": sha(PROMPT),
        "rendered_sha256": sha(rendered), "n_predict": 1024, "request": reqp}, "rows": rows}, indent=1))
elif cmd == "deep":
    port, tokens, out = int(sys.argv[2]), int(sys.argv[3]), pathlib.Path(sys.argv[4])
    text = filler(port, tokens, f"deep-{time.time_ns()}") + "\nSummarise the ledger above in three sentences."
    t0 = time.time()
    r = call(port, "/completion", {"prompt": text, "n_predict": 256, "cache_prompt": False, "temperature": 0, "top_k": 1})
    rep = {"prompt_tokens_target": tokens, "timings": r.get("timings"), "wall_s": round(time.time() - t0, 1), "label": "single sample"}
    out.write_text(json.dumps(rep, indent=1)); print(json.dumps(rep["timings"]))
elif cmd == "gguf":
    f = open(sys.argv[2], "rb")
    def u32(): return struct.unpack("<I", f.read(4))[0]
    def u64(): return struct.unpack("<Q", f.read(8))[0]
    def s(): return f.read(u64()).decode("utf-8", "replace")
    SZ = {0: 1, 1: 1, 2: 2, 3: 2, 4: 4, 5: 4, 6: 4, 7: 1, 10: 8, 11: 8, 12: 8}
    def skip(t):
        if t in SZ: f.read(SZ[t])
        elif t == 8: s()
        elif t == 9:
            et, n = u32(), u64()
            if et == 8:
                for _ in range(n): s()
            else: f.read(SZ[et] * n)
    assert f.read(4) == b"GGUF"; ver = u32(); nt, nkv = u64(), u64()
    found = None
    for _ in range(nkv):
        k = s(); t = u32()
        if k == "tokenizer.chat_template" and t == 8: found = s()
        else: skip(t)
    print(json.dumps({"gguf_version": ver, "chat_template_sha256": sha(found) if found else None, "chars": len(found) if found else None}))
