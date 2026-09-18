#!/usr/bin/env bash
# Gate 0 for this directory: every number the report states re-derives from the
# artefacts committed beside it.
#
# WHAT THIS DOES NOT DO, stated because an undeclared non-catch is the vacuous
# class: it does not re-run the bakeoff. `check-recompute.py` runs this script
# in a sandboxed copy of the directory, where the `diet` binary is not
# reachable, so the metrics themselves are not re-derived here -- what is
# re-derived is every digest and every count the record and the report state.
# A cache edited after the fact, a product edited after the fact, and a total
# that disagrees with the rows are all caught. A metric computed wrongly by a
# binary that is not here is not.
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")"

python3 - <<'PY'
import hashlib
import json
import pathlib
import sys
import tomllib

FENCE = "+++"

# 0 CLEAN, 1 FOUND SOMETHING, 2 COULD NOT RUN -- the repository's contract,
# and this script speaks all three since 2026-09-12. It used to exit 1 for
# everything, because `sys.exit("message")` does, so "the numbers do not
# re-derive" and "this directory cannot be read at all" were one code. Ruled:
# the census must be able to tell a recompute that failed from one that could
# not be attempted, and conflating them is how a gate reads "nothing wrong"
# when it means "did not look".
#
# The line: 2 when this script cannot reach a verdict, 1 when it reached one
# and the verdict is that the numbers do not re-derive. A consumed artefact
# that is not here is a 1 -- the answer is known, and it is no.
def cannot_run(message):
    print(f"recompute: {message}", file=sys.stderr)
    raise SystemExit(2)


def read(path):
    # A FILE THAT IS NOT HERE IS A SENTENCE, NOT A TRACEBACK. This script is
    # read by whoever is holding a directory that will not re-derive, and a
    # stack trace tells them which line of Python raised rather than which
    # artefact is missing.
    #
    # An earlier version of this comment claimed "no path out of this script
    # is an exception nobody wrote". THAT WAS FALSE and a fresh instance
    # proved it: only the file READ came through here, so a file that was
    # present and malformed went straight to `json.loads` or `tomllib.loads`
    # and out as a raw traceback. The parses are wrapped below now, and the
    # claim is not restated -- what holds it is the two cases in
    # `a_directory_this_script_cannot_read_is_a_two_not_a_traceback`.
    try:
        return pathlib.Path(path).read_text(encoding="utf-8")
    except OSError as err:
        cannot_run(f"{path} cannot be read: {err.strerror}")


text = read("README.md")
if not text.startswith(FENCE + "\n"):
    cannot_run("README.md does not open with +++ front-matter")
try:
    front = tomllib.loads(text.split(FENCE + "\n", 2)[1])
except (tomllib.TOMLDecodeError, IndexError) as err:
    cannot_run(f"README.md front-matter is not TOML: {err}")

rows = []
for number, line in enumerate(read("run.jsonl").splitlines(), start=1):
    if not line.strip():
        continue
    try:
        rows.append(json.loads(line))
    except json.JSONDecodeError as err:
        cannot_run(f"run.jsonl line {number} is not JSON: {err.msg}")
summary = next((row for row in rows if row.get("record") == "summary"), None)
if summary is None:
    cannot_run("run.jsonl has no summary row")


def digest(path):
    try:
        return hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest()
    except OSError as err:
        # An artefact the record consumed and that is not committed beside it
        # means the numbers cannot be re-derived. That is a refusal, and it
        # names the file, because the reader's next move is to go and find it.
        sys.exit(f"{path} is consumed by the record and is not here: {err.strerror}")


# The product, against the digest the report and the summary both state.
product = digest("report.json")
for where, stated in (("front-matter", front["product_sha256"]),
                      ("the summary row", summary["product_sha256"])):
    if stated != product:
        sys.exit(f"{where} states product_sha256 {stated}, the product hashes to {product}")

# Every consumed artefact, against the digest the record declares for it.
consumed = [
    artifact
    for row in rows
    if row.get("record") == "claim"
    for artifact in row.get("consumes", [])
]
# A CHECK OF NOTHING IS NOT A PASS, and gate 0 says so itself rather than
# leaving it to a linter one layer out. Ruled 2026-09-13.
#
# A directory whose claim consumes NOTHING re-derives every one of its zero
# artefacts and reports success, which is how a gate comes to run over nothing
# while reporting that it ran. Paired with `reproducible-by-config` it is a
# contradiction on its face: there is no evidence committed beside the record
# to reproduce it FROM.
#
# This is EXIT 1, not 2. The script read everything it needed and reached a
# verdict; the verdict is that this directory does not support the claim its
# front-matter makes. `check-results.py` refuses the same state through a
# different rule -- a claim that names no artefact could produce a bound but
# never a number -- so `verify.sh` was already red. It was `check-recompute.py`
# ALONE, which is the sandboxed way this script is actually run, that counted
# such a directory as "recomputed". Found by a fresh instance.
if not consumed and front.get("kind") == "reproducible-by-config":
    sys.exit(
        "this directory declares `reproducible-by-config` and its claim "
        "consumes nothing, so there is no evidence here to re-derive from; "
        "a recompute of zero artefacts is not a recompute"
    )

matched = 0
for artifact in consumed:
    found = digest(artifact["path"])
    if found != artifact["sha256"]:
        sys.exit(
            f"{artifact['path']} is declared {artifact['sha256']} and hashes to {found}"
        )
    matched += 1

# And the totals, against what was actually there to count.
for field, counted in (("targets_checked", len(consumed)), ("targets_matched", matched)):
    if summary[field] != counted:
        sys.exit(f"the summary says {field} is {summary[field]}, the rows hold {counted}")
    if front.get(field) not in (None, counted):
        sys.exit(f"the front-matter says {field} is {front[field]}, the rows hold {counted}")

print(f"{len(consumed)} artefact(s) and the product re-derive")
PY
