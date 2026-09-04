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
python3 - "$tmp/headline.json" "$here/headline.json" <<'PY'
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
