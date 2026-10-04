#!/usr/bin/env python3
"""A fault found not red on a full run opens an issue against its check, and a
pull request touching that check is refused until it is closed (#112).

    selftest-drift.py open DIR     on a full run (the nightly or the release path, #369): every census under
                                   DIR is read for `not_red` rows, and each
                                   check with one gets ONE open issue, labelled
                                   `check:<CHECK>`, listing its faults
    selftest-drift.py block FILE   on a pull request: FILE lists the checks
                                   whose faults the PR re-proves, one per line
                                   (scope-selftest.py --checks-out); an open
                                   issue labelled `check:<name>` for any of
                                   them refuses the run, naming the issue
    selftest-drift.py block-scope REPORT FILES
                                   on a pull request that runs no selftest
                                   (#369): REPORT is what `pr-scope.py`
                                   printed for the diff, FILES the diff's
                                   paths as `git diff --name-only -z` writes
                                   them; the report's `checks:`
                                   line is the list, widened to EVERY check
                                   when FILES touch the selftest's machinery
                                   (ruled (c) on #369, 5976704493), and
                                   refused as `block` does
    selftest-drift.py --selftest   the fixtures, against a stand-in for `gh`

The label is the mechanism and the issue is the record (ruled on #112,
2026-09-27). One issue per CHECK, not per fault: the block is per check, and a
runner regression that greens a whole lane would otherwise file a hundred
issues and hit GitHub's rate limit half way. A check already open gets one
comment naming only the faults its issue does not mention yet, so the nightly
does not repeat itself.

A pull request that FIXES the drift re-proves the check and is refused like any
other. The procedure is the refusal's: close the issue, re-run the pull
request, and let the next full run reopen it if the fix did not hold.

Why the block exists when a re-proven fault would fail anyway: the fault a PR
touches may be inherited from a full run older than the drift, and the issue
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
                 "--description", "a seeded fault for this check was not seen red on a full run (#112)")

    def create_issue(self, title: str, label: str, body: str) -> str:
        return self.run("issue", "create", "--title", title, "--label", label, "--body", body).strip()


def title(check: str) -> str:
    return f"selftest: the {check} check has faults not red on a full run"


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
                gh.comment(issue["number"], f"Also not red on a full run{where}:\n\n{listing(new)}\nRun: {run_url or 'unknown'}\n")
                said.append(f"{check}: #{issue['number']} already open; {len(new)} more fault(s) added")
            else:
                said.append(f"{check}: #{issue['number']} already open and names every fault")
            continue
        body = (
            f"The full selftest{where} did not see these `{check}` faults red:\n\n"
            f"{listing(faults)}\n"
            f"Until this issue is closed, a pull request whose selftest re-proves any `{check}` "
            f"fault is refused (#112). A pull request that fixes this is refused too: close the "
            f"issue, re-run it, and the next full run reopens it if the fix did not hold.\n\n"
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


# A change is MACHINERY (#369, ruled (c)) when the diff touches verify.sh or a
# file scope-selftest.py's MACHINERY_FILES names: the scope plan this replaces
# on pull requests re-proved every check for it. Decided from the diff's
# paths, never from pr-scope's printed reason -- pr-scope prints one reason,
# and a protocol file (a workflow, CONTRIBUTING.md) outranks machinery there,
# so a change to gate-selftest.yml, or machinery beside a doc, reads as
# protocol (#369's review). pr-scope's own verdict is untouched; the widening
# is the refusal's.
def machinery_files() -> frozenset[str]:
    """scope-selftest.py's MACHINERY_FILES, read from it, not restated."""
    import importlib.util
    spec = importlib.util.spec_from_file_location(
        "scope_selftest", pathlib.Path(__file__).resolve().parent / "scope-selftest.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return frozenset(module.MACHINERY_FILES)


def is_machinery(files: list[str], machinery: frozenset[str]) -> bool:
    return "verify.sh" in files or bool(set(files) & machinery)


def scope_checks(report: str, files: list[str], every: list[str], machinery: frozenset[str]) -> list[str]:
    """The checks a pull request's drift refusal is asked about: pr-scope.py's
    `checks:` line, or `every` check when `files` touch the machinery. A
    report with no `checks:` line, or no files, is refused rather than read
    as none."""
    named = next((l[len("checks:"):] for l in report.splitlines() if l.startswith("checks:")), None)
    if named is None:
        raise ValueError("not pr-scope.py's report: no `checks:` line")
    if not files:
        raise ValueError("no changed files given, so nothing says whether this is machinery")
    if is_machinery(files, machinery):
        if not every:
            raise ValueError("a machinery change, and no list of every check to widen to")
        return sorted(every)
    return [c.strip() for c in named.split(",") if c.strip()]


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


@fixture("faults not red on a full run open one issue per check, labelled with it")
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


@fixture("a machinery change under an open drift issue on a check pr-scope did not name is refused by name (#369)")
def _machinery_widens():
    gh = FakeGh({"check:bsd": [{"number": 8, "title": title("bsd"), "url": "u/8"}]})
    every = ["bsd", "ci", "hygiene", "history", "results"]
    machinery = machinery_files()
    report = "material\nreason: it changes verify.sh, the gate itself\nchecks: hygiene, ci, history"
    found = blocking(scope_checks(report, ["verify.sh"], every, machinery), gh)
    if len(found) != 1 or "#8" not in found[0] or "check:bsd" not in found[0]:
        return f"a verify.sh change under an open bsd issue found {found}"
    # Machinery that pr-scope reports as protocol (#369's review): the
    # selftest's workflow alone, and a machinery file beside a doc.
    for files in ([".github/workflows/gate-selftest.yml"], ["scripts/gatelib.py", "CONTRIBUTING.md"]):
        report = "material\nreason: x is protocol (y)\nchecks: hygiene, history"
        if scope_checks(report, files, every, machinery) != sorted(every):
            return f"a machinery change reported as protocol was not widened: {files}"
    report = "material\nreason: results/x is in the gate's tree (results/)\nchecks: results, hygiene"
    if scope_checks(report, ["results/x/README.md"], every, machinery) != ["results", "hygiene"]:
        return "a change that is not machinery was widened"
    for bad in (("material\nreason: x", ["verify.sh"]), ("checks: hygiene", [])):
        try:
            scope_checks(bad[0], bad[1], every, machinery)
        except ValueError:
            continue
        return f"an unreadable input was read: {bad!r}"
    return None


@fixture("an open issue for a check the PR does not touch does not refuse it")
def _unrelated():
    gh = FakeGh({"check:ci": [{"number": 7, "title": title("ci")}]})
    found = blocking(["test", "lanes"], gh)
    return f"refused on {found}" if found else None


@fixture("the issue a full run opened is the issue that blocks the next PR")
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
    if not ((len(argv) == 2 and argv[0] in ("open", "block")) or (len(argv) == 3 and argv[0] == "block-scope")):
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
        if argv[0] == "block-scope":
            every = subprocess.run([sys.executable, str(pathlib.Path(__file__).resolve().parent / "scope-selftest.py"),
                                    "--list-checks"], capture_output=True, text=True)
            if every.returncode != 0:
                raise Broken(f"scope-selftest.py --list-checks exited {every.returncode}: {every.stderr.strip()}")
            files = [f for f in pathlib.Path(argv[2]).read_bytes().decode("utf-8").split("\0") if f]
            checks = scope_checks(target.read_text(encoding="utf-8"), files, every.stdout.split(), machinery_files())
        else:
            checks = [c.strip() for c in target.read_text(encoding="utf-8").splitlines() if c.strip()]
        found = blocking(checks, gh)
    except (Broken, OSError, ValueError) as err:
        print(f"selftest-drift: {err}", file=sys.stderr)
        return EXIT_BROKEN
    if found:
        print("selftest-drift: this pull request touches or re-proves a check with an open drift issue from a full run. "
              "If this pull request is the fix, close the issue and re-run it; the next full run "
              "reopens it if the fix did not hold. Open:", file=sys.stderr)
        for line in found:
            print(f"  {line}", file=sys.stderr)
        return EXIT_REFUSED
    print(f"selftest-drift: no open drift issue for the {len(checks)} check(s) this pull request touches or re-proves")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
