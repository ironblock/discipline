#!/usr/bin/env python3
"""Require every CI job the gate depends on to have SUCCEEDED.

Reads the `needs` context as JSON on stdin or in $NEEDS.

The distinction this exists to make: a skipped job is not a failed job.
GitHub's `!failure()` and `!cancelled()` are both TRUE for a skipped job, so an
aggregator written with either passes when its dependencies never ran. The only
safe test is equality with the literal string 'success'.

Anything but 'success' -- 'failure', 'cancelled', 'skipped', or a result that
is missing entirely -- fails here. An empty `needs` fails too: a gate that
depends on nothing gates nothing.

ONE DECLARED EXCEPTION (#369): the `selftest` job runs only on the events
`.github/gate-budget.tsv`'s `selftest_events` names (the nightly and the
release path, until the gate redesign), so on every other event it is
skipped by its own `if:`. A skipped `selftest` is accepted on such an event
and on no other: on an event the row declares ON, a skipped selftest is a
selftest that did not run, and fails like any skipped job. The event comes
from $EVENT_NAME, $BASE_REF and $REF_NAME; with no event given, nothing is
accepted. No other job is ever accepted skipped.

Exit 0 if every job succeeded, 1 otherwise.
"""

from __future__ import annotations

import json
import os
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import gatelib  # noqa: E402

SUCCESS = "success"
ROOT = pathlib.Path(__file__).resolve().parent.parent
SELFTEST_JOB = "selftest"


def selftest_declared_off() -> bool:
    """Whether this run's event is one `selftest_events` declares off, read
    from the event the gate job passes and the two tables. Anything unreadable
    -- no event, no row, a row nothing reads -- is not off."""
    event = os.environ.get("EVENT_NAME", "")
    if not event:
        return False
    try:
        events = gatelib.selftest_events(gatelib.table_value(ROOT / ".github" / "gate-budget.tsv", "selftest_events"))
        release = gatelib.table_value(ROOT / ".github" / "branches.tsv", "release_branch")
    except OSError:
        return False
    if events is None or not release:
        return False
    return not gatelib.selftest_runs(events, release, event, os.environ.get("BASE_REF", ""), os.environ.get("REF_NAME", ""))


def main() -> int:
    raw = os.environ.get("NEEDS") or sys.stdin.read()
    try:
        needs = json.loads(raw)
    except ValueError as err:
        print(f"::error::the needs context is not JSON: {err}", file=sys.stderr)
        return 1
    if not isinstance(needs, dict):
        print(f"::error::the needs context is a {type(needs).__name__}, not an object",
              file=sys.stderr)
        return 1
    if not needs:
        print("::error::the gate depends on no jobs, so it gates nothing", file=sys.stderr)
        return 1

    off = selftest_declared_off()
    bad = {
        name: (job or {}).get("result", "<no result>")
        for name, job in needs.items()
        if not isinstance(job, dict) or job.get("result") != SUCCESS
    }
    if off and bad.get(SELFTEST_JOB) == "skipped":
        del bad[SELFTEST_JOB]
        print(f"check-job-results: '{SELFTEST_JOB}' skipped, on an event gate-budget.tsv's "
              f"selftest_events declares off (#369)")
    for name, result in sorted(bad.items()):
        print(f"::error::job '{name}' finished '{result}', not '{SUCCESS}'", file=sys.stderr)

    print(f"check-job-results: {len(needs)} job(s) required; {len(bad)} did not succeed")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
