#!/usr/bin/env bash
# Gate 0 for this directory, (b')-S2 (#142): (b')'s rule re-fired on a second substrate. Every artefact the
# record's claims consume must hash to the digest the record names; the ratified
# applier's selftest must pass; the applier over the committed rows, judge
# verdicts and key must reproduce word.json byte for byte, and its word must
# be the claim rows'; the comparison row must be comparison.json's word and counts, endpoint 2's
# hypothesis, consuming comparison.json; the rule and applier must be the digests the
# pre-registration names; the front matter's numbers must re-derive.
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
( cd "$here" && python3 -B apply_bprime.py --selftest decision-rule.toml ) > "$tmp/selftest.out" || { echo "recompute: the applier's selftest does not pass"; exit 1; }
tail -1 "$tmp/selftest.out"
( cd "$here" && python3 -B apply_bprime.py decision-rule.toml . ) > "$tmp/word.json" || { echo "recompute: the applier failed"; exit 2; }
cmp -s "$tmp/word.json" "$here/word.json" || { echo "recompute: the applier over this record does not reproduce word.json"; exit 1; }
( cd "$here" && python3 -B beside.py ) > "$tmp/beside.json" || { echo "recompute: beside.py failed"; exit 2; }
cmp -s "$tmp/beside.json" "$here/beside.json" || { echo "recompute: beside.py does not reproduce beside.json"; exit 1; }
echo "recompute: beside.json (read by nothing) re-derives byte for byte"
( cd "$here" && python3 -B compare_s2.py --selftest comparison-rule.toml apply_bprime.py ) > "$tmp/cselftest.out" || { echo "recompute: the comparison's selftest does not pass"; exit 1; }
tail -1 "$tmp/cselftest.out"
( cd "$here" && python3 -B compare_s2.py comparison-rule.toml apply_bprime.py stage2-record . decision-rule.toml ) > "$tmp/comparison.json" || { echo "recompute: the comparison applier failed"; exit 2; }
cmp -s "$tmp/comparison.json" "$here/comparison.json" || { echo "recompute: the comparison over this record and stage 2's does not reproduce comparison.json"; exit 1; }
echo "recompute: comparison.json (endpoint 2) re-derives byte for byte from this record and stage 2's committed files"
python3 - "$here" <<'PY' || exit 1
import json, pathlib, sys, tomllib, re
here = pathlib.Path(sys.argv[1])
word = json.loads((here / "word.json").read_text())["verdict"]
rows = [json.loads(l) for l in (here / "run.jsonl").read_text().splitlines() if l.strip()]
claims = [r for r in rows if r.get("record") == "claim"]
if not claims or any(c.get("result") != word for c in claims):
    print(f"recompute: the applier's word is {word!r} and the claim rows say {[c.get('result') for c in claims]}"); sys.exit(1)
pre = json.loads((here / "pre-registration.json").read_text())
# endpoint 2's row (#181's comparison kind): its word is comparison.json's, and its counts are
# comparison.json's rates times the shared forks, exact
from fractions import Fraction
cmp_ = json.loads((here / "comparison.json").read_text()); comps = [r for r in rows if r.get("record") == "comparison"]
counts = [Fraction(cmp_[k]) * cmp_["shared"] for k in ("rate_27B", "rate_S2")]
if any(c.denominator != 1 for c in counts): print("recompute: comparison.json's rates are not whole counts over the shared forks"); sys.exit(1)
want = [{"label": "the 27B (stage 2), imperative edits at 0.6 over the shared counted forks", "n": int(counts[0]), "of": cmp_["shared"]},
        {"label": "this substrate, imperative edits at 0.6 over the shared counted forks", "n": int(counts[1]), "of": cmp_["shared"]}]
row_ok = len(comps) == 1 and comps[0]["result"] == cmp_["word"] and comps[0]["predicted"] == "dependent" and comps[0]["counts"] == want \
    and comps[0]["hypothesis"] == pre["endpoints"]["2"] and [a["path"] for a in comps[0]["consumes"]] == ["comparison.json"]
if not row_ok:
    print(f"recompute: the comparison row is not endpoint 2's hypothesis, comparison.json's word {cmp_['word']!r} and counts {[w['n'] for w in want]} of {cmp_['shared']}, consuming comparison.json"); sys.exit(1)
front = tomllib.loads(re.match(r"\+\+\+\n(.*?)\n\+\+\+\n", (here / "README.md").read_text(encoding="utf-8"), re.S).group(1))
if front["hypothesis"] != pre["hypothesis"]: print("recompute: the front matter's hypothesis is not the pre-registration's"); sys.exit(1)
import hashlib
for p, want in (("decision-rule.toml", pre["rule"]["decision_rule_sha256"]), ("apply_bprime.py", pre["rule"]["applier_sha256"]), ("comparison-rule.toml", pre["rule"]["comparison_rule_sha256"]), ("compare_s2.py", pre["rule"]["comparison_applier_sha256"])):
    if hashlib.sha256((here / p).read_bytes()).hexdigest() != want: print(f"recompute: {p} is not the digest the pre-registration names"); sys.exit(1)
if (here / "window" / "run.rc").read_text().strip() != "0": print("recompute: the run's harness exit is not 0"); sys.exit(1)
print(f"recompute: word.json re-derives byte for byte from the committed record; the word, {word!r}, is the claim rows'; the comparison row is comparison.json's; the rules and appliers are the pre-registered digests")
PY
python3 - "$here" "word.json" <<'PY' || exit 1
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
