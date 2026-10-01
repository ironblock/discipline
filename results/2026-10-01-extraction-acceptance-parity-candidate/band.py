#!/usr/bin/env python3
"""The parity band for extraction-acceptance-inverts, derived from the committed
tallies of that row. A pure function of four committed artefacts, each pinned
here by sha256 and refused on any other bytes: the row's grade.py, its
report.json, and seat A's and seat B's events.jsonl.

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

Usage: python3 band.py ROW_DIR
  ROW_DIR holds grade.py, report.json, seat-a/events.jsonl, seat-b/events.jsonl.
Prints one JSON object. Exit 0 printed; 2 an input is missing, is not the
pinned bytes, or does not reproduce the row's own tallies.
"""
import hashlib, importlib.util, json, pathlib, random, sys

sys.dont_write_bytecode = True

PINNED = {
    "grade.py": "01765abb034eee57db7d573fb5414d6e394322afdae99008b9ff344523812778",
    "report.json": "f99a8c5c92ee619088a06313dc27bf95daeee0ed28af817a7d68d2b4ec48a083",
    "seat-a/events.jsonl": "1b1fdd414fb63f9da75b2a15ccc7f13131c519c731e3f3b4e70394b75c55d749",
    "seat-b/events.jsonl": "030813139cf9bd0065f003816a55b8bacbb46ec6c22b40e11340a53793867e36",
}
LEVEL = 0.95
RESAMPLES = 10000
SEED = 20260810


def fail(m):
    print(f"band: {m}", file=sys.stderr)
    raise SystemExit(2)


if len(sys.argv) != 2:
    fail("usage: band.py ROW_DIR")
row = pathlib.Path(sys.argv[1])
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

stated = json.loads((row / "report.json").read_text(encoding="utf-8"))["quality"]["tallies"]
for name, t in (("A", A), ("B", B)):
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
