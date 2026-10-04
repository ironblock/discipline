#!/usr/bin/env python3
"""The sample phases.py measures (#353), chosen by rule, not by hand: for each
lane, its first and middle fault by sorted id; twelve `test` cases spaced
evenly over their sorted ids; and a warm-up first (the first lane fault again),
whose row primes the shared target and is not counted.

    sample.py ROOT    prints one phases.py argument per line
"""
import pathlib, subprocess, sys
root = pathlib.Path(sys.argv[1])
sys.path.insert(0, str(root / "scripts"))
import gatelib  # noqa: E402
lanes: dict[str, list[str]] = {}
for line in subprocess.run(["python3", "scripts/apply-lane-faults.py", "--list"], cwd=root,
                           capture_output=True, text=True, check=True).stdout.splitlines():
    lane, ident = line.split("\t")[:2]
    lanes.setdefault(lane, []).append(ident)
picked = []
for lane, ids in sorted(lanes.items()):
    ids.sort()
    picked += [f"lane:{lane}:{ids[0]}", f"lane:{lane}:{ids[len(ids) // 2]}"]
tests = sorted(f"{c.check}.{c.injection[len('inject_'):]}"
               for c in gatelib.seeded_cases((root / "verify.sh").read_text()) if c.check == "test")
step = len(tests) / 12
picked += [f"case:{tests[int(i * step)]}" for i in range(12)]
print("\n".join([picked[0]] + picked))
