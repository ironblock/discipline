#!/usr/bin/env python3
"""A fault found not red on `main` opens an issue against its check, and a
pull request touching that check is refused until it is closed (#112).

    selftest-drift.py open DIR     on `main` and the nightly: every census under
                                   DIR is read for `not_red` rows, and each
                                   fault gets one open issue, titled by its id
                                   and labelled `check:<CHECK>`
    selftest-drift.py block FILE   on a pull request: FILE lists the checks
                                   whose faults the PR re-proves, one per line
                                   (scope-selftest.py --checks-out); an open
                                   issue labelled `check:<name>` for any of
                                   them refuses the run, naming the issue
    selftest-drift.py --selftest   the fixtures, against a stand-in for `gh`

The label is the mechanism and the issue is the record (ruled on #112,
2026-09-27). A fault already open is not opened again, so the nightly does
not file the same fault every night.

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
                       "--limit", "100", "--json", "number,title,url")
        return json.loads(out or "[]")

    def ensure_label(self, label: str) -> None:
        self.run("label", "create", label, "--force", "--color", LABEL_COLOR,
                 "--description", "a seeded fault for this check was not seen red on main (#112)")

    def create_issue(self, title: str, label: str, body: str) -> str:
        return self.run("issue", "create", "--title", title, "--label", label, "--body", body).strip()


def title(ident: str) -> str:
    return f"selftest: {ident} is not red on main"


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
    """Open one issue per fault not already open; say what was done."""
    said = []
    for ident, check, why in sorted(set(rows)):
        label = f"check:{check}"
        gh.ensure_label(label)
        existing = [i for i in gh.open_issues(label) if i.get("title") == title(ident)]
        if existing:
            said.append(f"{ident}: already open as #{existing[0]['number']}")
            continue
        body = (
            f"The seeded fault `{ident}` was not seen red by the full selftest on `main`"
            f"{f' at {commit}' if commit else ''}: {why}.\n\n"
            f"Its check is `{check}`. Until this issue is closed, a pull request whose "
            f"selftest re-proves any `{check}` fault is refused (#112).\n\n"
            f"Run: {run_url or 'unknown'}\n"
        )
        said.append(f"{ident}: opened {gh.create_issue(title(ident), label, body)}")
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
        self.labels: set[str] = set()

    def open_issues(self, label):
        return list(self.issues.get(label, []))

    def ensure_label(self, label):
        self.labels.add(label)

    def create_issue(self, title, label, body):
        number = 900 + len(self.created)
        self.created.append((title, label))
        self.issues.setdefault(label, []).append({"number": number, "title": title, "url": f"u/{number}"})
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


@fixture("a fault not red on main opens one issue labelled with its check")
def _opens():
    import tempfile
    with tempfile.TemporaryDirectory() as box:
        rows, commit = read_not_red(_census(pathlib.Path(box), "shard\t3\ncommit\tabc1234\nordinal\t4\tci.x\nnot_red\tci.x\tci\tthe gate did not fire\n"))
    gh = FakeGh()
    open_issues_for(rows, commit, gh, "run")
    if gh.created != [(title("ci.x"), "check:ci")]:
        return f"created {gh.created}"
    return None


@fixture("a fault already open is not opened again")
def _dedup():
    gh = FakeGh({"check:ci": [{"number": 7, "title": title("ci.x")}]})
    said = open_issues_for([("ci.x", "ci", "why")], "abc", gh, "run")
    if gh.created or "#7" not in said[0]:
        return f"opened a second issue: {gh.created}; said {said}"
    return None


@fixture("an open issue for another fault of the same check does not stand in for this one")
def _other_fault():
    gh = FakeGh({"check:ci": [{"number": 7, "title": title("ci.other")}]})
    open_issues_for([("ci.x", "ci", "why")], "abc", gh, "run")
    if gh.created != [(title("ci.x"), "check:ci")]:
        return f"ci.x was not opened while ci.other was open: {gh.created}"
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
    gh = FakeGh({"check:ci": [{"number": 7, "title": title("ci.x"), "url": "u/7"}]})
    found = blocking(["test", "ci"], gh)
    if len(found) != 1 or "#7" not in found[0]:
        return f"found {found}"
    return None


@fixture("an open issue for a check the PR does not touch does not refuse it")
def _unrelated():
    gh = FakeGh({"check:ci": [{"number": 7, "title": title("ci.x")}]})
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
    except (Broken, OSError, json.JSONDecodeError) as err:
        print(f"selftest-drift: {err}", file=sys.stderr)
        return EXIT_BROKEN
    if found:
        print("selftest-drift: this pull request re-proves a check with an open drift issue on main; "
              "close it (fix the fault on main) before this merges:", file=sys.stderr)
        for line in found:
            print(f"  {line}", file=sys.stderr)
        return EXIT_REFUSED
    print(f"selftest-drift: no open drift issue for the {len(checks)} check(s) this pull request re-proves")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
