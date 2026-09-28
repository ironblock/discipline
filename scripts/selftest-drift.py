#!/usr/bin/env python3
"""A fault found not red on `main` opens an issue against its check, and a
pull request touching that check is refused until it is closed (#112).

    selftest-drift.py open DIR     on `main` and the nightly: every census under
                                   DIR is read for `not_red` rows, and each
                                   check with one gets ONE open issue, labelled
                                   `check:<CHECK>`, listing its faults
    selftest-drift.py block FILE   on a pull request: FILE lists the checks
                                   whose faults the PR re-proves, one per line
                                   (scope-selftest.py --checks-out); an open
                                   issue labelled `check:<name>` for any of
                                   them refuses the run, naming the issue
    selftest-drift.py --selftest   the fixtures, against a stand-in for `gh`

The label is the mechanism and the issue is the record (ruled on #112,
2026-09-27). One issue per CHECK, not per fault: the block is per check, and a
runner regression that greens a whole lane would otherwise file a hundred
issues and hit GitHub's rate limit half way. A check already open gets one
comment naming only the faults its issue does not mention yet, so the nightly
does not repeat itself.

A pull request that FIXES the drift re-proves the check and is refused like any
other. The procedure is the refusal's: close the issue, re-run the pull
request, and let the next `main` run reopen it if the fix did not hold.

Why the block exists when a re-proven fault would fail anyway: the fault a PR
touches may be inherited from a `main` run older than the drift, and the issue
names the cause where the selftest would only name the symptom.

Exit 0 when done or nothing is open; 1 when `block` refuses; 2 when `gh`
cannot answer or the input is not what this script writes.
"""

from __future__ import annotations

import json
import os
import pathlib
import subprocess
import sys

EXIT_REFUSED = 1
EXIT_BROKEN = 2
LABEL_COLOR = "b60205"


class Broken(Exception):
    """`gh` could not answer, or the input is unreadable."""


class Gh:
    """The one door to GitHub. Fixtures replace it."""

    def __init__(self, repo: str | None) -> None:
        self.repo = repo

    def run(self, *args: str) -> str:
        cmd = ["gh", *args] + (["--repo", self.repo] if self.repo else [])
        done = subprocess.run(cmd, capture_output=True, text=True)
        if done.returncode != 0:
            raise Broken(f"`gh {' '.join(args[:2])}` exited {done.returncode}: {done.stderr.strip()}")
        return done.stdout

    def open_issues(self, label: str) -> list[dict]:
        out = self.run("issue", "list", "--state", "open", "--label", label,
                       "--limit", "100", "--json", "number,title,url,body")
        return json.loads(out or "[]")

    def comment(self, number: int, body: str) -> None:
        self.run("issue", "comment", str(number), "--body", body)

    def ensure_label(self, label: str) -> None:
        self.run("label", "create", label, "--force", "--color", LABEL_COLOR,
                 "--description", "a seeded fault for this check was not seen red on main (#112)")

    def create_issue(self, title: str, label: str, body: str) -> str:
        return self.run("issue", "create", "--title", title, "--label", label, "--body", body).strip()


def title(check: str) -> str:
    return f"selftest: the {check} check has faults not red on main"


LISTED = 50


def listing(faults: list[tuple[str, str]]) -> str:
    """The faults as Markdown lines, at most LISTED of them and a count."""
    lines = [f"- `{ident}`: {why}" for ident, why in faults[:LISTED]]
    if len(faults) > LISTED:
        lines.append(f"- and {len(faults) - LISTED} more; the run's census names every one")
    return "\n".join(lines) + "\n"


def read_not_red(directory: pathlib.Path) -> tuple[list[tuple[str, str, str]], str | None]:
    """(every `not_red` row as (id, check, why), the commit they are about)."""
    rows: list[tuple[str, str, str]] = []
    commit = None
    for path in sorted(p for p in directory.rglob("*") if p.is_file()):
        for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            parts = line.split("\t")
            if parts[0] == "commit" and len(parts) == 2:
                commit = parts[1].strip()
            elif parts[0] == "not_red":
                if len(parts) != 4 or not parts[1] or not parts[2]:
                    raise Broken(f"{path}:{number}: not `not_red<TAB>ID<TAB>CHECK<TAB>WHY`")
                rows.append((parts[1], parts[2], parts[3]))
    return rows, commit


