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
  * A NARROWING FLAG passed to verify.sh from a gating workflow: `--scope`,
    which runs part of the test suite, or `--range`, which scans part of the
    history. Either is a gate running less than it says while reporting the
    same green. They are refused by one rule and one table, because the next
    such flag must be refused by adding a row rather than by remembering.
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
import tempfile

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
# Flags that make verify.sh do LESS, and what each one narrows. A workflow
# that gates may spell none of them. One table so that a flag added to the
# script is refused here by a row, not by whoever remembers this rule exists.
NARROWING_FLAGS = (
    # `(?![-\w])`, so `--scope-plan` is not read as `--scope`. It is not a row
    # of its own: it is the selftest's ruled PR scoping (#112), and every
    # fault it does not re-prove is DECLARED in the census at the commit it
    # was last seen red, which is the thing a narrowing flag here hides.
    ("--scope", re.compile(r"--scope(?![-\w])"),
     "narrows the test check to part of the suite, or the injections check to "
     "one injection; the selftest scopes its own "
     "sandboxes and CI must not"),
    ("--range", re.compile(r"--range\b"),
     "narrows the history check to a slice somebody chose; the range CI must "
     "scan is the one its event names, and a chosen one is a gate reading past "
     "what was pushed"),
)
BRANCH_KEY = re.compile(r"^ {4,}branches:\s*(.*)$")
BRANCH_IGNORE = re.compile(r"^ {4,}branches-ignore:")
INLINE_LIST = re.compile(r"^\[([^\]]*)\]$")
LIST_ITEM = re.compile(r"^ {6,}-\s*(.+?)\s*$")
ON_BLOCK = re.compile(r"^on:\s*$", re.MULTILINE)
STEP_ITEM = re.compile(r"^\s*- [A-Za-z_-]+:")
APT = re.compile(r"\bapt(-get)?\b")
STEP_TIMEOUT = re.compile(r"^\s+timeout-minutes:\s*\d+\s*$")
BUDGET = pathlib.Path(__file__).resolve().parent.parent / ".github" / "gate-budget.tsv"
CANCEL_IN_PROGRESS = re.compile(r"^\s*cancel-in-progress:\s*(.+?)\s*$", re.MULTILINE)
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


