#!/usr/bin/env python3
"""What each check costs on CI, read from CI's own logs (#262).

    measure-check-seconds.py RUN_ID [RUN_ID ...]      fetch with `gh`, print the table
    measure-check-seconds.py --log FILE [--log FILE]  read saved job logs instead

Every check `verify.sh` runs prints `=== name ===` when it starts and
`--- name: PASS|FAIL` when it ends; GitHub stamps each log line with the time
it was written. A check's seconds in one run is the sum over that run's jobs
(a sharded check runs in several), and its row is the median over the runs
given. The output is `.github/check-seconds.tsv`'s body: check, seconds,
the date measured, and the runs read -- a measurement carries its source.

Never from a local run: a seat's machine is not the runner the budget is for.
"""

from __future__ import annotations

import collections
import datetime
import json
import re
import statistics
import subprocess
import sys

STAMP = re.compile(r"(\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d+)?)Z")
OPENS = re.compile(r"Z === (\w+) ===\s*$")
CLOSES = re.compile(r"Z --- (\w+): (?:PASS|FAIL)\b")


def seconds_in(log: str) -> dict[str, float]:
    """Each check's seconds in one job's log."""
    began: dict[str, datetime.datetime] = {}
    took: dict[str, float] = collections.defaultdict(float)
    for line in log.splitlines():
        stamp = STAMP.search(line)
        if not stamp:
            continue
        at = datetime.datetime.fromisoformat(stamp.group(1)[:26])
        if opened := OPENS.search(line):
            began[opened.group(1)] = at
        elif (closed := CLOSES.search(line)) and closed.group(1) in began:
            took[closed.group(1)] += (at - began.pop(closed.group(1))).total_seconds()
    return dict(took)


def run_logs(run: str) -> list[str]:
    """Every job's log in one CI run, through `gh`."""
    jobs = json.loads(
        subprocess.run(
            ["gh", "run", "view", run, "--json", "jobs"], check=True, capture_output=True, text=True
        ).stdout
    )["jobs"]
    logs = []
    for job in jobs:
        done = subprocess.run(
            ["gh", "run", "view", run, "--log", "--job", str(job["databaseId"])],
            capture_output=True,
            text=True,
        )
        if done.returncode == 0:
            logs.append(done.stdout)
    return logs


def main(argv: list[str]) -> int:
    if not argv:
        print(__doc__, file=sys.stderr)
        return 2
    per_run: dict[str, dict[str, float]] = {}
    if argv[0] == "--log":
        files = argv[1::2]
        if argv[::2] != ["--log"] * len(files):
            print("measure-check-seconds: --log takes one file each", file=sys.stderr)
            return 2
        for path in files:
            with open(path, encoding="utf-8", errors="replace") as handle:
                per_run[path] = seconds_in(handle.read())
    else:
        for run in argv:
            totals: dict[str, float] = collections.defaultdict(float)
            for log in run_logs(run):
                for check, took in seconds_in(log).items():
                    totals[check] += took
            per_run[run] = dict(totals)
    checks = sorted({check for took in per_run.values() for check in took})
    today = datetime.date.today().isoformat()
    for check in checks:
        samples = [took[check] for took in per_run.values() if check in took]
        print(f"{check}\t{statistics.median(samples):.1f}\t{today}\t{','.join(per_run)}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
