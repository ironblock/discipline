#!/usr/bin/env python3
"""Summarize the r3 window: per arm, the trunk's hit rate by step, cold prefill, forks, peak VRAM / min MemAvailable,
and ExLlamaV3's cache counters (deltas over the arm). Reads results.jsonl and box/{metrics,samp}-<arm>.* copied back."""
import collections, glob, json, os, statistics as st

HERE = os.path.dirname(os.path.abspath(__file__))
rows = [json.loads(l) for l in open(os.path.join(HERE, "results.jsonl"))]
arms = list(dict.fromkeys(r["arm"] for r in rows))

def metrics(arm):
    p = os.path.join(HERE, "box", f"metrics-{arm}.jsonl")
    if not os.path.exists(p):
        return None
    m = [json.loads(l) for l in open(p) if l.strip()]
    m = [x for x in m if "pagetable" in x]
    return (m[0], m[-1]) if m else None

def samp(arm):
    p = os.path.join(HERE, "box", f"samp-{arm}.csv")
    if not os.path.exists(p):
        return None
    v = [l.strip().split(",") for l in open(p) if l.count(",") == 3]
    v = [(int(a), int(b), int(c), int(d) if d else 0) for a, b, c, d in v if b and c]
    return {"peak_vram": max(x[1] for x in v), "min_avail": min(x[2] for x in v), "peak_rss": max(x[3] for x in v)}

out = []
for arm in arms:
    R = [r for r in rows if r["arm"] == arm]
    tr = sorted([r for r in R if r["kind"] == "trunk"], key=lambda r: r["step"])
    fk = [r for r in R if r["kind"] == "fork"]
    cold = [r for r in R if r["kind"] == "cold"] + [r for r in tr if r["step"] == 0]
    hits = [(r["step"], r["prompt_tokens"], r["cache_n"], (r["cache_n"] or 0) / r["prompt_tokens"], r["prompt_ms"], r["wall"]) for r in tr if r["step"] > 0]
    s = {"arm": arm,
         "trunk": [{"step": a, "prompt": b, "cache_n": c, "hit": round(d, 3), "prompt_s": round((e or 0) / 1000, 1), "wall_s": round(f, 1)} for a, b, c, d, e, f in hits],
         "hit_min": round(min(h[3] for h in hits), 3) if hits else None,
         "hit_median": round(st.median(h[3] for h in hits), 3) if hits else None,
         "reprefill_tokens": sum(h[1] - (h[2] or 0) for h in hits),
         "cold": [{"prompt": r["prompt_tokens"], "prompt_tps": round(r["prompt_n"] / (r["prompt_ms"] / 1000), 1) if r.get("prompt_ms") else None} for r in cold],
         "forks": [{"n": r["step"], "completion": r.get("completion_tokens"), "finish": r.get("finish"), "cache_n": r.get("cache_n"), "prompt": r.get("prompt_tokens")} for r in fk],
         "box": samp(arm)}
    m = metrics(arm)
    if m:
        a, b = m
        d = lambda k, x, y: {kk: (y[k] or {}).get(kk, 0) - (x[k] or {}).get(kk, 0) for kk in (y[k] or {}) if isinstance((y[k] or {}).get(kk), int)} if y[k] else None
        s["engine_delta"] = {"pagetable": d("pagetable", a, b), "recurrent": d("recurrent", a, b), "cpu_tier": d("cpu_tier", a, b)}
        s["engine_end"] = {"recurrent": b["recurrent"], "cpu_tier": b["cpu_tier"]}
    out.append(s)
print(json.dumps(out, indent=1))
