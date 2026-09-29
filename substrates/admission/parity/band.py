#!/usr/bin/env python3
"""The parity band for extraction-acceptance-inverts, generalised (#143, I5; the plan's D7 and N10): derived
from the committed tallies of a fire's row. A pure function of the artefacts a pinned MANIFEST names, each
pinned by sha256 and refused on any other bytes: the row's grade.py, its report.json when the row has one,
and seat A's and seat B's events.jsonl. The manifest also carries the level, the resamples and the seed.
The method is #115's band.py unchanged; with #115's manifest (configs/extraction-acceptance-inverts-115.json)
it reproduces #115's band.json byte for byte.

Per fork, offered and accepted are read through the row's own grade.py
(load_seat, raw_facts) and capped as its quality() caps them -- accepted is
min(accepted, offered) -- so the per-fork tallies sum to the row's deduped
totals, which is checked.

The effect is the deduped gate-accept rate of seat B minus seat A, each pooled
over the extraction forks (sum accepted / sum offered) from the integer
tallies, in IEEE double -- never the three-place rates the report prints. The
forks pair across seats by (turn, step). The interval is a paired percentile
bootstrap: each resample draws fork keys with replacement, by index from
rng.random(), and takes both seats' tallies for each drawn key.

Usage: python3 band.py MANIFEST ROW_DIR
  MANIFEST is JSON: {"band": {"artifacts": {path: sha256, ...}, "level": ..., "resamples": ..., "seed": ...}};
  ROW_DIR holds the artefacts it names (grade.py, seat-a/events.jsonl and seat-b/events.jsonl at least).
Prints one JSON object. Exit 0 printed; 2 an input is missing, is not the
pinned bytes, or does not reproduce the row's own tallies.
"""
import hashlib, importlib.util, json, pathlib, random, sys

sys.dont_write_bytecode = True

def fail(m):
    print(f"band: {m}", file=sys.stderr)
    raise SystemExit(2)


if len(sys.argv) != 3:
    fail("usage: band.py MANIFEST ROW_DIR")
try:
    man = json.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"))["band"]
    PINNED, LEVEL, RESAMPLES, SEED = dict(man["artifacts"]), float(man["level"]), int(man["resamples"]), int(man["seed"])
except (OSError, ValueError, KeyError, TypeError) as e:
    fail(f"the manifest cannot be read: {type(e).__name__}")
for need in ("grade.py", "seat-a/events.jsonl", "seat-b/events.jsonl"):
    if need not in PINNED:
        fail(f"the manifest does not pin {need}")
if not (0 < LEVEL < 1 and RESAMPLES > 0):
    fail("the manifest's level or resamples is out of range")
row = pathlib.Path(sys.argv[2])
for rel, want in PINNED.items():
    p = row / rel
    if not p.is_file():
        fail(f"{rel} is not in the row directory")
    got = hashlib.sha256(p.read_bytes()).hexdigest()
    if got != want:
        fail(f"{rel} hashes to {got}, not the pinned {want}")

spec = importlib.util.spec_from_file_location("grade", row / "grade.py")
grade = importlib.util.module_from_spec(spec)
spec.loader.exec_module(grade)


def tallies(seat):
    s = grade.load_seat(str(row / seat))
    out = {}
    for f in s["forks"]:
        if f["lane"] != "extraction":
            continue
        key = (f["turn"], f["step"])
        if not all(isinstance(x, int) for x in key):
            fail(f"{seat}: an extraction fork has no integer (turn, step): {key}")
        if key in out:
            fail(f"{seat}: two extraction forks at {key}")
        offered = len(grade.raw_facts(f["content"]))
        out[key] = (offered, min(len(s["accepted"].get(f["id"], [])), offered))
    return out


A, B = tallies("seat-a"), tallies("seat-b")
if set(A) != set(B):
    fail("the seats' extraction forks do not pair by (turn, step)")
keys = sorted(A)

stated = json.loads((row / "report.json").read_text(encoding="utf-8"))["quality"]["tallies"] if "report.json" in PINNED else {}
if "report.json" in PINNED and not all(n in stated for n in ("A", "B")):
    fail("the pinned report.json does not state both seats' tallies")
for name, t in ((n, t) for n, t in (("A", A), ("B", B)) if n in stated):
    off, acc = sum(v[0] for v in t.values()), sum(v[1] for v in t.values())
    want = (stated[name]["facts_offered"], stated[name]["facts_accepted_deduped"])
    if (off, acc) != want:
        fail(f"seat {name}: the per-fork tallies sum to {off}/{acc}, the report states {want[0]}/{want[1]}")


def effect(ks):
    oa = sum(A[k][0] for k in ks)
    aa = sum(A[k][1] for k in ks)
    ob = sum(B[k][0] for k in ks)
    ab = sum(B[k][1] for k in ks)
    if oa == 0 or ob == 0:
        return None
    return ab / ob - aa / oa


n = len(keys)
rng = random.Random(SEED)
draws, undefined = [], 0
for _ in range(RESAMPLES):
    d = effect([keys[int(rng.random() * n)] for _ in range(n)])
    if d is None:
        undefined += 1
    else:
        draws.append(d)
draws.sort()
lo = draws[int((1 - LEVEL) / 2 * len(draws))]
hi = draws[int((1 + LEVEL) / 2 * len(draws)) - 1]
print(json.dumps({
    "effect": "deduped gate-accept rate, seat B minus seat A, pooled over the extraction forks from the integer tallies",
    "forks": n,
    "seat_a": {"offered": sum(v[0] for v in A.values()), "accepted_deduped": sum(v[1] for v in A.values())},
    "seat_b": {"offered": sum(v[0] for v in B.values()), "accepted_deduped": sum(v[1] for v in B.values())},
    "point": round(effect(keys), 6),
    "level": LEVEL, "resamples": RESAMPLES, "seed": SEED, "resamples_undefined": undefined,
    "band": [round(lo, 6), round(hi, 6)],
    "reads": PINNED,
}, indent=1))
