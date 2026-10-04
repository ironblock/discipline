#!/usr/bin/env python3
"""#165's re-judge sample: a seeded, proportionally stratified draw of single-judged real items from the
three Sonnet records, and the pass layout (36 sample items + 4 of the 14 controls per batch, three passes, #165 amendment 3).
Usage: draw.py REPO_ROOT OUT_DIR. Deterministic: the same tree and seed give the same bytes."""
import collections, hashlib, importlib.util, json, math, pathlib, random, sys
ROOT, OUT = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2]); OUT.mkdir(parents=True, exist_ok=True)
SEED_PHRASE = "discipline #165 re-judge sample, 2026-10-02"
SEED = int(hashlib.sha256(SEED_PHRASE.encode()).hexdigest()[:16], 16)
N, PER_BATCH, CONTROLS_PER_BATCH, PASSES = 400, 36, 4, 3
SONNET = ["2026-09-21-false-nomination-framing-v2", "2026-09-27-false-nomination-edit-rate", "2026-09-29-false-nomination-edit-rate-second-substrate"]
spec = importlib.util.spec_from_file_location("m", ROOT / "results/2026-10-02-judge-state-lengths/measure.py"); m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m)
judged = collections.defaultdict(list)   # state sha -> [(record, batch, id, verdict, edit, is_control, item)]
for rec in SONNET:
    d = ROOT / "results" / rec / "judge"; key = json.loads((d / "key.json").read_text())
    for bf in sorted((d / "batches").glob("batch-*.json")):
        n = bf.stem.split("-")[1]; items = {it["id"]: it for it in json.loads(bf.read_text())["items"]}
        vf = d / f"verdicts-{n}.json"
        if not vf.exists(): continue
        ctl = set(key.get(str(int(n)), {}))
        for v in json.loads(vf.read_text()):
            it = items.get(v["id"])
            if it is None: continue
            judged[m.sha(m.state_of(it).encode())].append((rec, n, v["id"], v.get("verdict"), v.get("edit"), v["id"] in ctl, it))
pop = {s: js[0] for s, js in judged.items() if len(js) == 1 and not js[0][5]}
strata = collections.defaultdict(list)
for s, j in pop.items(): strata[(j[0], j[3])].append(s)
total = len(pop)
# largest-remainder proportional allocation
quota = {k: N * len(v) / total for k, v in strata.items()}
alloc = {k: math.floor(q) for k, q in quota.items()}
for k in sorted(quota, key=lambda k: (-(quota[k] - alloc[k]), k))[: N - sum(alloc.values())]: alloc[k] += 1
rng = random.Random(SEED)
sample = []
for k in sorted(strata):
    sample += rng.sample(sorted(strata[k]), alloc[k])
# controls: the 14 controls' states, as judged in the records (identical text in every record)
controls = {}
for s, js in judged.items():
    if js[0][5]: controls[s] = js[0][6]
assert len(controls) == 14, len(controls)
ctl_order = sorted(controls)
passes = []
for p in range(PASSES):
    order = sample[:]; random.Random(SEED + 1 + p).shuffle(order)
    nb = math.ceil(len(order) / PER_BATCH); batches = []
    q, r = divmod(len(order), nb); bounds = [0]
    for b in range(nb): bounds.append(bounds[-1] + q + (1 if b < r else 0))
    for b in range(nb):
        chunk = order[bounds[b]:bounds[b + 1]]
        ctl = [ctl_order[(p * nb * CONTROLS_PER_BATCH + b * CONTROLS_PER_BATCH + i) % 14] for i in range(CONTROLS_PER_BATCH)]
        batches.append({"sample": chunk, "controls": ctl})
    passes.append(batches)
plan = {"seed_phrase": SEED_PHRASE, "seed": SEED, "population": total, "sample_size": len(sample),
        "strata": {f"{k[0]} | {k[1]}": {"population": len(strata[k]), "drawn": alloc[k]} for k in sorted(strata)},
        "passes": PASSES, "batches_per_pass": [len(p) for p in passes], "items_per_batch": "the sample split evenly over ceil(400/36) = 12 batches (34 or 33 items) + 4 controls",
        "excluded": {"states judged more than once": sum(1 for js in judged.values() if len(js) > 1 and not js[0][5]), "controls": 14}}
(OUT / "plan.json").write_text(json.dumps(plan, indent=1) + "\n")
(OUT / "sample.json").write_text(json.dumps([{"state_sha256": s, "record": pop[s][0], "batch": pop[s][1], "id": pop[s][2], "verdict": pop[s][3], "edit": pop[s][4]} for s in sample], indent=1) + "\n")
(OUT / "layout.json").write_text(json.dumps(passes, indent=1) + "\n")
print(json.dumps(plan, indent=1))
