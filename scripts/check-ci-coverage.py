#!/usr/bin/env python3
"""Lint the wiring between verify.sh's checks and the CI that runs them.

CI can go green while running almost nothing. The failure modes are specific
and each has bitten real projects:

  * A check exists that no workflow runs. CI is green; the check runs nowhere.
  * A package workflow exists that the root workflow never calls.
  * A called job the gate job does not depend on. Its failure cannot fail the
    build.
  * A `branches:` filter on a gate workflow's `pull_request` trigger. Pull
    requests against every other branch then have no run at all. (The same
    filter on `push` is the opposite: it stops a sha being gated twice, once
    per event, and skips nothing a pull request covers.)
  * A `--scope` passed to verify.sh from a gating workflow. It narrows the
    test check to part of the suite, which is a gate running less than it says
    while reporting the same green.
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

# Which workflows gate is DERIVED, not listed: the root workflow and everything
# it calls. A workflow nobody waits on may filter by path, because a skip there
# cannot be mistaken for a pass. The boundary is therefore a class rather than
# a remembered exception -- a filtering workflow must not be reachable from the
# gate's `needs`, and adding one to the root workflow makes its filter a
# failure automatically.

CALLS = re.compile(r"^\s*uses:\s*\./\.github/workflows/([A-Za-z0-9._-]+)\s*$", re.MULTILINE)
JOB = re.compile(r"^  ([A-Za-z0-9_-]+):\s*$", re.MULTILINE)
JOBS_BLOCK = re.compile(r"^jobs:\s*$", re.MULTILINE)
NEEDS = re.compile(r"^\s*needs:\s*\[([^\]]*)\]\s*$", re.MULTILINE)
PATH_FILTER = re.compile(r"^\s*paths(-ignore)?:\s*$", re.MULTILINE)
WORKFLOW_CALL = re.compile(r"^\s*workflow_call:\s*$", re.MULTILINE)
SCOPE_FLAG = re.compile(r"--scope\b")
BRANCH_KEY = re.compile(r"^ {4,}branches:\s*(.*)$")
BRANCH_IGNORE = re.compile(r"^ {4,}branches-ignore:")
INLINE_LIST = re.compile(r"^\[([^\]]*)\]$")
LIST_ITEM = re.compile(r"^ {6,}-\s*(.+?)\s*$")
ON_BLOCK = re.compile(r"^on:\s*$", re.MULTILINE)
EVENT = re.compile(r"^  ([A-Za-z_][A-Za-z0-9_]*):")
BRANCH_FILTER = re.compile(r"^ {4,}branches(-ignore)?:")


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


def pull_request_branch_filter(text: str) -> str | None:
    """The branch filter under `pull_request:`, if the workflow carries one.

    A branch filter on `push:` is how the same sha stops being gated twice --
    once by the push event and once by the pull-request event -- and it skips
    nothing a pull request covers. The same filter under `pull_request:` is a
    different thing entirely: it leaves pull requests targeting any other
    branch with no run at all, which is the class `paths:` is banned for.

    Read line-wise, like the rest of this script, and scoped to the `on:`
    block so that a `branches:` key belonging to a job cannot be mistaken for
    one belonging to an event.
    """
    block = ON_BLOCK.search(text)
    if not block:
        return None
    event = None
    for line in text[block.end():].split("\n"):
        if line and not line[0].isspace():
            break                      # out of `on:` and into the next key
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        named = EVENT.match(line)
        if named:
            event = named.group(1)
            continue
        if event == "pull_request" and BRANCH_FILTER.match(line):
            return line.strip()
    return None


def push_filter(text: str) -> tuple[str, list[str]]:
    """How a workflow's `push:` trigger is scoped, and to what.

    Four states, because they are four different things and a rule that
    collapsed them would report the wrong one:

      "none"    no `push:` trigger at all -- the workflow never runs on a push
      "all"     a `push:` with no `branches:` key -- it runs on EVERY branch
      "named"   a `branches:` key; the list is what it names, possibly empty
      "ignore"  a `branches-ignore:` key, which names what is NOT gated

    Read line-wise like the rest of this script and scoped to the `on:` block,
    so a `branches:` key belonging to a job cannot be mistaken for one
    belonging to an event. Both spellings of a list are read -- inline
    `[a, b]` and the block form -- because a rule that saw only one of them
    would pass a workflow it had not actually read.
    """
    block = ON_BLOCK.search(text)
    if not block:
        return ("none", [])
    event = None
    state = "none"
    names: list[str] = []
    listing = False
    for line in text[block.end():].split("\n"):
        if line and not line[0].isspace():
            break                      # out of `on:` and into the next key
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        named = EVENT.match(line)
        if named:
            event = named.group(1)
            listing = False
            if event == "push" and state == "none":
                state = "all"          # present; a `branches:` key may follow
            continue
        if event != "push":
            continue
        if BRANCH_IGNORE.match(line):
            return ("ignore", [])
        key = BRANCH_KEY.match(line)
        if key:
            state, listing = "named", True
            inline = INLINE_LIST.match(key.group(1).strip())
            if inline:
                names = [b.strip().strip("\"'") for b in inline.group(1).split(",") if b.strip()]
                listing = False
            continue
        item = LIST_ITEM.match(line)
        if listing and item:
            names.append(item.group(1).strip().strip("\"'"))
    return (state, names)


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

    # 4. nothing reachable from the gate may filter by path, or narrow a check
    gating = {ROOT_WORKFLOW} | called
    for wf in sorted(WORKFLOWS.glob("*.yml")):
        text = wf.read_text(encoding="utf-8")
        filters = bool(PATH_FILTER.search(text))
        if filters and wf.name in gating:
            failures.append(
                f"{wf.name}: has a path filter and is reached from the gate's `needs`. "
                f"A skipped job is not a failed job, and a filtered-out workflow leaves "
                f"its required check pending forever"
            )
        # `verify.sh --only test --scope SPEC` runs a fraction of the tests. It
        # exists for the selftest's sandboxes, where one fault is being asked
        # about one gate, and `--selftest` passes it per case from inside. A
        # workflow that spells it is a gate running less than it says while
        # reporting the same green, which is path filtering wearing a
        # different hat.
        if SCOPE_FLAG.search(text) and wf.name in gating:
            failures.append(
                f"{wf.name}: passes `--scope` to verify.sh and is reached from the "
                f"gate's `needs`. That narrows the test check to part of the suite; "
                f"the selftest scopes its own sandboxes and CI must not"
            )

    # 5. no gating workflow may filter its pull-request trigger by branch
    for wf in sorted(WORKFLOWS.glob("*.yml")):
        if wf.name not in gating:
            continue
        found = pull_request_branch_filter(wf.read_text(encoding="utf-8"))
        if found:
            failures.append(
                f"{wf.name}: `pull_request` carries `{found}` and is reached from the "
                f"gate's `needs`. A pull request against any other branch would then "
                f"have no run at all, and its required checks would stay pending"
            )

    # 6. the branch a gating workflow gates on PUSH is the one the repository
    #    agrees is its trunk
    #
    #    This filter is load-bearing in a way the pull-request one is not: the
    #    sha that lands on the default branch is not a sha a pull request
    #    covers. `pull_request` grades `refs/pull/N/merge`, a preview computed
    #    at run time, and under a squash or rebase merge the commit that lands
    #    has no pull-request run at all. So the push-to-trunk run is what gates
    #    the tree that actually ships, and its trigger deciding nothing --
    #    deleted, or naming a branch that is not the trunk -- is a merged tree
    #    gated by nothing.
    #
    #    The trunk is not named here. It is named in the workflows, more than
    #    once, and this makes those copies hold each other: they must agree.
    #    A rename that changes all of them together is a deliberate reviewed
    #    act; a rename or a typo that changes one is what this catches. Same
    #    standard the tag vocabulary is held to, and stated rather than left
    #    to be discovered.
    #
    #    A bare `push:` is NOT a failure here. It fires on every branch, so it
    #    gates the trunk along with everything else; what it costs is a second
    #    run per sha, which is a different complaint than this rule's and not
    #    one it is entitled to make.
    named: dict[str, list[str]] = {}
    for wf in sorted(WORKFLOWS.glob("*.yml")):
        state, branches = push_filter(wf.read_text(encoding="utf-8"))
        if state == "named" and branches:
            named[wf.name] = branches
    for name in sorted(gating):
        wf = WORKFLOWS / name
        if not wf.is_file():
            continue
        body = wf.read_text(encoding="utf-8")
        state, branches = push_filter(body)
        if state == "none":
            if WORKFLOW_CALL.search(body):
                continue               # called; it runs on its caller's trigger
            failures.append(
                f"{name}: is reached from the gate's `needs` and has no `push:` "
                f"trigger. The commit that lands on the trunk is not one a pull "
                f"request covers -- a squash or rebase merge writes a sha no "
                f"pull-request run ever graded -- so nothing would gate the "
                f"tree that ships"
            )
        elif state == "ignore":
            failures.append(
                f"{name}: scopes its `push:` trigger with `branches-ignore:`, "
                f"which says what is not gated and leaves what is to be "
                f"inferred from the branches that happen to exist. Name the "
                f"trunk positively with `branches:` so this rule can check it"
            )
        elif state == "named" and not branches:
            failures.append(
                f"{name}: has a `push:` trigger whose `branches:` names nothing, "
                f"so it fires for no branch and the trunk is gated by no push run"
            )
    #    Corroboration is the only thing grading the NAME, and a check whose
    #    evidence can disappear without a word is the shape of the two defects
    #    this rule was written for. So the disappearance is refused rather than
    #    passed: with one namer left there is no second opinion about the
    #    trunk, and a typo in it reads exactly like a rename.
    if len(named) == 1 and set(named) & set(gating):
        failures.append(
            f"{sorted(named)[0]} is the only workflow naming the trunk, so nothing "
            f"corroborates the name it gates on and a typo in it is indistinguishable "
            f"from a rename. Name the trunk in a second workflow, or decide here that "
            f"one is enough and say why -- but not by deleting the other namers and "
            f"leaving this rule looking like it still grades something"
        )
    elif len(named) > 1:
        agreed = {tuple(sorted(v)) for v in named.values()}
        if len(agreed) != 1:
            failures.append(
                "the workflows disagree about which branch is the trunk: "
                + "; ".join(f"{k} says {v}" for k, v in sorted(named.items()))
                + ". They hold each other precisely so that a rename or a typo in "
                "one of them cannot quietly leave the trunk ungated"
            )

    # 7. every pkg-* workflow is callable and is actually called
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
