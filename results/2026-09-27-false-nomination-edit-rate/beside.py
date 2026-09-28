#!/usr/bin/env python3
"""The figures decision-rule.toml's [readings].beside names, reported beside the
word and read by nothing: per rung, the per-session cost (edit rate x (1 - p))
for both framings, and the acknowledgement grade -- how many counted
fork-rungs name the entry, and the judge's verdict split among them, per arm.
Reads stage2.json (the counted rates) and the committed grades and verdicts;
the applier is not touched and nothing here reaches the word.
Usage: python3 beside.py  (from this directory)  -> prints beside.json"""
import collections, json, pathlib
from fractions import Fraction as F
here = pathlib.Path(".")
s2 = json.loads((here / "stage2.json").read_text())
ids = json.loads((here / "judge/ids.json").read_text())
verdict = {}
for p in sorted((here / "judge").glob("verdicts-*.json")):
    for v in json.loads(p.read_text()):
        m = ids.get(v["id"])
        if m and "arm" in m:
            verdict[(m["rung"], m["fork"], m["arm"])] = v.get("verdict")
grades = [json.loads(l) for l in (here / "grades.jsonl").read_text().splitlines() if l.strip()]
out = {}
for rung, s in s2["rungs"].items():
    if not s.get("counted"):
        continue
    p = F(rung)
    cost = {a: str(F(s["rate"][a]) * (1 - p)) for a in ("imperative", "advisory")}
    ack = {}
    for arm in ("imperative", "advisory", "sham"):
        named = [g for g in grades if g["rung"] == rung and g["arm"] == arm and not g["true"] and g.get("names")]
        split = collections.Counter(verdict.get((rung, g["fork"], arm)) or "unjudged" for g in named)
        ack[arm] = {"named": len(named), "verdicts": dict(sorted(split.items()))}
    out[rung] = {"per_session_cost": cost, "acknowledgement": ack}
print(json.dumps(out, indent=1, sort_keys=True))