def open_issues_for(rows, commit, gh: Gh, run_url: str) -> list[str]:
    """One open issue per check with a fault not red; say what was done."""
    by_check: dict[str, list[tuple[str, str]]] = {}
    for ident, check, why in sorted(set(rows)):
        by_check.setdefault(check, []).append((ident, why))
    said = []
    where = f" at {commit}" if commit else ""
    for check, faults in sorted(by_check.items()):
        label = f"check:{check}"
        gh.ensure_label(label)
        existing = gh.open_issues(label)
        if existing:
            issue = existing[0]
            new = [(i, w) for i, w in faults if f"`{i}`" not in (issue.get("body") or "")]
            if new:
                gh.comment(issue["number"], f"Also not red on `main`{where}:\n\n{listing(new)}\nRun: {run_url or 'unknown'}\n")
                said.append(f"{check}: #{issue['number']} already open; {len(new)} more fault(s) added")
            else:
                said.append(f"{check}: #{issue['number']} already open and names every fault")
            continue
        body = (
            f"The full selftest on `main`{where} did not see these `{check}` faults red:\n\n"
            f"{listing(faults)}\n"
            f"Until this issue is closed, a pull request whose selftest re-proves any `{check}` "
            f"fault is refused (#112). A pull request that fixes this is refused too: close the "
            f"issue, re-run it, and the next `main` run reopens it if the fix did not hold.\n\n"
            f"Run: {run_url or 'unknown'}\n"
        )
        said.append(f"{check}: opened {gh.create_issue(title(check), label, body)} for {len(faults)} fault(s)")
    return said


def blocking(checks: list[str], gh: Gh) -> list[str]:
    """Every open `check:<name>` issue for the checks listed."""
    found = []
    for check in sorted(set(checks)):
        for issue in gh.open_issues(f"check:{check}"):
            found.append(f"#{issue['number']} {issue['title']} ({issue.get('url', '')}) [check:{check}]")
    return found


def run_url() -> str:
    server, repo, run = (os.environ.get(k) for k in ("GITHUB_SERVER_URL", "GITHUB_REPOSITORY", "GITHUB_RUN_ID"))
    return f"{server}/{repo}/actions/runs/{run}" if server and repo and run else ""


# --------------------------------------------------------------------------
# fixtures
# --------------------------------------------------------------------------

class FakeGh(Gh):
    def __init__(self, issues=None) -> None:
        super().__init__(None)
        self.issues = issues or {}          # label -> [issue]
        self.created: list[tuple[str, str]] = []
        self.comments: list[tuple[int, str]] = []
        self.labels: list[str] = []

    def open_issues(self, label):
        return list(self.issues.get(label, []))

    def ensure_label(self, label):
        self.labels.append(label)

    def comment(self, number, body):
        self.comments.append((number, body))

    def create_issue(self, title, label, body):
        number = 900 + len(self.created)
        self.created.append((title, label))
        self.issues.setdefault(label, []).append({"number": number, "title": title, "url": f"u/{number}", "body": body})
        return f"u/{number}"


FIXTURES: list = []


def fixture(name):
    def take(fn):
        FIXTURES.append((name, fn))
        return fn
    return take


def _census(box: pathlib.Path, text: str) -> pathlib.Path:
    path = box / "selftest-census-3" / "census.tsv"
    path.parent.mkdir(parents=True)
    path.write_text(text, encoding="utf-8")
    return box


@fixture("faults not red on main open one issue per check, labelled with it")
def _opens():
    import tempfile
    with tempfile.TemporaryDirectory() as box:
        rows, commit = read_not_red(_census(pathlib.Path(box),
            "shard\t3\ncommit\tabc1234\nordinal\t4\tci.x\n"
            "not_red\tci.x\tci\tthe gate did not fire\nnot_red\tci.y\tci\tred for the wrong reason\n"
            "not_red\tresults.z\tresults\tthe fixture did not fail\n"))
    gh = FakeGh()
    open_issues_for(rows, commit, gh, "run")
    if gh.created != [(title("ci"), "check:ci"), (title("results"), "check:results")]:
        return f"created {gh.created}"
    body = gh.issues["check:ci"][0]["body"]
    if "`ci.x`" not in body or "`ci.y`" not in body or "abc1234" not in body:
        return f"the ci issue does not name both faults and the commit: {body!r}"
    if gh.labels != ["check:ci", "check:results"]:
        return f"labels made {gh.labels}, not once per check"
    return None


@fixture("a runner regression greening a whole lane files one issue, not one per fault")
def _flood():
    rows = [(f"isolation.f{n}", "lanes", "the gate did not fire") for n in range(123)]
    gh = FakeGh()
    open_issues_for(rows, "abc", gh, "run")
    if len(gh.created) != 1 or len(gh.labels) != 1:
        return f"{len(gh.created)} issue(s), {len(gh.labels)} label call(s) for one check"
    body = gh.issues["check:lanes"][0]["body"]
    listed = sum(1 for line in body.splitlines() if line.startswith("- `"))
    if listed != LISTED or "and 73 more" not in body:
        return f"listed {listed} fault(s), not {LISTED} and a count of the rest"
    return None


