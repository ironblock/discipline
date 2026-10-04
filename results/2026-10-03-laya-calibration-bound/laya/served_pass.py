#!/usr/bin/env python3
"""#165's served laya pass (amendments 4 and 6): every request in requests.jsonl to sidekickd's /v1/classify, one at
a time, with no `calibration` field (T = 1). Per response: the label probabilities as returned, the sidekick-*
provenance headers (compute units, buckets, model, version), usage.prompt_tokens, and the time. /v1/models is read
at the start and at the end and kept whole, so placement (with `stale`) is reconciled against every response.
Refuses to start unless the model's placement is present and stale is false.
Usage: served_pass.py BASE_URL PATH TAG REQUESTS OUT_DIR   (e.g. http://127.0.0.1:PORT, the path tag cpu_and_ne)"""
import json, sys, time, pathlib, urllib.request, hashlib
BASE, TAG, REQ, OUT = sys.argv[1].rstrip("/"), sys.argv[2], pathlib.Path(sys.argv[3]), pathlib.Path(sys.argv[4])
OUT.mkdir(parents=True, exist_ok=True)
def get(path):
    with urllib.request.urlopen(BASE + path, timeout=60) as r: return json.load(r)
def post(path, body):
    req = urllib.request.Request(BASE + path, data=json.dumps(body).encode(), headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=600) as r:
        return json.load(r), {k.lower(): v for k, v in r.headers.items() if k.lower().startswith("sidekick-")}
utc = lambda: time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
BUCKETS = ("128", "256", "512", "1024")
def placement_problems(models):
    """sidekickd v0.7.0's shape (the Sidekick orchestrator, 2026-10-03): data[] -> the model -> placement, keyed by
    bucket; each {source: conversion|live, state, ane, gpu, cpu}; a `stale` key appears only when stale; a bucket with
    no record is absent; a failed live read is state: error."""
    e = next((m for m in models.get("data", []) if m.get("id") == "laya-typed-decisions"), None)
    if e is None: return ["laya-typed-decisions is not loaded"], None
    pl = e.get("placement") or {}
    probs = [f"bucket {b} absent" for b in BUCKETS if b not in pl]
    for b, v in pl.items():
        if "stale" in v: probs.append(f"bucket {b} stale: {v['stale']}")
        if v.get("state") != "ready": probs.append(f"bucket {b} state {v.get('state')!r}")
    return probs, e
# Sidekick warms each bucket before handing over; wait up to 5 minutes for every bucket ready, then refuse
deadline = time.time() + 300
while True:
    start = get("/v1/models"); probs, entry = placement_problems(start)
    if not probs or time.time() > deadline: break
    print("waiting:", probs, flush=True); time.sleep(10)
(OUT / f"models-start-{TAG}.json").write_text(json.dumps(start, indent=1))
(OUT / f"health-start-{TAG}.json").write_text(json.dumps(get("/health"), indent=1))
assert not probs, f"refusing to start: {probs}"
assert entry.get("compute_units") == TAG, f"/v1/models says compute_units {entry.get('compute_units')!r}, this pass is {TAG}"
reqs = [json.loads(l) for l in REQ.read_text().splitlines() if l.strip()]
with (OUT / f"responses-{TAG}.jsonl").open("w") as f:
    for i, r in enumerate(reqs):
        t0 = time.time(); body, hdr = post("/v1/classify", r["request"])
        if hdr.get("sidekick-compute-units") != TAG: print("UNITS MISMATCH", i, hdr, flush=True)
        f.write(json.dumps({"item": r["item"], "field": r["field"], "path": TAG, "at": utc(), "ms": round(1000 * (time.time() - t0), 1),
                            "headers": hdr, "response": body}) + "\n")
        if i % 100 == 0: print(TAG, i, "/", len(reqs), flush=True)
end = get("/v1/models"); (OUT / f"models-end-{TAG}.json").write_text(json.dumps(end, indent=1))
endprobs, _ = placement_problems(end)
if endprobs: print("PLACEMENT AT END:", endprobs, flush=True)
(OUT / f"health-end-{TAG}.json").write_text(json.dumps(get("/health"), indent=1))
print(TAG, "done:", len(reqs), "responses; requests sha256", hashlib.sha256(REQ.read_bytes()).hexdigest()[:16])
