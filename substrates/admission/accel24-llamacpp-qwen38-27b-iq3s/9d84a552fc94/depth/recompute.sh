#!/usr/bin/env bash
# Recompute the candidate's depth cell (#143) from its rows through the committed instrument: summarise the
# rows, decide under the committed criterion, and require both outputs to equal the committed ones byte for
# byte. Require the run's corpus manifest to be the committed one; the running exe and weights to be this
# fingerprint's and the served template this fingerprint's; the window log and restore checks to show the
# window closed as announced; the ladder the ruled fractions of the registry's serving_context, every cell
# present and full; the sampler the registry's declared card and max_tokens 4096; every row regraded; and
# cell.toml's word, reading and raw digests to agree. Exit 0 when all hold, 1 when any fails.
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
        fails.append(f"cell {f} holds {c['application']['n']} application and {c['retrieval']['n']} retrieval samples; the run declared {meta['samples']} and one")
# depth, which is what this cell measures: each target is its fraction of the pool, each cell reached it within
# the instrument's default tolerance (0.02) without exceeding it, and every deeper cell carries all four counter-examples
for frac, target in meta["cells"]:
    c = summ.get(str(frac))
    if target != round(frac * meta["serving_context"]): fails.append(f"cell {frac}'s target {target} is not its fraction of the pool")
    if c and frac > 0 and not (target * 0.98 <= c["depth_rendered"] <= target): fails.append(f"cell {frac} rendered {c['depth_rendered']} tokens against a target of {target}")
    if c and c["planted"] != (0 if frac == 0 else 4): fails.append(f"cell {frac} planted {c['planted']} counter-examples")
rows = [json.loads(l) for l in (here / "raw/rows.jsonl").read_text().splitlines()]
for r in rows:
    c = summ.get(str(r["fraction"]))
    if not c or (r["depth_rendered"], r["planted"]) != (c["depth_rendered"], c["planted"]): fails.append(f"a row at {r['fraction']} disagrees with its cell's depth or planting")
names = sorted(p.name for p in (here / "raw").iterdir())
cell0 = tomllib.loads((here / "cell.toml").read_text())
if names != sorted(cell0["raw"]): fails.append(f"raw/ holds {names}; cell.toml pins {sorted(cell0['raw'])}")
# the whole reading, derived from the summary: application counts per cell, then retrieval and the defects
fmt = lambda f, c: ("control" if f == "0.0" else f"{f} ({c['depth_rendered']:,} tokens)") + f" {c['application']['pass']}/{c['application']['n']}"
want = "; ".join(fmt(f, summ[f]) for f in sorted(summ, key=float))
retr = {f: (c["retrieval"]["pass"], c["retrieval"]["n"]) for f, c in summ.items()}
want += "; " + ("retrieval 1/1 at every cell" if all(v == (1, 1) for v in retr.values()) else "retrieval " + ", ".join(f"{f} {p}/{n}" for f, (p, n) in sorted(retr.items(), key=lambda x: float(x[0]))))
tot = {k: sum(c["application"][k] for c in summ.values()) for k in ("errors", "truncated", "thinking_off")}
want += "; " + ", ".join(("no " + w) if tot[k] == 0 else f"{tot[k]} {w}" for k, w in (("errors", "server error"), ("truncated", "truncation"), ("thinking_off", "thinking-off sample")))
if cell0["reading"] != want: fails.append(f"cell.toml's reading is not the summary's: {want}")
# regrade every row from its own text through the committed grader; an excerpt at its cap cannot be regraded
sys.path.insert(0, str(probe)); import depth_probe as dp
for r in rows:
    tag = f"{r['fraction']} {r['stage']} {r['sample']}"
    if r["stage"] == "application":
        if len(r.get("code") or "") >= 1500: fails.append(f"{tag}: the code excerpt is at its cap and cannot be regraded"); continue
        if dp.grade(meta["tier"], r.get("code") or None) != r["dims"]: fails.append(f"{tag}: the stored grade is not the code's")
        if r["prompt_tokens"] != r["depth_rendered"]: fails.append(f"{tag}: the server counted {r['prompt_tokens']} prompt tokens against a rendered depth of {r['depth_rendered']}")
    else:
        if len(r.get("answer") or "") >= 600: fails.append(f"{tag}: the answer excerpt is at its cap and cannot be regraded"); continue
        if dp.grade_retrieval(r["answer"]) != r["dims"]: fails.append(f"{tag}: the stored grade is not the answer's")
for f in summ:
    got = sorted(r["sample"] for r in rows if str(r["fraction"]) == f and r["stage"] == "application")
    if got != list(range(meta["samples"])): fails.append(f"cell {f}'s application samples are numbered {got}")
# the console log's per-sample lines are the rows'
con = re.findall(r"cell ([0-9.]+) \((\d+) tok, (\d+) planted\) (\w+) (\d+): ALL=(\w+) err=(\S+)", (here / "raw/console.log").read_text())
if con != [(str(r["fraction"]), str(r["depth_rendered"]), str(r["planted"]), r["stage"], str(r["sample"]), str(r["dims"]["ALL"]), str(r["error"])) for r in rows]:
    fails.append("raw/console.log's per-sample lines are not the rows'")
# identity: the running candidate's exe (read by the box script at launch) is the llama-server of this
# fingerprint's engine manifest, its weights are this fingerprint's, and the window log's server is that pid;
# the window log shows production stopped, the candidate up, and the floor restored with its own exe and line
idb = (here / "raw/identity-before.txt").read_text(); wl = (here / "raw/window.log").read_text()
run = re.search(r"cand_running_exe ([0-9a-f]{64}) pid=(\d+)", idb)
manifest = (here.parent / "raw/engine-manifest.txt").read_text()
if not run or f"cand_file {run.group(1)} llama-server" not in manifest: fails.append("the running candidate's exe is not this fingerprint's llama-server")
if not re.search(rf"cand_weights {fp['weights']['main']} ", idb): fails.append("the candidate's weights are not this fingerprint's")
if not run or not re.search(rf"candidate up server={run.group(2)} exe={run.group(1)[:16]} ", wl): fails.append("the window log's candidate is not the identity's")
if not re.search(r"production stopped", wl) or not re.search(r"restored: pid=\d+ exe=980845d60ae7a820 cmdline_same=yes", wl): fails.append("the window log does not show production stopped and restored on its own exe and line")
ml = (here / "raw/mac.log").read_text(); rc = (here / "raw/restore-checks.txt").read_text()
for need, where, what in (("fingerprint rc=0", ml, "the fingerprint check"), ("verify-box rc=0", ml, "the box verification"), ("depth run rc=0", ml, "the probe run"),
                          ("SUBSTRATE IDENTICAL", rc, "the fingerprint verdict"), ("verify-box: PASS", rc, "the verification verdict"), ("verdict: PASS", rc, "the canary verdict")):
    if need not in where: fails.append(f"{what} is not recorded as passing")
card = dict((k, float(v)) for k, v in re.findall(r"(\w+) ([0-9.]+)", reg["sampler_card_declared"]))
if {k: float(v) for k, v in meta["sampler"].items()} != card: fails.append(f"the sampler {meta['sampler']} is not the registry's declared card {card}")
if meta["max_tokens"] != 4096: fails.append("max_tokens is not the declared 4096")
if meta["template_sha256"] != fp["template"]: fails.append("the served template is not this fingerprint's")
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