@fixture("a check already open is not opened again, and says only what is new")
def _dedup():
    gh = FakeGh({"check:ci": [{"number": 7, "title": title("ci"), "body": "- `ci.x`: why\n"}]})
    said = open_issues_for([("ci.x", "ci", "why")], "abc", gh, "run")
    if gh.created or gh.comments or "#7" not in said[0]:
        return f"repeated a fault already named: created {gh.created}, commented {gh.comments}"
    open_issues_for([("ci.x", "ci", "why"), ("ci.y", "ci", "why")], "abc", gh, "run")
    if gh.created or len(gh.comments) != 1:
        return f"a new fault of an open check: created {gh.created}, commented {len(gh.comments)} time(s)"
    number, body = gh.comments[0]
    if number != 7 or "`ci.y`" not in body or "`ci.x`" in body:
        return f"the comment on #{number} was {body!r}"
    return None


@fixture("a census with no not_red row opens nothing")
def _quiet():
    import tempfile
    with tempfile.TemporaryDirectory() as box:
        rows, _ = read_not_red(_census(pathlib.Path(box), "shard\t1\ncommit\tabc\nordinal\t1\tci.x\n"))
    gh = FakeGh()
    open_issues_for(rows, None, gh, "")
    return f"created {gh.created}" if gh.created else None


@fixture("a malformed not_red row is refused, not skipped")
def _malformed():
    import tempfile
    with tempfile.TemporaryDirectory() as box:
        try:
            read_not_red(_census(pathlib.Path(box), "not_red\tci.x\n"))
        except Broken:
            return None
    return "a two-field not_red row was read"


@fixture("an open issue for a check the PR re-proves refuses it, by name")
def _blocks():
    gh = FakeGh({"check:ci": [{"number": 7, "title": title("ci"), "url": "u/7"}]})
    found = blocking(["test", "ci"], gh)
    if len(found) != 1 or "#7" not in found[0]:
        return f"found {found}"
    return None


@fixture("an open issue for a check the PR does not touch does not refuse it")
def _unrelated():
    gh = FakeGh({"check:ci": [{"number": 7, "title": title("ci")}]})
    found = blocking(["test", "lanes"], gh)
    return f"refused on {found}" if found else None


@fixture("the issue opened on main is the issue that blocks the next PR")
def _round_trip():
    gh = FakeGh()
    open_issues_for([("results.y", "results", "the fixture did not fail")], "abc", gh, "run")
    found = blocking(["results"], gh)
    return None if len(found) == 1 else f"opened {gh.created}, then found {found}"


def selftest() -> int:
    broken = []
    for name, run in FIXTURES:
        try:
            why = run()
        except Exception as err:
            why = f"{type(err).__name__}: {err}"
        print(f"{'ok  ' if why is None else 'FAIL'}  {name}")
        if why is not None:
            broken.append((name, why))
    for name, why in broken:
        print(f"  {name}\n    {why}", file=sys.stderr)
    print(f"selftest-drift: {len(FIXTURES)} fixture(s), {len(broken)} failed",
          file=sys.stderr if broken else sys.stdout)
    return 1 if broken else 0


def main(argv: list[str]) -> int:
    if argv == ["--selftest"]:
        return selftest()
    if len(argv) != 2 or argv[0] not in ("open", "block"):
        print(__doc__.split("\n\n", 2)[1], file=sys.stderr)
        return EXIT_BROKEN
    gh = Gh(os.environ.get("GITHUB_REPOSITORY"))
    target = pathlib.Path(argv[1])
    try:
        if argv[0] == "open":
            if not target.is_dir():
                print(f"selftest-drift: no census directory at {target}; nothing was not red")
                return 0
            rows, commit = read_not_red(target)
            for line in open_issues_for(rows, commit, gh, run_url()):
                print(f"selftest-drift: {line}")
            print(f"selftest-drift: {len(set(rows))} fault(s) not red")
            return 0
        checks = [c.strip() for c in target.read_text(encoding="utf-8").splitlines() if c.strip()]
        found = blocking(checks, gh)
    except (Broken, OSError, ValueError) as err:
        print(f"selftest-drift: {err}", file=sys.stderr)
        return EXIT_BROKEN
    if found:
        print("selftest-drift: this pull request re-proves a check with an open drift issue on main. "
              "If this pull request is the fix, close the issue and re-run it; the next main run "
              "reopens it if the fix did not hold. Open:", file=sys.stderr)
        for line in found:
            print(f"  {line}", file=sys.stderr)
        return EXIT_REFUSED
    print(f"selftest-drift: no open drift issue for the {len(checks)} check(s) this pull request re-proves")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
