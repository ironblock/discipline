"""The falsification's analysis, fixed in advance (#971; PREREG.md and Amendment 1, 2026-10-29):
python3 -I analyze.py ROWS.jsonl, or --selftest.

- Per arm, across all 120 passes (a void or no-answer pass has a cost, Amendment 1): executed-right count, and the
  medians of tool calls and seconds (primary) and of completion tokens, searches, docs reads and output tokens.
- Paired per-(task, seed) differences, T-N and S-N, of tool calls and seconds: their median, with a bootstrap 95%
  interval over the 120 pairs (10,000 resamples of the pairs, `random.Random(29)`).
- A per-task table: each arm's executed-right count and median tool calls and seconds.
- The decision (T against N alone; S is reported, never decided on): the notes WIN only if
  1. T's executed-right count >= N's - 1, and
  2. T's median tool calls OR median seconds is at least 25% below N's.
  Otherwise the tutorial is PARKED.
"""
import json, random, statistics, sys

PRIMARY = ("tool_calls", "seconds")
SECONDARY = ("completion_tokens", "searches", "docs_reads", "output_tokens")
RESAMPLES, BOOT_SEED = 10000, 29


def value(row, key):
    """Every pass has a cost (Amendment 1): a row without one is an error, never a free pass."""
    v = row.get(key)
    if not isinstance(v, (int, float)):
        raise ValueError(f"analyze: {row.get('task')} {row.get('seed')} {row.get('arm')} has no {key}")
    return v


def summary(rows):
    by_arm = {}
    for r in rows:
        by_arm.setdefault(r["arm"], []).append(r)
    return {arm: {"n": len(rs), "executed_right": sum(bool(r.get("executed_right")) for r in rs),
                  "void": sum(bool(r.get("void")) for r in rs),
                  **{f"median_{k}": statistics.median([value(r, k) for r in rs]) for k in PRIMARY + SECONDARY}}
            for arm, rs in sorted(by_arm.items())}


def paired(rows, a, b, key):
    """The (task, seed) pairs' differences a-b of KEY: their median and its bootstrap 95% interval."""
    cell = {(r["task"], r["seed"], r["arm"]): r for r in rows}
    keys = sorted({(t, s) for t, s, arm in cell if arm == a and (t, s, b) in cell})
    diffs = [value(cell[(t, s, a)], key) - value(cell[(t, s, b)], key) for t, s in keys]
    if not diffs:
        return {"pairs": 0}
    rng = random.Random(BOOT_SEED)
    boots = sorted(statistics.median(rng.choices(diffs, k=len(diffs))) for _ in range(RESAMPLES))
    return {"pairs": len(diffs), "median_diff": statistics.median(diffs),
            "ci95": [boots[int(0.025 * RESAMPLES)], boots[int(0.975 * RESAMPLES) - 1]]}


def per_task(rows):
    out = {}
    for r in rows:
        out.setdefault(r["task"], {}).setdefault(r["arm"], []).append(r)
    return {task: {arm: {"right": sum(bool(r.get("executed_right")) for r in rs),
                         "median_tool_calls": statistics.median([value(r, "tool_calls") for r in rs]),
                         "median_seconds": statistics.median([value(r, "seconds") for r in rs])}
                   for arm, rs in sorted(arms.items())} for task, arms in sorted(out.items())}


def decide(s):
    """Rules 1 and 2, T against N."""
    n, t = s["N"], s["T"]
    correct = t["executed_right"] >= n["executed_right"] - 1
    cuts = {k: (n[f"median_{k}"] - t[f"median_{k}"]) / n[f"median_{k}"] if n[f"median_{k}"] else 0.0 for k in PRIMARY}
    cheaper = any(c >= 0.25 for c in cuts.values())
    return {"rule1_correctness": correct, "rule2_cost": cheaper, "cuts": cuts, "verdict": "win" if correct and cheaper else "parked"}


def analyze(rows):
    s = summary(rows)
    return {"summary": s, "paired": {f"{a}-N": {k: paired(rows, a, "N", k) for k in PRIMARY} for a in ("T", "S")},
            "per_task": per_task(rows), "decision": decide(s)}


def synthetic(t_cost=1.0, t_wrong=0, n_wrong=0, void_t=0):
    """120 passes per arm: N costs 10 calls and 100 s each; T costs t_cost times that."""
    rows = []
    for i in range(120):
        task, seed = f"t{i % 12}", 101 + i // 12
        rest = {k: 0 for k in SECONDARY}
        rows.append({"task": task, "seed": seed, "arm": "N", "executed_right": i >= n_wrong, "tool_calls": 10, "seconds": 100, **rest})
        rows.append({"task": task, "seed": seed, "arm": "S", "executed_right": True, "tool_calls": 9, "seconds": 90, **rest})
        void = i < void_t  # a void pass spent its full stop: 3600 s
        rows.append({"task": task, "seed": seed, "arm": "T", "executed_right": i >= t_wrong and not void, "void": void,
                     "tool_calls": 40 if void else 10 * t_cost, "seconds": 3600 if void else 100 * t_cost, **rest})
    return rows


def selftest():
    bad = 0
    cases = [("T 30% cheaper, as right: win", synthetic(0.7), "win"),
             ("T 30% cheaper, one fewer right: win (rule 1 allows one)", synthetic(0.7, t_wrong=1), "win"),
             ("T 30% cheaper, two fewer right: parked", synthetic(0.7, t_wrong=2), "parked"),
             ("T 20% cheaper: parked", synthetic(0.8), "parked"),
             ("T exactly 25% cheaper: win", synthetic(0.75), "win"),
             ("T costlier: parked", synthetic(1.2), "parked"),
             ("T's voids are not right: parked", synthetic(0.7, void_t=2), "parked")]
    for why, rows, want in cases:
        got = analyze(rows)["decision"]["verdict"]
        if got != want:
            print(f"analyze: {why}: {got}"); bad += 1
    a = analyze(synthetic(0.7))
    p = a["paired"]["T-N"]["tool_calls"]
    if p["pairs"] != 120 or p["median_diff"] != -3.0 or not (p["ci95"][0] <= -3.0 <= p["ci95"][1]):
        print(f"analyze: paired T-N tool calls {p}"); bad += 1
    if a["summary"]["T"]["n"] != 120 or len(a["per_task"]) != 12:
        print("analyze: not 120 per arm, or not 12 tasks"); bad += 1
    costless = synthetic(0.7)
    del costless[2]["seconds"]
    try:
        analyze(costless)
        print("analyze: a row with no cost was read as free"); bad += 1
    except ValueError:
        pass
    if analyze(synthetic(0.7))["paired"] != analyze(synthetic(0.7))["paired"]:
        print("analyze: the bootstrap is not reproducible at seed 29"); bad += 1
    print(f"analyze: {len(cases)} decisions, the paired interval and the tables, {bad} wrong")
    return 1 if bad else 0


if __name__ == "__main__":
    if sys.argv[1:] == ["--selftest"]:
        sys.exit(selftest())
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    print(json.dumps(analyze([json.loads(l) for l in open(sys.argv[1]) if l.strip()]), indent=1))
