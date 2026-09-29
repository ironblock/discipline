#!/usr/bin/env bash
# Recompute the floor's depth cell (#143) from its rows through the committed instrument: summarise the
# rows, decide under the committed criterion, and require both outputs to equal the committed ones byte for
# byte; require the run's corpus manifest digest to be the committed manifest's, and the identity read after
# the window to be this cells directory's exe. Exit 0 when all hold, 1 when any fails.
set -uo pipefail
here="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
probe="$here/../../../depth-probe"
tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT
bad=0
python3 -B "$probe/depth_probe.py" summarise "$here/raw/rows.jsonl" > "$tmp/summary.json" || { echo "recompute: summarise failed"; exit 1; }
python3 -B "$probe/depth_probe.py" decide "$tmp/summary.json" "$probe/criterion.toml" > "$tmp/decide.json" || { echo "recompute: decide failed"; exit 1; }
cmp -s "$tmp/summary.json" "$here/raw/summary.json" || { echo "recompute: the rows do not summarise to raw/summary.json"; bad=1; }
cmp -s "$tmp/decide.json" "$here/raw/decide.json" || { echo "recompute: the summary does not decide to raw/decide.json"; bad=1; }
python3 - "$here" "$probe" <<'PY' || bad=1
import hashlib, json, pathlib, sys
here, probe = map(pathlib.Path, sys.argv[1:])
meta = json.loads((here / "raw/meta.json").read_text()); dec = json.loads((here / "raw/decide.json").read_text())
fails = []
if meta["corpus_manifest_sha256"] != hashlib.sha256((probe / "corpus/manifest.json").read_bytes()).hexdigest():
    fails.append("the run's corpus manifest is not the committed one")
if not (meta["admission"] and meta["tier"] == "supported"): fails.append("the run was not the admission probe")
if [c[0] for c in meta["cells"]] != [0.0, 0.5, 0.9, 0.95] or meta["serving_context"] != 160000: fails.append("the ladder is not the ruled fractions of the registry's 160,000 pool")
if "exe=980845d60ae7a820f5e2a8b7081727a242b35d3ca8a4021a6fb1240f4a0aa3d4" not in (here / "raw/identity-after.txt").read_text():
    fails.append("the identity after the window is not this cells directory's exe")
import tomllib
cell = tomllib.loads((here / "cell.toml").read_text())
for name, want in cell["raw"].items():
    if hashlib.sha256((here / "raw" / name).read_bytes()).hexdigest() != want: fails.append(f"raw/{name} does not hash as cell.toml says")
if cell["word"] != dec["word"] or cell["strict_beside"] != dec["strict_beside"]: fails.append(f"cell.toml says {cell['word']!r} / {cell['strict_beside']!r}; decide gives {dec['word']!r} / {dec['strict_beside']!r}")
for f in fails: print(f"recompute: {f}")
print(f"recompute: word {dec['word']!r}, strict beside {dec['strict_beside']!r}" if not fails else "", end="\n" if not fails else "")
sys.exit(1 if fails else 0)
PY
exit $bad
