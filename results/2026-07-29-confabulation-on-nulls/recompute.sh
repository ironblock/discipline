#!/usr/bin/env bash
# Gate 0 for this directory, in two steps: every artifact the record's claims
# consume must hash to the digest the record names, and the committed
# instrument over the committed artifact must reproduce the committed
# headline, field by field. Exit 0 when both hold, 1 when either fails, 2 when
# the check cannot run.
set -uo pipefail
here="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
tmp="$(mktemp -d)" || exit 2
trap 'rm -rf "$tmp"' EXIT
python3 - "$here" <<'PY' || exit 1
import hashlib, json, pathlib, sys
here = pathlib.Path(sys.argv[1])
seen = {}
for line in (here / "run.jsonl").read_text().splitlines():
    row = json.loads(line)
    for artifact in row.get("consumes") or []:
        seen[artifact["path"]] = artifact["sha256"]
if not seen:
    print("recompute: the record names no consumed artifact"); sys.exit(1)
bad = 0
for path, want in sorted(seen.items()):
    got = hashlib.sha256((here / path).read_bytes()).hexdigest()
    if got != want:
        print(f"recompute: {path} hashes to {got[:12]}..., the record says {want[:12]}..."); bad += 1
if bad: sys.exit(1)
print(f"recompute: {len(seen)} consumed artifact(s) hash as the record says")
PY
python3 "$here/recompute_headline.py" --out "$tmp" >/dev/null || { echo "recompute: the instrument failed"; exit 2; }
python3 - "$tmp/headline.json" "$here/headline.json" <<'PY' || exit 1
import json, sys
got, want = (json.load(open(p)) for p in sys.argv[1:3])
def flat(o, p=""):
    if isinstance(o, dict):
        for k, v in o.items(): yield from flat(v, f"{p}{k}.")
    elif isinstance(o, list): yield p[:-1], json.dumps(o)
    else: yield p[:-1], o
g, w = dict(flat(got)), dict(flat(want))
diff = [k for k in sorted(set(g) | set(w)) if g.get(k) != w.get(k)]
if diff:
    for k in diff[:6]: print(f"recompute: {k}: committed={w.get(k)!r} recomputed={g.get(k)!r}")
    print(f"recompute: {len(diff)} of {len(w)} fields DIVERGE"); sys.exit(1)
print(f"recompute: {len(w)}/{len(w)} fields reproduce")
PY
python3 - "$here" "headline.json" <<'PY' || exit 1
import hashlib, json, pathlib, re, sys, tomllib

# Third step: every number the README's FRONT MATTER states, re-derived from the
# artefacts, and every number the artefacts derive, stated.
#
# FRONT MATTER, AND NOT THE PROSE. This step reads the `+++` block and nothing
# else, so the figures in the body -- which are the ones a reader actually takes
# away -- are bound to the product by nobody. A review demonstrated it: altering
# a headline rate in the prose leaves every gate green. Disclosed in the
# directory's `known_defects` rather than papered over, because closing it needs
# a declaration this schema does not have yet -- the directory saying which
# product fields its prose cites -- and that is a ruling, not a patch.
#
# The first two steps prove the consumed artefacts are the ones the record
# names and that the instrument reproduces the product. Neither reads the
# README, so a stated figure could drift from the record it summarises and both
# would still pass. `check-recompute.py` probes for exactly that.
#
# THE FIRST VERSION OF THIS STEP WAS WEAKER THAN THE TEMPLATE IT CAME FROM, in
# three ways a review found by seeding them:
#
#   * it walked dicts and not lists, so integers inside an array were neither
#     derived nor refused;
#   * it tested `isinstance(value, int)`, so a number written as a TOML float
#     was silently skipped -- and the stated COUNT fell with it, so the message
#     shrank rather than complaining;
#   * with every front-matter number written as a float, it reported "0 stated
#     integer(s)" and exited 0, while `check-recompute.py`'s probe found no
#     `^\w+ = \d+$` line to perturb and scored that as a pass. A check that
#     cannot fail, inside the step added to answer a check that cannot fail.
#
# So: both directions, every numeric type, containers walked, and comparison by
# type as well as value -- `0 == 0.0` in Python, and a count written as a float
# is a different statement about a record whose counts are integers.
here, product_name = pathlib.Path(sys.argv[1]), sys.argv[2]
text = (here / "README.md").read_text(encoding="utf-8")
match = re.match(r"\+\+\+\n(.*?)\n\+\+\+\n", text, re.S)
if match is None:
    print("recompute: README.md has no +++ front matter"); sys.exit(1)
