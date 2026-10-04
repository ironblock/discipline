#!/usr/bin/env bash
# Gate 0 for this directory, #165's re-judge and laya calibration bound.
# Every artefact the claim consumes must hash as the record names it; results.json must re-derive, byte for byte,
# from the committed verdicts, requests, fp32 logits and served responses by analyze.py; the claim's word must be
# the one results.json's readings give under planning's hypothesis (5975253990); the Sidekick program's files must chain to its
# manifest (14b034ea...), directly or through REDACTED.sha256; the two scrubbed files must hash as scrub.json says;
# and the front matter's numbers must re-derive. Where the repository is around this directory, the draw (plan,
# sample, layout) is rebuilt from the archive by draw.py and compared; in check-recompute's sandbox (this directory
# alone) that step says it did not run. Stdlib only. Exit 0 when all hold, 1 when any fails, 2 when a step cannot run.
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
cp -R "$here"/. "$tmp/copy"
( cd "$tmp/copy" && python3 -B analyze.py . results.json >/dev/null ) || { echo "recompute: analyze.py failed"; exit 2; }
cmp -s "$tmp/copy/results.json" "$here/results.json" || { echo "recompute: results.json does not re-derive from the committed verdicts, requests and responses"; exit 1; }
echo "recompute: results.json re-derives byte for byte"
python3 - "$here" <<'PY' || exit 1
import hashlib, json, pathlib, sys
here = pathlib.Path(sys.argv[1]); r = json.loads((here / "results.json").read_text())
# The word, under planning's hypothesis (5975253990, (d)): laya-typed-decisions as shipped, with this program's own
# temperature, agrees with the fresh Sonnet majority at or above the judges' own agreement (1 - d), and is
# calibrated within the labels' noise, on both questions. Calibration is amendment 1's three-way reading of pass A's
# cross-fitted top-1 ECE against e3 + the floor, with F2 the floor of record and F1 beside (ruling (b)); the two
# must agree, or the record does not decide the reading and this step says so. A question whose temperature fits
# sit at the pre-registered range's bound reads `inconclusive (fit at bound)` (ruling (a)). The hypothesis needs both
# halves on both questions: any agreement short of the judges' own, or any `miscalibrated`, refutes it.
readings, agree_ok, bad = {}, {}, 0
for q, rule in sorted(r["rule"].items()):
    reading = rule["F2_base_rate"]["reading"]
    if rule["F1_consistency"]["reading"] != reading:
        print(f"recompute: the {q} question's floors disagree: F2 {reading!r}, F1 {rule['F1_consistency']['reading']!r}"); sys.exit(1)
    if any(r["laya"][q]["passes"]["A"]["T_at_range_bound"].values()):
        reading = "inconclusive (fit at bound)"
    readings[q] = reading
    agree_ok[q] = r["laya"][q]["passes"]["A"]["agreement_with_fresh_majority"]["rate"] >= 1 - r["judges"][q]["d"]
if not all(agree_ok.values()) or "miscalibrated" in readings.values():
    word = "refuted"
elif set(readings.values()) == {"calibrated"}:
    word = "supported"
else:
    word = "inconclusive"
readings = {q: f"{readings[q]}; agreement {'at or above' if agree_ok[q] else 'below'} the judges' own" for q in readings}
claims = [json.loads(l) for l in (here / "run.jsonl").read_text().splitlines() if json.loads(l).get("record") == "claim"]
if not claims or any(c["result"] != word for c in claims):
    print(f"recompute: the readings are {readings}, so the word is {word!r}; the claim rows say {[c['result'] for c in claims]}"); sys.exit(1)
print(f"recompute: the readings {readings} give the claim's word {word!r}")
# The Sidekick program's files: each clean file hashes as MANIFEST.sha256 says; each redacted file hashes as
# REDACTED.sha256's first column says, and that row's second column is the file's MANIFEST digest.
sk = here / "sidekick"
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
if sha(sk / "MANIFEST.sha256") != "14b034ea896fefa4cb72201ab98ceb1f75922b929554ab95ee1346f3c652fa15":
    print("recompute: sidekick/MANIFEST.sha256 is not the manifest the Sidekick program posted (14b034ea...)"); sys.exit(1)
