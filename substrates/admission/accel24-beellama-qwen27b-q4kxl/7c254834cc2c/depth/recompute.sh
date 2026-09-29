#!/usr/bin/env bash
# Recompute the floor's depth cell (#143) from its rows through the committed instrument: summarise the
# rows, decide under the committed criterion, and require both outputs to equal the committed ones byte for
# byte. Require the run's corpus manifest to be the committed one; the identity read after the window to be
# this fingerprint's engine and the served template this fingerprint's; the ladder the ruled fractions of the
# registry's serving_context, with every cell present and full; the sampler the registry's card and max_tokens
# 4096; and cell.toml's word and raw digests to agree. Exit 0 when all hold, 1 when any fails.
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
import re, tomllib
fp = json.loads(json.loads((here.parent / "fingerprint.json").read_text())["canonical"])
reg = tomllib.loads((here.parents[3] / "registry.toml").read_text())["substrate"][here.parents[1].name]
summ = json.loads((here / "raw/summary.json").read_text())
if not (meta["admission"] and meta["tier"] == "supported"): fails.append("the run was not the admission probe")
if [c[0] for c in meta["cells"]] != [0.0, 0.5, 0.9, 0.95] or meta["serving_context"] != reg["serving_context"]:
    fails.append("the ladder is not the ruled fractions of the registry's serving_context")
if sorted(summ) != sorted(str(c[0]) for c in meta["cells"]): fails.append(f"the summary's cells {sorted(summ)} are not the ladder's")
for f, c in summ.items():
    if c["application"]["n"] != meta["samples"] or c["application"]["n"] < 5 or c["retrieval"]["n"] != 1:
        fails.append(f"cell {f} holds {c['application']['n']} application and {c['retrieval']['n']} retrieval samples")
card = dict((k, float(v)) for k, v in re.findall(r"(\w+) ([0-9.]+)", reg["sampler_card"]))
if {k: float(v) for k, v in meta["sampler"].items()} != card: fails.append(f"the sampler {meta['sampler']} is not the registry's card {card}")
if meta["max_tokens"] != 4096: fails.append("max_tokens is not the declared 4096")
if meta["template_sha256"] != fp["template"]: fails.append("the served template is not this fingerprint's")
if f"exe={fp['engine']}" not in (here / "raw/identity-after.txt").read_text():
    fails.append("the identity after the window is not this fingerprint's engine")
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
