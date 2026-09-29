#!/usr/bin/env bash
# Gate 0 for this directory: every number the report states re-derives from
# the artefacts committed beside it.
#
# The linter checks that the report agrees with the record's SUMMARY row. That
# is agreement between two things the same run wrote, and a summary is a claim
# about a run rather than a reading of it. This re-derives the numbers from the
# rows -- counts the turns, sums their prefill, hashes the product -- so a
# summary that says three turns over a record holding two is caught here and
# nowhere else.
#
# The contract every results directory implements: run from anywhere, exit 0
# if and only if the recorded numbers re-derive. Copy it and change what it
# derives; do not change what it means.
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")"

python3 - <<'PY'
import hashlib
import json
import pathlib
import sys
import tomllib

FENCE = "+++"

text = pathlib.Path("README.md").read_text(encoding="utf-8")
if not text.startswith(FENCE + "\n"):
    sys.exit("README.md does not open with +++ front-matter")
front = tomllib.loads(text.split(FENCE + "\n", 2)[1])

rows = [
    json.loads(line)
    for line in pathlib.Path("run.jsonl").read_text(encoding="utf-8").splitlines()
    if line.strip()
]
turns = [r for r in rows if r.get("record") == "turn"]

derived = {
    "turns": len(turns),
    "prefill_tokens_total": sum(r.get("prefill_tokens", 0) for r in turns),
    "product_sha256": hashlib.sha256(
        pathlib.Path("product.txt").read_bytes()
    ).hexdigest(),
}

wrong = [
    f"  {key}: the report states {front.get(key)!r}, the artefacts give {value!r}"
    for key, value in derived.items()
    if front.get(key) != value
]
if wrong:
    print("recompute: the report does not re-derive", file=sys.stderr)
    print("\n".join(wrong), file=sys.stderr)
    sys.exit(1)
print(f"recompute: {len(derived)} recorded value(s) re-derived from the artefacts")
PY
