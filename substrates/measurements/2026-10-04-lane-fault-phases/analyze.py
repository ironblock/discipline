#!/usr/bin/env python3
"""Per-phase medians and p90s from phases.py's JSONL (#353).

    analyze.py PHASES.jsonl     prints a markdown table per kind (lane, test)

The first row is the warm-up and is dropped. Phases, in seconds of wall clock:
  copy        the sandbox copy, git init, git add (sandbox())
  fingerprint the two sandbox_state()s and the diff-tree between them
  inject      the injection
  build       cargo's own "Finished ... in" summed over the check's invocations
              (compile and link of the mutated crate and its test targets)
    link        of which: the linker's wall clock, summed (links run in parallel,
                so this can exceed its share of build)
  test run    each invocation's wall clock minus its Finished time: the test
              binaries running
  other       the check's wall clock minus every cargo invocation's: verify.sh,
              hermetic.sh and the lane runner's own Python
  verdict     the signature match
A lane fault's check runs every lane's `cargo test`; `other lanes` is what a
per-lane scope would remove: the invocations for the lanes the fault is not in,
less the build, which whichever invocation runs first pays and one would still.
"""
import json, statistics, sys

rows = [json.loads(l) for l in open(sys.argv[1])][1:]

def q(xs, p):
    xs = sorted(xs)
    return xs[min(len(xs) - 1, int(round(p * (len(xs) - 1))))]

def phases(r):
    cargo = r["cargo"]
    build = sum(c["finished"] or 0 for c in cargo)
    run = sum(c["wall"] - (c["finished"] or 0) for c in cargo)
    d = {
        "copy": r["copy"],
        "fingerprint": r["state_before"] + r["state_after"],
        "inject": r["inject"],
        "build": build,
        "link (within build)": sum(r["link"]),
        "test run": run,
        "other": r["check"] - sum(c["wall"] for c in cargo),
        "verdict": r["verdict"],
    }
    d["total"] = d["copy"] + d["fingerprint"] + d["inject"] + r["check"] + d["verdict"]
    if r["lane"]:
        lane = r["lane"]
        mine = [c for c in cargo if f"-- {lane}" in c["args"]]
        others = [c for c in cargo if c not in mine]
        removable = sum(c["wall"] for c in others)
        if cargo and cargo[0] in others:
            removable -= cargo[0]["finished"] or 0
        d["other lanes (removable)"] = removable
        d["own lane's test run"] = sum(c["wall"] - (c["finished"] or 0) for c in mine)
    return d

for kind in ("lane", "test"):
    sel = [phases(r) for r in rows if (r["lane"] is not None) == (kind == "lane")]
    if not sel:
        continue
    keys = list(sel[0])
    total_med = statistics.median(s["total"] for s in sel)
    print(f"\n**{kind} faults, n = {len(sel)}** (all verdicts read: {all(r['verdict_ok'] for r in rows if (r['lane'] is not None) == (kind == 'lane'))})\n")
    print("| phase | median s | p90 s | median share of the total |")
    print("|---|---:|---:|---:|")
    for k in keys:
        xs = [s[k] for s in sel]
        med = statistics.median(xs)
        print(f"| {k} | {med:.1f} | {q(xs, 0.9):.1f} | {'' if k == 'total' else f'{100 * med / total_med:.0f}%'} |")
    if kind == "lane":
        frac = [s["other lanes (removable)"] / s["total"] for s in sel]
        print(f"\nper fault, the other lanes' share of its total: median {100 * statistics.median(frac):.0f}%, "
              f"min {100 * min(frac):.0f}%, max {100 * max(frac):.0f}%")
