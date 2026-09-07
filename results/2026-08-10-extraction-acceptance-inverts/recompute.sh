#!/usr/bin/env bash
# Gate 0 for this directory: every artifact the record's claims consume must
# hash to the digest the record names, and the committed grader over the
# committed seat logs must reproduce the committed report, field by field.
# Exit 0 when both hold, 1 when either fails, 2 when the check cannot run.
set -uo pipefail
here="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
tmp="$(mktemp -d)" || exit 2
trap 'rm -rf "$tmp"' EXIT
python3 - "$here" <<'PY' || exit 1
import hashlib, json, pathlib, sys
here = pathlib.Path(sys.argv[1]); seen = {}
for line in (here / "run.jsonl").read_text().splitlines():
    for artifact in json.loads(line).get("consumes") or []:
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
python3 "$here/grade.py" --seat-a "$here/seat-a" --seat-b "$here/seat-b" --seat-c "$here/seat-c" --gliner "$here/seat-c" --out "$tmp" >/dev/null || { echo "recompute: the grader failed"; exit 2; }
python3 - "$tmp/report.json" "$here/report.json" <<'PY'
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
python3 - "$here" "report.json" <<'PY' || exit 1
import hashlib, json, pathlib, re, sys, tomllib

# Third step: every number the README states, re-derived from the artefacts.
#
# The first two steps prove the consumed artefacts are the ones the record
# names and that the instrument reproduces the product. Neither reads the
# README, so a stated figure could drift from the record it summarises and
# both would still pass. `check-recompute.py` probes for exactly that: it
# perturbs one integer in the README and requires this script to notice.
#
# The rule here is stronger than answering the probe. An integer this script
# cannot derive is refused, because the probe lands on whichever integer comes
# first and an unchecked one would let "1 recomputed" mean less than it says.
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
derived = {
    "turns": len(turns),
    "prefill_tokens_total": sum(int(row.get("prefill_tokens") or 0) for row in turns),
    "regime.dogma_version": start["regime"]["dogma_version"],
}
product = hashlib.sha256((here / product_name).read_bytes()).hexdigest()

def integers(obj, prefix=""):
    for key, value in obj.items():
        path = f"{prefix}{key}"
        if isinstance(value, dict):
            yield from integers(value, f"{path}.")
        elif isinstance(value, int) and not isinstance(value, bool):
            yield path, value

bad = 0
stated = dict(integers(front))
for path, value in sorted(stated.items()):
    if path not in derived:
        print(f"recompute: the front matter states `{path} = {value}`, which this "
              f"script does not derive from the artefacts"); bad += 1
    elif derived[path] != value:
        print(f"recompute: the front matter says `{path} = {value}`, the artefacts "
              f"give {derived[path]}"); bad += 1
if front.get("product_sha256") != product:
    print(f"recompute: the front matter names product {front.get('product_sha256')}, "
          f"{product_name} hashes to {product}"); bad += 1
summary = next((row for row in rows if row.get("record") == "summary"), None)
if summary is not None:
    for key in ("turns", "prefill_tokens_total", "product_sha256"):
        want = product if key == "product_sha256" else derived[key]
        if summary.get(key) != want:
            print(f"recompute: the record's summary says `{key} = {summary.get(key)!r}`, "
                  f"the artefacts give {want!r}"); bad += 1
if bad: sys.exit(1)
print(f"recompute: {len(stated)} stated integer(s) and the product digest re-derive "
      f"from the artefacts")
PY
