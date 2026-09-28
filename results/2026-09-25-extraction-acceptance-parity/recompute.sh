#!/usr/bin/env bash
# Gate 0 for this directory, the parity fire of extraction-acceptance-inverts.
# Every artifact the record's claims consume must hash to the digest the record
# names; the band must re-derive, byte for byte, from the archived row's
# committed tallies by band.py; the archived row's own grader over the re-fired
# seat logs must reproduce the committed report, field by field; and the
# ratified applier over the archived row, the band, the re-fired logs and the
# box record must reproduce the committed verdict, byte for byte.
# Exit 0 when all hold, 1 when any fails, 2 when a step cannot run.
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
( cd "$here" && python3 -B band.py archived ) > "$tmp/band.json" || { echo "recompute: band.py failed"; exit 2; }
cmp -s "$tmp/band.json" "$here/band.json" || { echo "recompute: band.py over the archived tallies does not reproduce band.json"; exit 1; }
echo "recompute: band.json re-derives byte for byte from the archived tallies"
python3 -B "$here/archived/grade.py" --seat-a "$here/seat-a" --seat-b "$here/seat-b" --seat-c "$here/archived/seat-c" --gliner "$here/archived/seat-c" --out "$tmp" >/dev/null || { echo "recompute: the grader failed"; exit 2; }
python3 - "$tmp/report.json" "$here/report.json" <<'PY' || exit 1
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
( cd "$here" && python3 -B apply.py archived band.json . box.json ) > "$tmp/verdict.json" || { echo "recompute: apply.py failed"; exit 2; }
cmp -s "$tmp/verdict.json" "$here/verdict.json" || { echo "recompute: apply.py does not reproduce verdict.json"; exit 1; }
python3 - "$here" <<'PY' || exit 1
import json, pathlib, sys
here = pathlib.Path(sys.argv[1])
word = json.loads((here / "verdict.json").read_text())["word"]
rows = [json.loads(l) for l in (here / "run.jsonl").read_text().splitlines() if l.strip()]
claims = [r for r in rows if r.get("record") == "claim"]
if not claims or any(c.get("result") != word for c in claims):
    print(f"recompute: the applier's word is {word!r} and the claim rows say {[c.get('result') for c in claims]}"); sys.exit(1)
print(f"recompute: verdict.json re-derives byte for byte; the word, {word!r}, is the claim rows'")
PY
( cd "$here" && python3 -B apply.py --selftest archived band.json ) > "$tmp/selftest.out" || { echo "recompute: apply.py's selftest does not pass (its fixtures, seen red before the fire)"; exit 1; }
tail -1 "$tmp/selftest.out"
python3 - "$here" <<'PY' || exit 1
# The box record and the window's raw outputs, tied: every box.json field is
# re-derived from the raw output of the command that wrote it; the seats'
# harness exit codes are 0 (a non-zero exit is unadjudicated under the one-fire
# rule); the start row's engine and weights are the ones read off the running
# servers; the digests the pre-registration names are the files here; and the
# claim row and the front matter say what verdict.json and the pre-registration
# say. The instance id is the one thing not re-derivable here: the fingerprint
# log names the capture's digest, not the registry's instance id (known_defects).
import hashlib, json, pathlib, re, sys, tomllib
here = pathlib.Path(sys.argv[1]); w = here / "window"; bad = []
read = lambda p: (w / p).read_text(encoding="utf-8")
box = json.loads((here / "box.json").read_text())
def canary(name):
    m = re.findall(r"^verdict: (PASS|DRIFT)\b", read(name), re.M)
    return m[-1] if m else "NONE"
want = {
    "canary_before": [canary("canary-before-1.log"), canary("canary-before-2.log")],
    "canary_after": [canary("canary-after-1.log")],
    "verify_before": 0 if re.search(r"^verify-box: PASS$", read("verify-before.log"), re.M) else 1,
    "verify_after": 0 if re.search(r"^verify-box: PASS$", read("verify-after.log"), re.M) else 1,
    "fingerprint_before": 0 if "SUBSTRATE IDENTICAL" in read("fingerprint-before.log") else 3,
    "fingerprint_after": 0 if "SUBSTRATE IDENTICAL" in read("fingerprint-after.log") else 3,
}
answer = json.loads(read("seatb-canary.json"))
msg = (answer.get("choices") or [{}])[0].get("message") or {}
want["seatb_canary"] = {"think": "<think>" in (msg.get("content") or "") or bool(msg.get("reasoning_content")),
                        "draft_n": "draft_n" in (answer.get("timings") or {})}
for k, v in want.items():
    if box.get(k) != v: bad.append(f"box.json {k} is {box.get(k)!r}; the raw output gives {v!r}")
digests = {n: re.findall(r"^(baseline|current)  ?digest: ([0-9a-f]+)$", read(n), re.M) for n in ("fingerprint-before.log", "fingerprint-after.log")}
seen = {d for pairs in digests.values() for _, d in pairs}
if len(seen) != 1 or any(len(p) != 2 for p in digests.values()):
    bad.append(f"the fingerprint logs do not name one capture digest before and after: {sorted(seen)}")
