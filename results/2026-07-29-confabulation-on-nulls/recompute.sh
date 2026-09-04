#!/usr/bin/env bash
# Gate 0 for this directory: run the committed instrument over the committed
# artifact and compare, field by field, with the committed headline.
# Exit 0 when every field reproduces, 1 when any differs, 2 when it cannot run.
set -uo pipefail
here="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
tmp="$(mktemp -d)" || exit 2
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
