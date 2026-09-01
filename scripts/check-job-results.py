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

Stdlib only. Exit 0 if every job succeeded, 1 otherwise.
"""

from __future__ import annotations

import json
import os
import sys

SUCCESS = "success"


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

    bad = {
        name: (job or {}).get("result", "<no result>")
        for name, job in needs.items()
        if not isinstance(job, dict) or job.get("result") != SUCCESS
    }
    for name, result in sorted(bad.items()):
        print(f"::error::job '{name}' finished '{result}', not '{SUCCESS}'", file=sys.stderr)

    print(f"check-job-results: {len(needs)} job(s) required; {len(bad)} did not succeed")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