if not (box.get("instance_before") == box.get("instance_after")): bad.append("box.json's instances differ")
for seat in ("seat-a", "seat-b"):
    if read(f"{seat}.rc").strip() != "0": bad.append(f"{seat}'s harness exited {read(seat + '.rc').strip()}")
start = next(json.loads(l) for l in (here / "run.jsonl").read_text().splitlines() if json.loads(l).get("record") == "start")
sub = {s["id"]: s for s in start["regime"]["substrates"]}
ident = dict(l.split(" ", 1) for l in read("seatb-identity.txt").splitlines() if " " in l)
cpu, acc = sub["cpu-beellama-qwen3-1p7b-q4km"], sub["accel24-beellama-qwen27b-q4kxl"]
if ident.get("exe_sha256") != cpu["engine"]["version_or_digest"]: bad.append("the seat-B engine in the start row is not the one read at launch")
if ident.get("weights_sha256") != cpu["weights"]["sha256"]: bad.append("the seat-B weights in the start row are not the ones read at launch")
if acc["engine"]["version_or_digest"] not in read("production-before.txt").split(): bad.append("the production engine in the start row is not the one read off the running server")
pre = json.loads((here / "pre-registration.json").read_text())
rule = tomllib.loads((here / "decision-rule.toml").read_text())
sha = lambda p: hashlib.sha256((here / p).read_bytes()).hexdigest()
for p, stated in (("band.py", pre["band"]["band_py_sha256"]), ("band.json", pre["band"]["band_json_sha256"]),
                  ("apply.py", pre["rule"]["applier_sha256"]), ("apply.py", rule["rule"]["applier_sha256"])):
    if sha(p) != stated: bad.append(f"{p} hashes to {sha(p)[:12]}..., the pre-registration names {stated[:12]}...")
text = (here / "README.md").read_text(encoding="utf-8")
front = tomllib.loads(re.match(r"\+\+\+\n(.*?)\n\+\+\+\n", text, re.S).group(1))
if front["hypothesis"] != pre["hypothesis"]: bad.append("the front matter's hypothesis is not the pre-registration's")
v = json.loads((here / "verdict.json").read_text())
claim = next(json.loads(l) for l in (here / "run.jsonl").read_text().splitlines() if json.loads(l).get("record") == "claim")
figures = [f"{v['effect']:.6f}", f"seat A {v['seat_a']['accepted_deduped']} of {v['seat_a']['offered']}",
           f"seat B {v['seat_b']['accepted_deduped']} of {v['seat_b']['offered']}", f"[{v['band'][0]}, {v['band'][1]}]"]
missing = [f for f in figures if f not in claim["hypothesis"]]
if missing: bad.append(f"the claim row does not state verdict.json's {missing}")
for b in bad: print(f"recompute: {b}")
if bad: sys.exit(1)
print("recompute: box.json re-derives from the window's raw outputs; the seats exited 0; the start row's engines and weights are the ones read at the window; the pre-registration's digests, hypothesis and the claim row's figures agree with the files")
PY
python3 - "$here" "report.json" <<'PY' || exit 1
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

# A recompute's summary counts targets, not turns: how many artefacts the
# claims consume, and how many of them hash as the record says. Both are
# re-derived here from the files rather than read back from the summary --
# the record's self-agreement is the thing this step exists to distrust.
consumed = {}
for row in rows:
    for artifact in row.get("consumes") or []:
        consumed[artifact["path"]] = artifact["sha256"]
targets = sorted(consumed)
matched = [path for path in targets
           if hashlib.sha256((here / path).read_bytes()).hexdigest() == consumed[path]]
# `dogma_version` comes from the ARM, not from the record. Reading it out of
# the record's own start row would be a restatement dressed as a derivation --
# the record's self-agreement is the thing this step exists to distrust -- and
# an earlier version of this script did exactly that, so a README and a record
# altered together passed. The arm is the independent artefact, and the start
# row is checked against it below.
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
    if summary.get("kind") != "recompute":
        print(f"recompute: the record's summary is a {summary.get('kind')!r}, not a "
              f"recompute"); bad += 1
    for key in ("targets_checked", "targets_matched", "product_sha256"):
        if not agrees(summary.get(key), derived[key]):
            print(f"recompute: the record's summary says `{key} = {summary.get(key)!r}`, "
                  f"the artefacts give {derived[key]!r}"); bad += 1
    # The digests the summary says it compared are the consumed artefacts'
    # digests, in the order this script compares them.
    if summary.get("digests") != [consumed[path] for path in targets]:
        print("recompute: the record's summary lists digests that are not the consumed "
              "artefacts' digests in path order"); bad += 1
if bad: sys.exit(1)
print(f"recompute: {len(stated)} stated value(s) re-derive from the artefacts, and "
      f"every value the artefacts derive is stated")
PY
