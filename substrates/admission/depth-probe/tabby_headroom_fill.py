#!/usr/bin/env python3
"""The headroom fill against a TabbyAPI endpoint (#393): max_batch_size requests share the KV pool, each filled to
min(pool / slots - 1024, max_seq_len - 1024) tokens with a request-unique filler, while nvidia-smi is sampled; the peak gives
`vram_free_at_peak_mib`, in the shape the floor record's raw/fill.json has (admission-recompute.sh reads exactly that field and
any "error" key). --cap-per-request and --slots bound a dry run on a production endpoint; a capped run is marked so and is
never a record. Token counts come from /v1/token/encode on the string; each request's usage.prompt_tokens is recorded.
The key, when the endpoint has one, comes from TABBY_API_KEY and is never written."""
from __future__ import annotations
import argparse, json, os, subprocess, sys, threading, time, urllib.request

def smi(field: str) -> int:
    return int(subprocess.check_output(["nvidia-smi", f"--query-gpu={field}", "--format=csv,noheader,nounits"]).split()[0])

def main() -> int:
    ap = argparse.ArgumentParser(); ap.add_argument("--endpoint", required=True); ap.add_argument("--out", required=True)
    ap.add_argument("--bar-mib", type=int, default=300, help="the criterion's free VRAM at peak")
    ap.add_argument("--cap-per-request", type=int, default=0, help="dry run: tokens per request ceiling (0 = none)")
    ap.add_argument("--slots", type=int, default=0, help="dry run: concurrent requests (0 = the endpoint's max_batch_size)")
    a = ap.parse_args(); key = os.environ.get("TABBY_API_KEY")
    def call(path, body=None, timeout=7200):
        h = {"Content-Type": "application/json"}
        if key: h["Authorization"] = f"Bearer {key}"
        r = urllib.request.Request(a.endpoint.rstrip("/") + path, data=json.dumps(body).encode() if body is not None else None, headers=h)
        with urllib.request.urlopen(r, timeout=timeout) as f: return json.load(f)
    m = call("/v1/model")["parameters"]; pool, maxseq, bs = m["cache_size"], m["max_seq_len"], m["max_batch_size"]
    slots = a.slots or bs; per = min(pool // bs - 1024, maxseq - 1024)
    if a.cap_per_request: per = min(per, a.cap_per_request)
    unit = "Ledger line {}: the quick brown fox jumps over the lazy dog near the riverbank. "
    enc = lambda t: len(call("/v1/token/encode", {"text": t, "add_bos_token": False})["tokens"])
    tpu = enc(unit.format("s0-000000") * 200) / 200
    texts = []
    for i in range(slots):
        tag = f"s{i}-{time.time_ns() % 1000000:06d}"; n = int(per / tpu); t = unit.format(tag) * n
        while enc(t) > per: n -= max(1, n // 100); t = unit.format(tag) * n
        texts.append(t)
    total = smi("memory.total"); peak = [smi("memory.used")]; stop = threading.Event(); samples = [0]
    def sampler():
        while not stop.is_set(): peak[0] = max(peak[0], smi("memory.used")); samples[0] += 1; time.sleep(0.5)
    res = [None] * slots
    def one(i):
        t0 = time.time()
        try:
            r = call("/v1/completions", {"prompt": texts[i], "max_tokens": 16, "temperature": 0})
            res[i] = {"prompt_tokens": r.get("usage", {}).get("prompt_tokens"), "wall_s": round(time.time() - t0, 1), "slot": i}
        except Exception as e: res[i] = {"error": repr(e)[:200], "wall_s": round(time.time() - t0, 1), "slot": i}
    th = threading.Thread(target=sampler); th.start()
    ws = [threading.Thread(target=one, args=(i,)) for i in range(slots)]; [w.start() for w in ws]; [w.join() for w in ws]
    time.sleep(2); stop.set(); th.join()
    free = total - peak[0]; capped = bool(a.cap_per_request or a.slots)
    out = {"engine": "tabbyapi", "n_ctx": pool, "np": bs, "max_seq_len": maxseq, "per_slot_target": per, "requests": res, "concurrent": slots,
           "vram_total_mib": total, "vram_peak_mib": peak[0], "vram_after_mib": smi("memory.used"), "vram_free_at_peak_mib": free,
           "samples": samples[0], "bar_mib": a.bar_mib, "passes_bar": free >= a.bar_mib and not any("error" in (r or {}) for r in res)}
    if capped: out["capped_dry_run"] = True; out["passes_bar"] = None
    os.makedirs(os.path.dirname(os.path.abspath(a.out)), exist_ok=True); json.dump(out, open(a.out, "w"), indent=1); print(json.dumps({k: v for k, v in out.items() if k != "requests"}))
    return 0

if __name__ == "__main__":
    sys.exit(main())
