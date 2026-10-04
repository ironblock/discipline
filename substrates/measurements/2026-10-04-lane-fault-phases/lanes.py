#!/usr/bin/env python3
"""Each lane's own test run, measured inside every sampled lane fault (#353),
and what a per-lane scope would remove, weighted by how many faults each lane
declares (`apply-lane-faults.py --list` on ROOT).

    lanes.py PHASES.jsonl ROOT
"""
import json, re, statistics, subprocess, sys
rows = [json.loads(l) for l in open(sys.argv[1])][1:]
per: dict[str, list[float]] = {}
for r in rows:
    if r["lane"]:
        for c in r["cargo"]:
            per.setdefault(re.search(r"-- (\w+)", c["args"]).group(1), []).append(c["wall"] - (c["finished"] or 0))
med = {l: statistics.median(x) for l, x in per.items()}
counts: dict[str, int] = {}
for line in subprocess.run(["python3", "scripts/apply-lane-faults.py", "--list"], cwd=sys.argv[2],
                           capture_output=True, text=True, check=True).stdout.splitlines():
    counts[line.split("\t")[0]] = counts.get(line.split("\t")[0], 0) + 1
print("| lane | faults | its test run, median s (n) |\n|---|---:|---:|")
for l in sorted(med, key=lambda l: -med[l]):
    print(f"| {l} | {counts.get(l, 0)} | {med[l]:.1f} ({len(per[l])}) |")
whole = sum(med.values())
saved = sum(n * (whole - med[l]) for l, n in counts.items())
total = sum(counts.values())
print(f"\nall six lanes' test runs, sum of medians: {whole:.1f} s; a per-lane scope would remove "
      f"~{saved:.0f} s over the {total} lane faults, {saved / total:.1f} s per fault (estimated from these medians)")