manifest = {l.split()[1]: l.split()[0] for l in (sk / "MANIFEST.sha256").read_text().splitlines() if l.strip()}
redacted = {}
for l in (sk / "REDACTED.sha256").read_text().splitlines():
    if l.strip() and not l.startswith("#"):
        f = l.split(); redacted[f[2]] = (f[0], f[1])
for name, want in sorted(manifest.items()):
    got = sha(sk / name)
    if name in redacted:
        if redacted[name][1] != want or got != redacted[name][0]:
            print(f"recompute: sidekick/{name} does not chain to the manifest through REDACTED.sha256"); bad += 1
    elif got != want:
        print(f"recompute: sidekick/{name} hashes to {got[:12]}..., the manifest says {want[:12]}..."); bad += 1
if set(redacted) - set(manifest):
    print(f"recompute: REDACTED.sha256 names files the manifest does not: {sorted(set(redacted) - set(manifest))}"); bad += 1
for row in json.loads((here / "scrub.json").read_text()):
    if sha(here / row["path"]) != row["scrubbed_sha256"]:
        print(f"recompute: {row['path']} does not hash as scrub.json's scrubbed digest"); bad += 1
if bad: sys.exit(1)
print(f"recompute: {len(manifest)} Sidekick file(s) chain to its manifest, {len(redacted)} through REDACTED.sha256; the scrubbed files hash as scrub.json says")
PY
root="$(cd -- "$here/../.." && pwd)"
if [ -f "$root/results/2026-10-02-judge-state-lengths/measure.py" ] && ls "$root"/results/*/judge/batches >/dev/null 2>&1; then
  python3 -B "$here/rejudge/draw.py" "$root" "$tmp/draw" >/dev/null || { echo "recompute: draw.py failed"; exit 2; }
  for f in plan.json sample.json layout.json; do cmp -s "$tmp/draw/$f" "$here/rejudge/$f" || { echo "recompute: rejudge/$f does not rebuild from the archive"; exit 1; }; done
  echo "recompute: plan.json, sample.json and layout.json rebuild from the archive by draw.py"
else
  echo "recompute: the draw was not rebuilt here (no repository around this directory)"
fi
python3 - "$here" "results.json" <<'PY' || exit 1
import hashlib, json, pathlib, re, sys, tomllib

# Every number the README's FRONT MATTER states, re-derived from the artefacts, and every number the artefacts
# derive, stated. This step is 2026-10-02-judge-state-lengths' third step, unchanged: it reads the `+++` block and
# nothing else, so the figures in the body are bound to the product by the analysis step above, not by this one.
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

consumed = {}
for row in rows:
    for artifact in row.get("consumes") or []:
        consumed[artifact["path"]] = artifact["sha256"]
targets = sorted(consumed)
matched = [path for path in targets
           if hashlib.sha256((here / path).read_bytes()).hexdigest() == consumed[path]]
regimen = tomllib.loads((here / "regimen.toml").read_text(encoding="utf-8"))
derived = {
    "targets_checked": len(targets),
    "targets_matched": len(matched),
    "regime.dogma_version": regimen["dogma_version"],
    "product_sha256": hashlib.sha256((here / product_name).read_bytes()).hexdigest(),
}
if start["regime"]["dogma_version"] != regimen["dogma_version"]:
    print(f"recompute: the record's start row says `dogma_version = "
          f"{start['regime']['dogma_version']!r}`, the arm says "
          f"{regimen['dogma_version']!r}"); sys.exit(1)


def stated_values(obj, prefix=""):
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
    if summary.get("kind") != "recompute":
        print(f"recompute: the record's summary is a {summary.get('kind')!r}, not a recompute"); bad += 1
    for key in ("targets_checked", "targets_matched", "product_sha256"):
        if not agrees(summary.get(key), derived[key]):
            print(f"recompute: the record's summary says `{key} = {summary.get(key)!r}`, "
                  f"the artefacts give {derived[key]!r}"); bad += 1
    if summary.get("digests") != [consumed[path] for path in targets]:
        print("recompute: the record's summary lists digests that are not the consumed "
              "artefacts' digests in path order"); bad += 1
if bad: sys.exit(1)
print(f"recompute: {len(stated)} stated value(s) re-derive from the artefacts, and "
      f"every value the artefacts derive is stated")
PY
