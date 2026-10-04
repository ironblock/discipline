#!/usr/bin/env python3
"""#165 re-judge: the 36 batch files from layout.json, as the archive's batches are shaped ({"batch", "items"}:
each item id plus the fields the judge prompt reads). Item ids are fresh opaque 12-hex per batch item and item order
within a batch is shuffled, both from seed + 13 (they carry no information; the layout fixes membership). Writes
judge/batches/batch-NN.json, judge/ids.json (opaque id -> state sha256, control flag), judge/key.json (each batch's
controls' ruled answers, as the archive keys them). Usage: build_batches.py REPO_ROOT OUT_DIR"""
import hashlib, importlib.util, json, pathlib, random, sys
ROOT, OUT = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
RJ = pathlib.Path(__file__).resolve().parent / "out"
plan = json.loads((RJ / "plan.json").read_text()); layout = json.loads((RJ / "layout.json").read_text())
spec = importlib.util.spec_from_file_location("m", ROOT / "results/2026-10-02-judge-state-lengths/measure.py"); m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m)
SONNET = ["2026-09-21-false-nomination-framing-v2", "2026-09-27-false-nomination-edit-rate", "2026-09-29-false-nomination-edit-rate-second-substrate"]
item_of, answer_of = {}, {}
for rec in SONNET:
    d = ROOT / "results" / rec / "judge"; key = json.loads((d / "key.json").read_text())
    for bf in sorted((d / "batches").glob("batch-*.json")):
        n = str(int(bf.stem.split("-")[1]))
        for it in json.loads(bf.read_text())["items"]:
            s = m.sha(m.state_of(it).encode())
            body = {k: v for k, v in it.items() if k != "id"}
            item_of.setdefault(s, body)
            if it["id"] in key.get(n, {}): answer_of.setdefault(s, key[n][it["id"]])
rng = random.Random(plan["seed"] + 13)
ids, keys, nn = {}, {}, 0
(OUT / "batches").mkdir(parents=True, exist_ok=True)
for p, batches in enumerate(layout):
    for b in batches:
        nn += 1; tag = f"{nn:02d}"; members = [(s, False) for s in b["sample"]] + [(s, True) for s in b["controls"]]
        rng.shuffle(members); items = []; keys[str(nn)] = {}
        for s, ctl in members:
            oid = "%012x" % rng.getrandbits(48)
            while oid in ids: oid = "%012x" % rng.getrandbits(48)
            ids[oid] = {"state_sha256": s, "control": ctl, "pass": p + 1, "batch": tag}
            items.append({"id": oid, **item_of[s]})
            if ctl: keys[str(nn)][oid] = answer_of[s]
        (OUT / "batches" / f"batch-{tag}.json").write_text(json.dumps({"batch": nn, "items": items}, indent=1, ensure_ascii=False) + "\n")
(OUT / "ids.json").write_text(json.dumps(ids, indent=1) + "\n"); (OUT / "key.json").write_text(json.dumps(keys, indent=1) + "\n")
print(nn, "batches;", len(ids), "items;", sum(len(v) for v in keys.values()), "keyed controls")