def newest_run_step_verdicts(live: str) -> list[str]:
    """Runs pages.yml's newest-run step against a stub `gh` (rule 10)."""
    lines = live.splitlines()
    at = next((i for i, l in enumerate(lines) if l.strip() == "- name: Publish only the newest run the gate passed"), None)
    if at is None:
        return ["pages.yml: no step named 'Publish only the newest run the gate passed' to run against a stub gh"]
    run = next((i for i in range(at + 1, len(lines)) if lines[i].strip() == "run: |"), None)
    if run is None:
        return ["pages.yml: the newest-run step has no `run: |` block to run against a stub gh"]
    indent = len(lines[run]) - len(lines[run].lstrip()) + 2
    body = []
    for line in lines[run + 1:]:
        if line.strip() and len(line) - len(line.lstrip()) < indent:
            break
        body.append(line[indent:])
    script = "\n".join(body) + "\n"
    failures = []
    with tempfile.TemporaryDirectory() as box:
        stub = pathlib.Path(box) / "gh"
        # (what the API names as the newest passed run, this run, publish?)
        for newest, this, publishes in (
            ("998", "998", True),
            # The list can lag a run that has only just completed: a LOWER
            # number publishes. Without this row, `-eq` passes the rest.
            ("990", "998", True),
            ("1006", "998", False),
            ("", "998", False),
        ):
            stub.write_text(f"#!/bin/sh\necho '{newest}'\n", encoding="utf-8")
            stub.chmod(0o755)
            env = {"PATH": f"{box}:/usr/bin:/bin", "GITHUB_REPOSITORY": "o/r", "RUN_NUMBER": this, "GH_TOKEN": "stub"}
            done = subprocess.run(["bash", "-c", script], env=env, capture_output=True, text=True)
            if (done.returncode == 0) != publishes:
                said = "publishes" if done.returncode == 0 else f"refuses (exit {done.returncode})"
                failures.append(
                    f"pages.yml: the newest-run step {said} when the newest passed run is "
                    f"{newest or 'none'} and this run is {this}; it must {'publish' if publishes else 'refuse'}"
                )
    return failures


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
        # `verify.sh --only test --scope SPEC` runs a fraction of the tests;
        # `verify.sh --only history --range A..B` scans a fraction of the
        # history. Both exist for a caller who already knows what they are
        # asking about -- the selftest's sandboxes, and a person reproducing a
        # CI verdict on their own machine. A workflow that spells either is a
        # gate running less than it says while reporting the same green, which
        # is path filtering wearing a different hat.
        if wf.name in gating:
            for flag, pattern, why in NARROWING_FLAGS:
                if pattern.search(text):
                    failures.append(
                        f"{wf.name}: passes `{flag}` to verify.sh and is reached "
                        f"from the gate's `needs`. That {why}"
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

    # 8. a run on the trunk is never cancelled by the next push to it
    #
    #    The push-to-trunk run is the full selftest whose census every pull
    #    request's scope plan is read from (#112). `cancel-in-progress: true`
    #    keyed on the ref cancels it whenever a second merge lands inside its
    #    nineteen minutes -- 6 of 30 trunk pushes were, measured on #112 --
    #    and pull requests are then scoped against an older census. A pull
    #    request superseding its own run is the saving the key exists for, so
    #    an expression is allowed; the literal `true` is what is refused.
    # Every `cancel-in-progress` in the file (a job may carry its own), with a
    # trailing comment dropped and YAML's own spellings of true folded.
    literal = [m.group(1).split(" #", 1)[0].strip().strip("\"'").lower()
               for m in CANCEL_IN_PROGRESS.finditer(root_text)]
    if any(value in ("true", "yes", "on") for value in literal):
        failures.append(
            f"{ROOT_WORKFLOW}: `cancel-in-progress: true` cancels a push run on the "
            f"trunk when the next merge lands, and the next pull request is scoped "
            f"against the census of an older commit. Cancel pull-request runs only"
        )

    # 9. the budget file declares what CI is held to
    #
    #    `wall_clock_seconds` is what every run prints its cost against, and
    #    `max_shards` is the ceiling scope-selftest.py --matrix divides. Both
    #    used to be refused missing by derive-shards.py --check, which retired
    #    with the shard plan (#112); a budget file declaring nothing would
    #    otherwise leave every run printing "against no budget" and every
    #    run's matrix step -- pull request, push and nightly -- failing.
    declared: dict[str, str] = {}
    if BUDGET.is_file():
        for line in BUDGET.read_text(encoding="utf-8").splitlines():
            key, _, value = line.partition("\t")
            if key and not key.startswith("#"):
                declared[key.strip()] = value.strip()
    for key in ("wall_clock_seconds", "max_shards"):
        value = declared.get(key, "")
        if not (value.isdigit() and int(value) > 0):
            failures.append(
                f"{BUDGET.name} declares no {key} as a positive whole number "
                f"(found {value!r}); it is what CI is held to, and nothing else says it"
            )

    # 10. the site is published only from what the gate passed (#32 I3)
    #
    #    pages.yml runs downstream of a `verify` run and publishes only when
    #    that run SUCCEEDED, was a PUSH and ran this repository's code -- a
    #    branch filter matches a fork's `main` by name -- and only when no
    #    LATER verify run on main has passed (by run number, read through the
    #    API), so a re-run of an older push run cannot deploy over a newer one,
    #    while a later run still running does not block it. It checks the site
    #    with `./verify.sh --site _site` before it uploads, nothing lets a step
    #    fail and the job go on, and what it uploads is the site it checked.
    #    Deploys run one at a time through a `pages` group on the deploy JOB,
    #    never the workflow, where a skipped run cancels a waiting deploy
    #    (#244). Read from the lines that are not comments: a guard commented
    #    out is no guard. And the two parts it downloads must be uploaded by
    #    something the gate runs.
    pages = WORKFLOWS / "pages.yml"
    if pages.is_file():
        live = "\n".join(l for l in pages.read_text(encoding="utf-8").splitlines() if not l.lstrip().startswith("#"))
        if not re.search(r"^  workflow_run:\s*$", live, re.M) or not re.search(r"^\s+workflows: \[verify\]\s*$", live, re.M):
            failures.append("pages.yml: does not run downstream of the `verify` workflow (workflow_run of verify)")
        if re.search(r"^  (push|workflow_dispatch|schedule|pull_request|pull_request_target):", live, re.M):
            failures.append("pages.yml: publishes on a trigger of its own, not only on a `verify` run's completion")
        guards = {
            "github.event.workflow_run.conclusion == 'success'": "pages.yml: deploys on a workflow_run whatever its conclusion",
            "github.event.workflow_run.event == 'push'": "pages.yml: deploys on a run that was not a push",
            "github.event.workflow_run.head_repository.full_name == github.repository": "pages.yml: deploys a run of a fork's code",
        }
        conditions = re.findall(r"^    if: (.+?)\s*$", live, re.M)
        clauses = [c.strip() for c in conditions[0].split("&&")] if len(conditions) == 1 else []
        for guard, message in guards.items():
            if guard not in clauses:
                failures.append(message)
        if len(conditions) == 1 and (set(clauses) - set(guards) or "||" in conditions[0] or "!" in conditions[0]):
            failures.append(f"pages.yml: the deploy's condition is not exactly its guards joined by && (found `{conditions[0]}`)")
        upload_at = live.find("actions/upload-pages-artifact")
        check = re.search(r"^\s+run: \./verify\.sh --site _site\s*$", live, re.M)
        if upload_at != -1 and (not check or check.start() > upload_at):
            failures.append("pages.yml: upload-pages-artifact is not preceded by ./verify.sh --site _site")
        # The newest-run step, by name, read as ONE step: the query is its
        # command, the run number its own env, and nothing makes it optional.
        step = re.search(r"^      - name: Publish only the newest run the gate passed\n(.*?)(?=^      - |\Z)", live, re.M | re.S)
        query = r'^\s+newest="\$\(gh api "repos/\$\{GITHUB_REPOSITORY\}/actions/workflows/verify\.yml/runs\?branch=main&event=push&status=success&per_page=1"'
        if upload_at != -1 and (
            not step or step.start() > upload_at
            or not re.search(query, step.group(1), re.M)
            or not re.search(r"^\s+RUN_NUMBER: \$\{\{ github\.event\.workflow_run\.run_number \}\}\s*$", step.group(1), re.M)
        ):
            failures.append("pages.yml: publishes without checking that no later verify run on main has passed")
        if step and re.search(r"^        if:", step.group(1), re.M):
            failures.append("pages.yml: the newest-run step carries an `if:`, so it can be skipped and the deploy go on")
        # And the step DECIDES as described: run under bash with a stub `gh`
        # answering a run number, it passes when no later run has passed (the
        # same number, or a lower one the list lags with) and refuses when one
        # has, or when the answer is empty. Rule 10's text
        # checks above cannot see a comparison turned round; this can.
        failures += newest_run_step_verdicts(live)
        # Deploys one at a time, through a group on the deploy JOB (#244): a
        # workflow-level group admits the runs whose job is skipped, and one
        # of those cancels a deploy waiting in it.
        if re.search(r"""^(\?\s*)?["']?concurrency["']?\s*(:|$)""", live, re.M):
            failures.append("pages.yml: a workflow-level concurrency group, which a run whose deploy is skipped still enters, cancelling a waiting deploy (#244)")
        # Read inside the `deploy` job only: the same lines under another job
        # hold nothing back. The group is exactly `group: pages` and
        # `cancel-in-progress: false`; any other key (`queue: max` keeps every
        # waiting deploy rather than the newest) is refused.
        deploy = re.search(r"^  deploy:\s*\n(.*?)(?=^  \S|\Z)", live, re.M | re.S)
        group = deploy and re.search(r"^    concurrency:\s*\n((?:      .*\n?)*)", deploy.group(1), re.M)
        # One `concurrency` key in the job: YAML keeps the LAST of two.
        if deploy and len(re.findall(r"""^    (\?\s*)?["']?concurrency["']?\s*(:|$)""", deploy.group(1), re.M)) != 1:
            group = None
        keys = dict(re.findall(r"^      ([\w-]+):\s*(.*?)\s*$", group.group(1), re.M)) if group else {}
        if keys.get("group") != "pages":
            failures.append("pages.yml: the deploy job holds no `pages` concurrency group, so deploys can overlap")
        elif keys.get("cancel-in-progress") != "false":
            failures.append("pages.yml: the deploy job's `pages` group does not say cancel-in-progress: false, so a newer deploy may cancel one mid-publish")
        elif set(keys) != {"group", "cancel-in-progress"}:
            failures.append(f"pages.yml: the deploy job's `pages` group carries keys beyond group and cancel-in-progress ({', '.join(sorted(set(keys) - {'group', 'cancel-in-progress'}))})")
        if re.search(r"^\s+continue-on-error:", live, re.M):
            failures.append("pages.yml: a step may fail and the deploy go on (continue-on-error)")
        uploaded_path = re.search(r"actions/upload-pages-artifact@\S+\s*\n\s+with:\s*\n\s+path: (\S+)", live)
        if upload_at != -1 and (not uploaded_path or uploaded_path.group(1) != "_site"):
            failures.append("pages.yml: uploads something other than the _site it checked")
        uploaded = "".join((WORKFLOWS / wf).read_text(encoding="utf-8") for wf in sorted(gating) if (WORKFLOWS / wf).is_file())
        for part in ("site-replay", "site-ledger"):
            if not re.search(rf"^\s+name: {part}\s*$", uploaded, re.M):
                failures.append(f"pages.yml: publishes {part}, which no workflow the gate runs uploads")

    # 11. a step that installs from a package mirror is bounded (#222's run)
    #
    #    A hung mirror is not a test result. On 2026-10-01 four selftest shards
    #    sat in `apt-get update` until GitHub's six-hour job limit cancelled
    #    them. A step that runs `apt-get` therefore carries `timeout-minutes`,
    #    read off the step itself: its `- ` item and every line indented deeper,
    #    whatever key opens it. `apt` is held to it as well as `apt-get`. A job-level timeout would not do -- it bounds the
    #    whole job, and the selftest's own runtime is what it should measure.
    for wf in sorted(WORKFLOWS.glob("*.yml")):
        lines = wf.read_text(encoding="utf-8").splitlines()
        starts = [i for i, line in enumerate(lines) if STEP_ITEM.match(line)]
        for start in starts:
            # The step is its `- ` item and every line indented deeper than the
            # dash, so it ends at the next step, the next job, or the end of the
            # file -- never in a neighbour (#232's review: a span to the next
            # `- name:` credited a later job's timeout to this step).
            dash = len(lines[start]) - len(lines[start].lstrip())
            end = start + 1
            while end < len(lines):
                line = lines[end]
                if line.strip() and not line.lstrip().startswith("#") and len(line) - len(line.lstrip()) <= dash:
                    break
                end += 1
            step = lines[start:end]
            if any(APT.search(line) and not line.lstrip().startswith("#") for line in step) and not any(
                STEP_TIMEOUT.match(line) for line in step
            ):
                failures.append(
                    f"{wf.name}:{start + 1}: a step runs apt-get with no `timeout-minutes`; "
                    f"a hung mirror holds the job until GitHub's six-hour limit"
                )

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