front = tomllib.loads(match.group(1))

rows = [json.loads(line) for line in (here / "run.jsonl").read_text().splitlines()]
start = next((r for r in rows if r.get("record") == "start"), None)
if start is None:
    print("recompute: the record has no start row"); sys.exit(1)

# A recompute over an archive conducts no session, so its turn count and its
# prefill total are whatever the record's own turn rows come to. Counted, not
# asserted: the zeroes are a measurement of this record and not a convention.
turns = [row for row in rows if row.get("record") == "turn"]
# `dogma_version` comes from the ARM, not from the record. Reading it out of
# the record's own start row would be a restatement dressed as a derivation --
# the record's self-agreement is the thing this step exists to distrust -- and
# an earlier version of this script did exactly that, so a README and a record
# altered together passed. The arm is the independent artefact, and the start
# row is checked against it below.
regimen = tomllib.loads((here / "regimen.toml").read_text(encoding="utf-8"))
derived = {
    "turns": len(turns),
    "prefill_tokens_total": sum(int(row.get("prefill_tokens") or 0) for row in turns),
    "regime.dogma_version": regimen["dogma_version"],
    "product_sha256": hashlib.sha256((here / product_name).read_bytes()).hexdigest(),
}
if start["regime"]["dogma_version"] != regimen["dogma_version"]:
    print(f"recompute: the record's start row says `dogma_version = "
          f"{start['regime']['dogma_version']!r}`, the arm says "
          f"{regimen['dogma_version']!r}"); sys.exit(1)


def stated_values(obj, prefix=""):
    """Every number and every digest the front matter states, by path.

    Lists are walked as well as tables: a number inside an array is still a
    number the record asserts, and the version of this that walked only tables
    let `null_steps = [3, 4]` through without deriving or refusing it.
    """
    if isinstance(obj, dict):
        pairs = [(f"{prefix}.{k}" if prefix else k, v) for k, v in obj.items()]
    elif isinstance(obj, list):
        pairs = [(f"{prefix}[{i}]", v) for i, v in enumerate(obj)]
    else:
        return
    for path, value in pairs:
        if isinstance(value, bool):
            continue
        if isinstance(value, (int, float)) or path == "product_sha256":
            yield path, value
        else:
            yield from stated_values(value, path)


def agrees(stated, want):
    """Equal, and the same type.

    `0 == 0.0` is True in Python. These are counts, and a count written as a
    float is a different statement about the record -- and, left alone, one the
    mutation probe's `^\\w+ = \\d+$` cannot even find to perturb.
    """
    return type(stated) is type(want) and stated == want


bad = 0
stated = dict(stated_values(front))
for path, value in sorted(stated.items()):
    if path not in derived:
        print(f"recompute: the front matter states `{path} = {value!r}`, which this "
              f"script does not derive from the artefacts"); bad += 1
    elif not agrees(value, derived[path]):
        print(f"recompute: the front matter says `{path} = {value!r}`, the artefacts "
              f"give {derived[path]!r}"); bad += 1
for path in sorted(derived):
    if path not in stated:
        print(f"recompute: the artefacts derive `{path} = {derived[path]!r}`, which "
              f"the front matter does not state"); bad += 1
summary = next((row for row in rows if row.get("record") == "summary"), None)
if summary is not None:
    for key in ("turns", "prefill_tokens_total", "product_sha256"):
        if not agrees(summary.get(key), derived[key]):
            print(f"recompute: the record's summary says `{key} = {summary.get(key)!r}`, "
                  f"the artefacts give {derived[key]!r}"); bad += 1
if bad: sys.exit(1)
print(f"recompute: {len(stated)} stated value(s) re-derive from the artefacts, and "
      f"every value the artefacts derive is stated")
PY
