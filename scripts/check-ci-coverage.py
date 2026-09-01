#!/usr/bin/env python3
"""Lint the wiring between verify.sh's checks and the CI that runs them.

CI can go green while running almost nothing. The failure modes are specific
and each has bitten real projects:

  * A check exists that no workflow runs. CI is green; the check runs nowhere.
  * A package workflow exists that the root workflow never calls.
  * A called job the gate job does not depend on. Its failure cannot fail the
    build.
  * A `paths:` filter on a gate workflow. A skipped job is not a failed job:
    `!failure()` passes on skipped, and a whole workflow filtered out leaves
    its required check pending forever. Path filtering is therefore banned on
    anything that gates, and banned mechanically rather than by convention.

This reads line-oriented facts out of the workflow files rather than parsing
YAML, because the standard library has no YAML parser and this gate must not
need an install step to tell the truth.

Stdlib only. Exit 0 if the wiring is sound, 1 otherwise.
"""

from __future__ import annotations

import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
WORKFLOWS = ROOT / ".github" / "workflows"
OWNERS = ROOT / ".github" / "check-owners.tsv"
VERIFY = ROOT / "verify.sh"

ROOT_WORKFLOW = "verify.yml"

# Workflows that act on a merge rather than gating one. They may filter by
# path: nothing is waiting on their result, so a skip cannot be mistaken for a
# pass. Every other workflow file is a gate and may not filter.
NOT_GATES = {"pages.yml", "repo-metadata.yml"}

CALLS = re.compile(r"^\s*uses:\s*\./\.github/workflows/([A-Za-z0-9._-]+)\s*$", re.MULTILINE)
JOB = re.compile(r"^  ([A-Za-z0-9_-]+):\s*$", re.MULTILINE)
JOBS_BLOCK = re.compile(r"^jobs:\s*$", re.MULTILINE)
NEEDS = re.compile(r"^\s*needs:\s*\[([^\]]*)\]\s*$", re.MULTILINE)
PATH_FILTER = re.compile(r"^\s*paths(-ignore)?:\s*$", re.MULTILINE)
WORKFLOW_CALL = re.compile(r"^\s*workflow_call:\s*$", re.MULTILINE)


def declared_checks(failures: list[str]) -> list[str]:
    """The checks verify.sh itself names, asked of verify.sh rather than copied."""
    try:
        done = subprocess.run(
            ["bash", str(VERIFY), "--list"], capture_output=True, text=True, check=True
        )
    except (OSError, subprocess.CalledProcessError) as err:
        failures.append(f"{VERIFY}: cannot list checks: {err}")
        return []
    return [line.strip() for line in done.stdout.split("\n") if line.strip()]


def owners(failures: list[str]) -> dict[str, str]:
    if not OWNERS.is_file():
        failures.append(f"{OWNERS}: missing")
        return {}
    table: dict[str, str] = {}
    for number, line in enumerate(OWNERS.read_text(encoding="utf-8").split("\n"), 1):
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        parts = line.split("\t")
        if len(parts) < 2 or not parts[0].strip() or not parts[1].strip():
            failures.append(f"{OWNERS}:{number}: not `check<TAB>owner`")
            continue
        check, owner = parts[0].strip(), parts[1].strip()
        if check in table:
            failures.append(f"{OWNERS}:{number}: `{check}` is owned twice")
        table[check] = owner
    return table


def main() -> int:
    failures: list[str] = []

    checks = declared_checks(failures)
    table = owners(failures)

    if not checks:
        failures.append("verify.sh declares no checks; a CI of nothing is not a pass")

    # 1. every check has exactly one owner, and every owner names a real check
    for check in checks:
        if check not in table:
            failures.append(
                f"check `{check}` has no owner in {OWNERS.name}, so no workflow runs it"
            )
    for check in table:
        if check not in checks:
            failures.append(f"{OWNERS.name} owns `{check}`, which verify.sh does not define")

    root = WORKFLOWS / ROOT_WORKFLOW
    if not root.is_file():
        failures.append(f"{root}: missing")
        print(*failures, sep="\n", file=sys.stderr)
        return 1
    root_text = root.read_text(encoding="utf-8")

    # 2. every owner has a workflow, and the root workflow calls it
    called = set(CALLS.findall(root_text))
    for owner in sorted(set(table.values())):
        wf = f"pkg-{owner}.yml"
        if not (WORKFLOWS / wf).is_file():
            failures.append(f"owner `{owner}` has no {WORKFLOWS.name}/{wf}")
        elif wf not in called:
            failures.append(f"{wf} exists but {ROOT_WORKFLOW} never calls it")

    # 3. every job the root workflow defines is depended on by the gate job
    #
    # Scoped to the `jobs:` block: `push:` and `pull_request:` under `on:` carry
    # the same two-space indent as a job name and would otherwise be read as
    # jobs that the gate fails to depend on.
    block = JOBS_BLOCK.search(root_text)
    if not block:
        failures.append(f"{ROOT_WORKFLOW}: has no `jobs:` block")
        jobs: set[str] = set()
    else:
        jobs = set(JOB.findall(root_text[block.end():]))
    needs_lists = NEEDS.findall(root_text)
    if not needs_lists:
        failures.append(f"{ROOT_WORKFLOW}: no gate job with a `needs: [...]` list")
    else:
        needed = {n.strip() for n in needs_lists[-1].split(",") if n.strip()}
        gate_jobs = jobs - {"gate"}
        for job in sorted(gate_jobs - needed):
            failures.append(
                f"{ROOT_WORKFLOW}: job `{job}` is not in the gate's `needs`, so its "
                f"failure cannot fail the build"
            )
        for job in sorted(needed - gate_jobs):
            failures.append(f"{ROOT_WORKFLOW}: the gate needs `{job}`, which is not a job")

    # 4. nothing that gates may filter by path
    for wf in sorted(WORKFLOWS.glob("*.yml")):
        if wf.name in NOT_GATES:
            continue
        text = wf.read_text(encoding="utf-8")
        if PATH_FILTER.search(text):
            failures.append(
                f"{wf.name}: has a path filter. A skipped job is not a failed job, and "
                f"a filtered-out workflow leaves its required check pending forever"
            )

    # 5. every pkg-* workflow is callable and is actually called
    for wf in sorted(WORKFLOWS.glob("pkg-*.yml")) + sorted(WORKFLOWS.glob("gate-*.yml")):
        text = wf.read_text(encoding="utf-8")
        if not WORKFLOW_CALL.search(text):
            failures.append(f"{wf.name}: is not `on: workflow_call:`, so it cannot be composed")
        if wf.name not in called:
            failures.append(f"{wf.name}: exists but {ROOT_WORKFLOW} never calls it")

    for message in failures:
        print(message, file=sys.stderr)
    if failures:
        print(f"check-ci-coverage: {len(failures)} failure(s)", file=sys.stderr)
        return 1
    print(
        f"check-ci-coverage: {len(checks)} check(s) across "
        f"{len(set(table.values()))} package(s); every one is owned, called and gated"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
