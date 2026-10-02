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

Most rules read line-oriented facts out of the workflow files rather than
parsing YAML, because the standard library has no YAML parser. Rule 10 is the
exception: pages.yml, the deploy, is read through Ruby's YAML parser (psych),
since a text read of it was shown to be passable spelling by spelling (#256).

Python's standard library, plus `ruby` for rule 10. Exit 0 if the wiring is
sound, 1 if not, and 2 if `ruby` is missing, exits non-zero, or prints
something that is not the parser's answer -- a misuse named on stderr, never a
pass. A pages.yml that is not YAML is a failure of the file: exit 1.
"""

from __future__ import annotations

import collections
import json
import os
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
SECONDS_TABLE = pathlib.Path(__file__).resolve().parent.parent / ".github" / "check-seconds.tsv"

# Each check a declared split may name (#262). Its members are asked of
# verify.sh itself -- `VERIFY_LIST_MEMBERS=1 VERIFY_CHECK_SHARD=K/N
# ./verify.sh --only CHECK` -- through the same check function and helper
# a CI job runs, so the proof that the shards are complete covers the wiring
# from the job's env to the script, not only the script's own filter.
SHARDABLE = ("injections", "bsd", "recompute")

SHARD_MATRIX = re.compile(r"^\s+shard:\s*\$\{\{\s*fromJSON\(needs\.plan\.outputs\.shards\)\s*\}\}\s*$", re.M)


def owner_reads(owner: str) -> re.Pattern[str]:
    """The plan job's line reading `owner`'s shard count out of check-owners.tsv."""
    return re.compile(
        r"awk -F'\\t' -v pkg=" + re.escape(owner) + r" '!/\^#/ && NF>=3 && \$2 == pkg \{ print \$3 \}' \.github/check-owners\.tsv"
    )


# A check's declared shard count, from check-owners.tsv's third column.
SHARDS: dict[str, int] = {}

# How a sharded package's job tells its check which shard it is: the matrix
# entry over the count the plan job read from check-owners.tsv.
SHARD_ENV = re.compile(
    r"^\s+VERIFY_CHECK_SHARD:\s*\$\{\{\s*matrix\.shard\s*\}\}/\$\{\{\s*needs\.plan\.outputs\.count\s*\}\}\s*$",
    re.M,
)

# A check's declared shard count, from check-owners.tsv's third column.
SHARDS: dict[str, int] = {}
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


def members_listed(output: str, check: str) -> list[str]:
    """The member names a check printed between verify.sh's `=== check ===`
    and `--- check:` lines, under VERIFY_LIST_MEMBERS: one name per line,
    each a bare identifier, so the check's own diagnostics are not counted."""
    inside, names = False, []
    for line in output.splitlines():
        if line.strip() == f"=== {check} ===":
            inside = True
        elif line.startswith(f"--- {check}:"):
            inside = False
        elif inside and re.fullmatch(r"[A-Za-z0-9_][A-Za-z0-9_.-]*", line.strip()):
            names.append(line.strip())
    return names


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
        if len(parts) >= 3 and parts[2].strip():
            count = parts[2].strip()
            if not count.isdigit() or int(count) < 2:
                failures.append(f"{OWNERS}:{number}: `{check}`'s shard count is {count!r}, not a whole number of 2 or more")
            else:
                SHARDS[check] = int(count)
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


# pages.yml read through a YAML parser (rule 10, #256): Ruby's own, psych,
# which ships with Ruby on macOS and on ubuntu-latest. The tree is walked, not
# loaded -- no type is resolved, so `on:` stays the string it is to GitHub
# rather than YAML 1.1's `true` -- and what could make one reading of the file
# differ from another is refused outright, anywhere in it: more than one
# document, a key twice in one mapping (YAML keeps the last), and a tag,
# anchor, alias or merge key. Out comes JSON: the document, with every scalar
# a string, and the refusals.
PSYCH_WALK = r"""
require "psych"
require "json"
problems = []
short = ->(n) { (n.respond_to?(:tag) && n.tag) ? n.tag.sub("tag:yaml.org,2002:", "!!") + " " : "" }
mark = ->(n) { (n.respond_to?(:anchor) && n.anchor) ? "&#{n.anchor} " : "" }
walk = lambda do |node, path|
  case node
  when Psych::Nodes::Alias
    problems << { "kind" => "unplain", "path" => path, "detail" => "*#{node.anchor}" }
    nil
  when Psych::Nodes::Scalar
    problems << { "kind" => "unplain", "path" => path, "detail" => "#{mark.(node)}#{short.(node)}#{node.value}".strip } if node.tag || node.anchor
    node.value
  when Psych::Nodes::Sequence
    problems << { "kind" => "unplain", "path" => path, "detail" => "#{mark.(node)}#{short.(node)}[...]".strip } if node.tag || node.anchor
    node.children.each_with_index.map { |c, i| walk.(c, path + [i.to_s]) }
  when Psych::Nodes::Mapping
    problems << { "kind" => "unplain", "path" => path, "detail" => "#{mark.(node)}#{short.(node)}{...}".strip } if node.tag || node.anchor
    out = {}
    node.children.each_slice(2) do |k, v|
      unless k.is_a?(Psych::Nodes::Scalar)
        problems << { "kind" => "unplain", "path" => path, "detail" => "a key that is not a scalar" }
        next
      end
      if k.tag || k.anchor || k.value == "<<"
        problems << { "kind" => "unplain", "path" => path, "detail" => "#{mark.(k)}#{short.(k)}#{k.value}".strip }
      end
      problems << { "kind" => "duplicate", "path" => path, "key" => k.value } if out.key?(k.value)
      out[k.value] = walk.(v, path + [k.value])
    end
    out
  end
end
begin
  stream = Psych.parse_stream(File.read(ARGV[0]))
rescue Psych::SyntaxError => e
  puts JSON.dump({ "error" => e.message })
  exit 0
end
docs = stream.children
problems << { "kind" => "documents", "count" => docs.size } if docs.size != 1
puts JSON.dump({ "doc" => docs.empty? ? nil : walk.(docs[0].root, []), "problems" => problems })
"""


class ParserMissing(Exception):
    """The parser rule 10 reads pages.yml with is not here: a misuse, not a pass."""


def parse_workflow(path: pathlib.Path) -> tuple[object, list[str]]:
    """pages.yml as parsed, and why it may not be read as one plain document."""
    if path.read_bytes().startswith(b"\xef\xbb\xbf"):
        return None, ["pages.yml: begins with a byte-order mark, which the parser refuses; save it without one"]
    try:
        done = subprocess.run(["ruby", "-e", PSYCH_WALK, str(path)], capture_output=True, text=True)
    except FileNotFoundError as err:
        raise ParserMissing("rule 10 reads pages.yml with Ruby's YAML parser (psych), and `ruby` was not found") from err
    if done.returncode != 0:
        raise ParserMissing(f"rule 10's YAML parser (ruby, psych) failed (exit {done.returncode}): {done.stderr.strip()[:300]}")
    try:
        out = json.loads(done.stdout)
    except json.JSONDecodeError as err:
        raise ParserMissing(f"rule 10's YAML parser (ruby, psych) answered with something that is not its JSON: {done.stdout.strip()[:120]!r}") from err
    if not isinstance(out, dict) or not ("error" in out or ("doc" in out and isinstance(out.get("problems"), list))):
        raise ParserMissing(f"rule 10's YAML parser (ruby, psych) answered with JSON that is not its answer: {done.stdout.strip()[:120]!r}")
    if "error" in out:
        return None, [f"pages.yml: is not YAML: {out['error']}"]
    refusals = []
    for p in out["problems"]:
        where = ".".join(p.get("path", [])) or "the top level"
        if p["kind"] == "documents":
            refusals.append(f"pages.yml: holds {p['count']} YAML documents, not one")
        elif p["kind"] == "duplicate":
            refusals.append(f"pages.yml: {where}: duplicate key `{p['key']}`; YAML keeps the last, the eye reads the first")
        elif p["path"] == ["jobs", "deploy"]:
            refusals.append(f"pages.yml: the deploy job carries a key not spelled plainly ({p['detail']})")
        else:
            refusals.append(f"pages.yml: {where}: a tag, anchor, alias or merge key ({p['detail']})")
    return out["doc"], refusals


def newest_run_step_verdicts(script: str) -> list[str]:
    """Runs pages.yml's newest-run step, as parsed, against a stub `gh` (rule 10)."""
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
    #    (#244). Read through a YAML parser (#256): what is checked is what
    #    YAML reads -- a guard commented out is no guard, and a second job,
    #    document or spelling cannot show the eye one workflow and GitHub
    #    another. And the two parts it downloads must be uploaded by
    #    something the gate runs.
    pages = WORKFLOWS / "pages.yml"
    if pages.is_file():
        # Read through a YAML parser (#256), not as text: what is checked is
        # what YAML reads, so no spelling -- a second job, a flow collection
        # running past a block, a key hidden in a block scalar -- can show
        # the text one workflow and GitHub another.
        doc, refusals = parse_workflow(pages)
        failures += refusals
        doc = doc if isinstance(doc, dict) else {}
        on = doc.get("on") if isinstance(doc.get("on"), dict) else {}
        run_of = on.get("workflow_run") if isinstance(on.get("workflow_run"), dict) else {}
        if run_of.get("workflows") != ["verify"]:
            failures.append("pages.yml: does not run downstream of the `verify` workflow (workflow_run of verify)")
        # Exactly: a run completed, on main. Without the branch filter a
        # passing push run on any branch would publish (#258's review).
        elif run_of != {"workflows": ["verify"], "types": ["completed"], "branches": ["main"]}:
            failures.append(f"pages.yml: the workflow_run trigger is not exactly verify's runs completed on main (found {json.dumps(run_of, sort_keys=True)})")
        if set(on) - {"workflow_run"}:
            failures.append("pages.yml: publishes on a trigger of its own, not only on a `verify` run's completion")
        if "concurrency" in doc:
            failures.append("pages.yml: a workflow-level concurrency group, which a run whose deploy is skipped still enters, cancelling a waiting deploy (#244)")
        for key in ("defaults", "env"):
            if key in doc:
                failures.append(f"pages.yml: workflow-level `{key}`, which can change how every step runs")
        jobs = doc.get("jobs") if isinstance(doc.get("jobs"), dict) else {}
        deploy = jobs.get("deploy") if isinstance(jobs.get("deploy"), dict) else {}
        if not deploy:
            failures.append("pages.yml: no `deploy` job")
        # The deploy job is the only job: another could upload and publish
        # with none of the checks below (#258's review).
        if set(jobs) - {"deploy"}:
            failures.append(f"pages.yml: jobs other than `deploy` ({', '.join(sorted(set(jobs) - {'deploy'}))}), which nothing here checks")
        # The deploy job's own keys, an allowlist: anything else -- a
        # container, a matrix, `env`, `defaults`, `needs` -- changes how or
        # where its steps run, or what they see.
        extra = set(deploy) - {"name", "if", "concurrency", "runs-on", "environment", "steps", "timeout-minutes"}
        if extra:
            failures.append(f"pages.yml: the deploy job carries {', '.join(sorted(extra))}, which can change how or where its steps run")
        guards = {
            "github.event.workflow_run.conclusion == 'success'": "pages.yml: deploys on a workflow_run whatever its conclusion",
            "github.event.workflow_run.event == 'push'": "pages.yml: deploys on a run that was not a push",
            "github.event.workflow_run.head_repository.full_name == github.repository": "pages.yml: deploys a run of a fork's code",
        }
        condition = deploy.get("if") if isinstance(deploy.get("if"), str) else ""
        # `${{ ... }}` around the WHOLE `if:` is the same expression to
        # GitHub; any text outside it -- a space, or the newline a `|` or `>`
        # scalar ends with -- makes the value a non-empty string, which is
        # true whatever the guards say (#258's third review).
        wrapped = re.fullmatch(r"\$\{\{(.*)\}\}", condition, re.S)
        refused = not wrapped and "${{" in condition
        if refused:
            # Named once, as what it is: the guards may be there, but GitHub
            # never reads them, so "missing guard" would be the wrong story.
            failures.append(f"pages.yml: the deploy's condition has text outside its ${{{{ }}}}, which GitHub reads as a string, always true (found {condition!r})")
        condition = wrapped.group(1).strip() if wrapped else condition
        clauses = [c.strip() for c in condition.split("&&")] if condition and not refused else []
        for guard, message in guards.items():
            if guard not in clauses and not refused:
                failures.append(message)
        if condition and not refused and (set(clauses) - set(guards) or "||" in condition or "!" in condition):
            failures.append(f"pages.yml: the deploy's condition is not exactly its guards joined by && (found `{condition}`)")
        steps = [st for st in deploy.get("steps", []) if isinstance(st, dict)] if isinstance(deploy.get("steps"), list) else []
        uploads = [i for i, st in enumerate(steps) if str(st.get("uses", "")).lower().startswith("actions/upload-pages-artifact@")]
        if len(uploads) != 1:
            failures.append(f"pages.yml: the deploy job uploads with upload-pages-artifact {len(uploads)} times, not once")
        upload = uploads[0] if uploads else len(steps)
        check = next((i for i, st in enumerate(steps) if str(st.get("run", "")).strip() == "./verify.sh --site _site"), None)
        if check is None or check > upload:
            failures.append("pages.yml: upload-pages-artifact is not preceded by ./verify.sh --site _site")
        # The actions the deploy job may use, matched without regard to case
        # or version: no other action, and each of these where it belongs.
        uses = [str(st.get("uses", "")).lower().split("@", 1)[0] for st in steps]
        allowed = {"", "actions/checkout", "actions/download-artifact", "actions/configure-pages", "actions/upload-pages-artifact", "actions/deploy-pages"}
        if set(uses) - allowed:
            failures.append(f"pages.yml: the deploy job uses {', '.join(sorted(set(uses) - allowed))}, which nothing here checks")
        # The sha that run checked, checked out before the check: the site is
        # that sha's, and so are the admissions and tables it is checked
        # against (#258's review).
        checkouts = [i for i, u in enumerate(uses) if u == "actions/checkout"]
        checked_out = steps[checkouts[0]].get("with") if len(checkouts) == 1 else None
        if (
            len(checkouts) != 1 or (check is not None and checkouts[0] > check) or not isinstance(checked_out, dict)
            or checked_out.get("ref") != "${{ github.event.workflow_run.head_sha }}"
            or {k: v for k, v in checked_out.items() if k != "ref"} not in ({}, {"persist-credentials": "false"})
        ):
            failures.append("pages.yml: the site is not checked against the sha that run built (one actions/checkout, ref: the run's head_sha, before the check)")
        # Nothing between the check and the upload writes to the site: only
        # configure-pages may stand there (#258's review).
        if check is not None and uploads and any(u != "actions/configure-pages" for u in uses[check + 1:upload]):
            failures.append("pages.yml: a step between ./verify.sh --site _site and the upload, which can change the site after its check")
        # From the check on, nothing is skipped or made to run regardless:
        # an `if:` there (`always()`) publishes a site that failed its check.
        for i in range(check if check is not None else len(steps), len(steps)):
            if "if" in steps[i]:
                failures.append(f"pages.yml: a step from the check on carries `if:` ({steps[i].get('name') or steps[i].get('uses') or steps[i].get('run')}), so it can run whatever the check said")
        deploys = [i for i, u in enumerate(uses) if u == "actions/deploy-pages"]
        if len(deploys) != 1 or (uploads and deploys[0] < upload) or "with" in steps[deploys[0]]:
            failures.append("pages.yml: the deploy job does not deploy once, after the upload, the artifact it uploaded (one actions/deploy-pages, no `with`)")
        # A step that decides can be made not to: skipped, or run by another
        # shell than the one its script is written for (#258's review).
        for i, what in ((check, "check step"), (next((j for j, st in enumerate(steps) if st.get("name") == "Publish only the newest run the gate passed"), None), "newest-run step")):
            if i is not None and set(steps[i]) & {"if", "shell", "working-directory"}:
                failures.append(f"pages.yml: the {what} carries {', '.join(sorted(set(steps[i]) & {'if', 'shell', 'working-directory'}))}, so it may not run as written")
        if check is not None and "env" in steps[check]:
            failures.append("pages.yml: the check step carries env, so it may not run as written")
        # The newest-run step, by name: the query is the command that sets
        # `newest`, the run number its own env, and nothing makes it optional.
        at = next((i for i, st in enumerate(steps) if st.get("name") == "Publish only the newest run the gate passed"), None)
        step = steps[at] if at is not None else {}
        script = step.get("run") if isinstance(step.get("run"), str) else ""
        env = step.get("env") if isinstance(step.get("env"), dict) else {}
        query = r'^\s*newest="\$\(gh api "repos/\$\{GITHUB_REPOSITORY\}/actions/workflows/verify\.yml/runs\?branch=main&event=push&status=success&per_page=1"'
        if (
            at is None or at > upload
            or not re.search(query, script, re.M)
            or env != {"GH_TOKEN": "${{ github.token }}", "RUN_NUMBER": "${{ github.event.workflow_run.run_number }}"}
        ):
            failures.append("pages.yml: publishes without checking that no later verify run on main has passed")
        # And the step DECIDES as described: run under bash with a stub `gh`
        # answering a run number, it passes when no later run has passed (the
        # same number, or a lower one the list lags with) and refuses when one
        # has, or when the answer is empty.
        failures += newest_run_step_verdicts(script) if script else ["pages.yml: the newest-run step has no `run` to run against a stub gh"]
        # Deploys one at a time, through a group on the deploy JOB (#244),
        # exactly `group: pages` and `cancel-in-progress: false`: any other
        # key (`queue: max` keeps every waiting deploy rather than the newest)
        # is refused.
        group = deploy.get("concurrency")
        if group is None:
            failures.append("pages.yml: the deploy job holds no `pages` concurrency group, so deploys can overlap")
        elif not isinstance(group, dict):
            failures.append("pages.yml: the deploy job's concurrency is not a map of `group: pages` and `cancel-in-progress: false`")
        elif group.get("group") != "pages":
            failures.append("pages.yml: the deploy job holds no `pages` concurrency group, so deploys can overlap")
        elif group.get("cancel-in-progress") != "false":
            failures.append("pages.yml: the deploy job's `pages` group does not say cancel-in-progress: false, so a newer deploy may cancel one mid-publish")
        elif set(group) != {"group", "cancel-in-progress"}:
            failures.append(f"pages.yml: the deploy job's `pages` group carries keys beyond group and cancel-in-progress ({', '.join(sorted(set(group) - {'group', 'cancel-in-progress'}))})")
        if "continue-on-error" in deploy or any("continue-on-error" in st for st in steps):
            failures.append("pages.yml: a step may fail and the deploy go on (continue-on-error)")
        uploaded_path = steps[upload]["with"].get("path") if uploads and isinstance(steps[upload].get("with"), dict) else None
        if uploads and str(uploaded_path).rstrip("/") not in ("_site", "./_site"):
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

    # 12. a declared split is one check per package, passed to every shard
    #
    #    A sharded check runs alone in its package's jobs, each with
    #    VERIFY_CHECK_SHARD=K/N; a second check in that package would run once
    #    per shard, and a workflow that never passes the shard would run the
    #    whole check in every job -- the budget spent N times, not split.
    for check, count in sorted(SHARDS.items()):
        owner = table.get(check)
        if check not in SHARDABLE:
            failures.append(f"{OWNERS.name}: `{check}` declares {count} shards, and it is not a check that can be split by name ({', '.join(sorted(SHARDABLE))})")
            continue
        if sorted(c for c, o in table.items() if o == owner) != [check]:
            failures.append(f"{OWNERS.name}: `{check}` is sharded, so its package `{owner}` must own it alone")
        wf = WORKFLOWS / f"pkg-{owner}.yml"
        # The env line itself, on a line that is not a comment: a workflow
        # that only MENTIONS the shard passed when this read the whole text
        # (its own seeded case found that).
        live = "\n".join(l for l in wf.read_text(encoding="utf-8").splitlines() if not l.lstrip().startswith("#")) if wf.is_file() else ""
        if wf.is_file() and not SHARD_ENV.search(live):
            failures.append(f"{wf.name}: `{check}` is sharded and the workflow never passes VERIFY_CHECK_SHARD, so every job would run all of it")
        # And the shards it starts are the table's: the matrix is the plan
        # job's reading of this check's own row, so no shard can be left out
        # of the matrix while every rule above still reads the table.
        if wf.is_file() and not (SHARD_MATRIX.search(live) and owner_reads(owner).search(live)):
            failures.append(f"{wf.name}: `{check}`'s matrix is not the plan job's reading of its shard count from {OWNERS.name}, so the shards CI starts may not be the shards declared")

    # 13. a declared split is complete: every member runs in exactly one shard
    #
    #    Asked of the check itself, shard by shard, against what it runs
    #    unsplit. A member in no shard is a member nothing runs -- a test that
    #    cannot fail, which is #239's lesson -- and a member in two is the
    #    budget spent twice.
    for check, count in sorted(SHARDS.items()):
        if check not in SHARDABLE:
            continue
        listed = []
        for part in [None, *range(1, count + 1)]:
            env = {k: v for k, v in os.environ.items() if k not in ("VERIFY_CHECK_SHARD", "VERIFY_INJECTION_SCOPE")}
            env["VERIFY_LIST_MEMBERS"] = "1"
            if part is not None:
                env["VERIFY_CHECK_SHARD"] = f"{part}/{count}"
            done = subprocess.run(
                ["bash", str(VERIFY), "--only", check], cwd=VERIFY.parent, env=env, capture_output=True, text=True
            )
            shown = f"VERIFY_CHECK_SHARD={part}/{count} " if part else ""
            if done.returncode != 0:
                failures.append(f"`{check}`: {shown}verify.sh --only {check} listing its members exited {done.returncode}: {(done.stdout + done.stderr).strip()[-200:]}")
                listed = []
                break
            listed.append(members_listed(done.stdout, check))
        if not listed:
            continue
        whole, parts = listed[0], listed[1:]
        seen = collections.Counter(name for part in parts for name in part)
        missing = [name for name in whole if seen[name] == 0]
        twice = sorted(name for name, n in seen.items() if n > 1)
        stray = sorted(set(seen) - set(whole))
        if not whole:
            failures.append(f"`{check}` lists no member at all; a split of nothing is not a split")
        if missing:
            failures.append(f"`{check}`'s {count} shards run none of {len(missing)} member(s), so nothing runs them: {', '.join(missing[:5])}")
        if twice:
            failures.append(f"`{check}`'s shards run {len(twice)} member(s) more than once: {', '.join(twice[:5])}")
        if stray:
            failures.append(f"`{check}`'s shards run {len(stray)} member(s) the unsplit check does not: {', '.join(stray[:5])}")

    # 14. no package's measured seconds pass the budget (#262)
    #
    #    Read from .github/check-seconds.tsv, a measurement of CI's own logs
    #    with its runs named, never from this run: the verdict is the same on
    #    every run, and the next overage is a red line rather than a print.
    #    A sharded check counts its seconds divided by its shards.
    measured: dict[str, float] = {}
    if not SECONDS_TABLE.is_file():
        failures.append(f"{SECONDS_TABLE.name}: missing; a budget nobody measures against is a print")
    else:
        for number, line in enumerate(SECONDS_TABLE.read_text(encoding="utf-8").split("\n"), 1):
            if not line.strip() or line.startswith("#"):
                continue
            row = line.split("\t")
            if len(row) != 4 or not re.fullmatch(r"\d+(\.\d+)?", row[1]) \
                    or not re.fullmatch(r"\d{4}-\d\d-\d\d", row[2]) or not re.fullmatch(r"\d+(,\d+)*", row[3]):
                failures.append(f"{SECONDS_TABLE.name}:{number}: not `check<TAB>seconds<TAB>YYYY-MM-DD<TAB>run,run,...`")
                continue
            if row[0] in measured:
                failures.append(f"{SECONDS_TABLE.name}:{number}: `{row[0]}` is measured twice")
            measured[row[0]] = float(row[1])
        for check in checks:
            if check not in measured:
                failures.append(f"{SECONDS_TABLE.name}: `{check}` has no measured seconds, so no budget can hold its job")
        for check in sorted(set(measured) - set(checks)):
            failures.append(f"{SECONDS_TABLE.name}: `{check}` is measured, and verify.sh defines no such check")
    budget = declared.get("wall_clock_seconds", "")
    if measured and budget.isdigit():
        for owner in sorted(set(table.values())):
            owned = [c for c, o in table.items() if o == owner and c in measured]
            per_job = sum(measured[c] / SHARDS.get(c, 1) for c in owned)
            if per_job > int(budget):
                shown = ", ".join(f"{c} {measured[c]:g} s" + (f" / {SHARDS[c]}" if c in SHARDS else "") for c in sorted(owned))
                failures.append(
                    f"pkg-{owner}.yml: its checks' measured seconds per job are {per_job:.0f} s, past the "
                    f"{budget} s budget ({shown}); split it in {OWNERS.name}, never by dropping a check"
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
    try:
        sys.exit(main())
    except ParserMissing as err:
        print(f"check-ci-coverage: {err}", file=sys.stderr)
        sys.exit(2)
