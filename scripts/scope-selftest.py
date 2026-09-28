#!/usr/bin/env python3
"""Decide which seeded faults a PR re-proves and which it inherits (#112).

A fault's redness is a property of a (target, catcher) pair, and it does not
change under a diff that touches neither. So a PR re-proves a fault only when
the diff reaches something that fault depends on, and INHERITS the rest --
declared in the census with the `main` commit at which each was last seen
red, never counted as passed.

    scope-selftest.py --base REF --census DIR --out PLAN
    verify.sh --selftest --scope-plan PLAN [--shard K/N] [--census ...]

`--base` is what the PR is measured against (its merge base with `main`).
`--census` is a directory of census files from the latest successful full
selftest on `main`: the ids that ran there, the commit they ran at, and the
files each fault's injection touched. PLAN is `inherit<TAB>ID<TAB>SHA` rows;
a fault the plan does not name is re-proven, so every rule below fails
toward running.

A FAULT IS RE-PROVEN when any of these holds (ruled on #112, 2026-09-27):

  * it has no last-red commit on `main` -- new on this PR, or never seen red
    there, or seen red at a commit this branch does not contain;
  * the diff touches a file its injection touched (read off the sandbox by
    the `main` run, never parsed out of the injection's body);
  * the diff touches the file that defines its catcher: the test module a
    `test` scope or a lane fault's `catches` names, the fixture a results
    fault plants, the pattern table a pattern class is drawn from, or a
    script its check function calls;
  * inside `verify.sh`, at FUNCTION level: its injection, its `seeded_case`
    line, or its check function changed;
  * the selftest machinery changed -- `seeded_case`, `in_shard`, the sandbox,
    `expect_exit`, the scope plan's own reader, this script, the census
    reader, `verify.sh`'s code outside any function, the toolchain -- which
    re-proves everything.

Indirect dependencies (a helper change that quietly greens a fault) are the
full `main` run's to catch, within a day; that is the design's own answer to
drift, and this script does not pretend to see them.

Exit 0 with a plan written; 2 when the question cannot be answered.
"""

from __future__ import annotations

import argparse
import pathlib
import re
import subprocess
import sys
import tomllib

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import gatelib  # noqa: E402

EXIT_BROKEN = 2

ROOT = pathlib.Path(__file__).resolve().parent.parent
MANIFEST_READER = ROOT / "scripts" / "check-fault-manifest.py"

# A top-level shell function in verify.sh, by the same shape check-injections
# reads injections with: `name() {` at column 0, closed by `}` at column 0.
FUNCTION = re.compile(r"^([A-Za-z_][A-Za-z0-9_]*)\(\) \{\n(.*?)^\}\n", re.M | re.S)
# ...and the one-line form, `check_parity() { python3 scripts/...; }`, which
# ten of the checks are spelled in.
ONE_LINE = re.compile(r"^([A-Za-z_][A-Za-z0-9_]*)\(\) \{ (.*) \}\n", re.M)
SCRIPT = re.compile(r"scripts/[A-Za-z0-9_./-]+")
CALL = re.compile(r"\b([a-z_][a-z0-9_]*)\b")

# verify.sh functions whose change re-proves every fault: the machinery that
# runs a case, not a case. A helper an injection calls is here too -- it is
# shared by many injections, and which ones is not worth the second reader.
MACHINERY_FUNCTIONS = frozenset(
    {
        "seeded_case", "in_shard", "claim_fault", "load_scope_plan",
        "load_shard_plan", "sandbox", "sandbox_state", "shard_cost", "now_ms",
        "expect_exit", "edit_in_place", "seed_commit", "strip_substrates",
        "scratch", "prove_patterns", "inject_lane_fault", "scope_args",
        "run_check",
    }
)
# Files whose change re-proves every fault.
MACHINERY_FILES = frozenset(
    {
        "scripts/scope-selftest.py", "scripts/check-selftest-census.py",
        "scripts/hermetic.sh", "scripts/gatelib.py",
        "scripts/apply-lane-faults.py", "scripts/check-fault-manifest.py",
        "Cargo.toml", "Cargo.lock", "rust-toolchain.toml",
        ".github/workflows/gate-selftest.yml", "diet/Cargo.toml",
    }
)


# What a check READS beyond its scripts: a change there changes what the
# check is run against, so it re-proves that check's faults. `results` and
# `recompute` read the committed results/ tree itself.
CHECK_INPUTS = {
    "results": {"results/"},
    "recompute": {"results/"},
}


class Unusable(Exception):
    """The plan cannot be derived at all."""


# --------------------------------------------------------------------------
# reading verify.sh
# --------------------------------------------------------------------------


def functions(text: str) -> dict[str, str]:
    """Every top-level function's body, by name, in either spelling."""
    found = {m.group(1): m.group(2) for m in ONE_LINE.finditer(text)}
    found.update({m.group(1): m.group(2) for m in FUNCTION.finditer(text)})
    return found


def outside_functions(text: str) -> str:
    """verify.sh with every top-level function removed: its own top-level code."""
    return ONE_LINE.sub("", FUNCTION.sub("", text))


def without_cases(body: str) -> str:
    """`selftest`'s body with its `seeded_case` calls removed.

    Every case is a line of `selftest`, so its body changes whenever a case is
    added; that is the case's change, graded per fault, not the machinery's.
    """
    return "\n".join(
        line for line in gatelib.logical_lines(body) if not line.startswith("seeded_case")
    )


def case_lines(text: str) -> dict[str, tuple]:
    """Every seeded case, by the id the manifest registers it under."""
    return {
        f"{case.check}.{case.injection.removeprefix('inject_')}": tuple(case)
        for case in gatelib.seeded_cases(text)
    }


def changed_functions(base: str, head: str) -> set[str]:
    """Names whose body differs, or that exist on one side only."""
    before, after = functions(base), functions(head)
    names = set(before) | set(after)
    changed = {n for n in names if before.get(n) != after.get(n)}
    # `selftest` is compared without its cases; its cases are compared below.
    if "selftest" in changed and without_cases(before.get("selftest", "")) == without_cases(
        after.get("selftest", "")
    ):
        changed.discard("selftest")
    return changed


def check_units(bodies: dict[str, str], check: str) -> set[str]:
    """The verify.sh functions a check runs: itself, and those it calls, one
    level down -- `build_diet`, `resolve_diet`. A change to any re-proves it."""
    body = bodies.get(f"check_{check}", "")
    return {f"check_{check}"} | (set(CALL.findall(body)) & set(bodies))


def check_scripts(bodies: dict[str, str], check: str) -> set[str]:
    """The scripts those functions name."""
    found: set[str] = set()
    for unit in check_units(bodies, check):
        found |= set(SCRIPT.findall(bodies.get(unit, "")))
    return found


# --------------------------------------------------------------------------
# the catcher's files
# --------------------------------------------------------------------------


def module_data(path: str, root: pathlib.Path) -> str | None:
    """The data directory beside a Rust module, `diet/formats/record/` for
    `formats::record`: its fixtures and corpora are what its tests read."""
    parts = [p for p in path.split("::") if p and p != "crate"]
    for end in range(len(parts), 0, -1):
        candidate = "diet/" + "/".join(parts[:end]) + "/"
        if parts[0] != "src" and (root / candidate).is_dir():
            return candidate
    return None


def module_file(path: str, root: pathlib.Path) -> str | None:
    """The source file a Rust path like `client::tests::name` lives in: the
    longest prefix that names a file, `a/b.rs` or `a/b/mod.rs`."""
    parts = [p for p in path.split("::") if p and p != "crate"]
    for end in range(len(parts), 0, -1):
        stem = "/".join(parts[:end])
        for candidate in (f"diet/src/{stem}.rs", f"diet/src/{stem}/mod.rs"):
            if (root / candidate).is_file():
                return candidate
    return None


# Any change to Rust source: the conservative answer for a catcher this
# script cannot place, which fails toward running.
ANY_RUST = "diet/**.rs"


def scope_files(scope: str | None, root: pathlib.Path) -> set[str]:
    """The files a `test` scope's tests live in."""
    if not scope:
        return {ANY_RUST}
    target, _, flt = scope.partition("/")
    found: set[str] = set()
    if target.startswith("test:"):
        found.add(f"diet/tests/{target.removeprefix('test:')}.rs")
    if flt:
        placed = module_file(flt, root)
        found.add(placed if placed else ANY_RUST)
        data = module_data(flt, root)
        if data:
            found.add(data)
    elif not target.startswith("test:"):
        found.add(ANY_RUST)
    return found


def catches_files(catches: list[str], root: pathlib.Path) -> set[str]:
    """The files a lane fault's catching tests live in."""
    found: set[str] = set()
    for name in catches:
        placed = module_file(name, root) if "::" in name else None
        found.add(placed if placed else ANY_RUST)
        data = module_data(name, root) if "::" in name else None
        if data:
            found.add(data)
    return found


# --------------------------------------------------------------------------
# the fault list and each fault's dependencies
# --------------------------------------------------------------------------


def listed_faults(root: pathlib.Path) -> dict[str, str]:
    """Every fault the selftest proves red, id -> kind, from the manifest's
    own reader rather than a second opinion about which faults exist."""
    run = subprocess.run(
        [sys.executable, str(root / "scripts" / "check-fault-manifest.py"), "--list-selftest-red"],
        capture_output=True, text=True, cwd=root,
    )
    if run.returncode != 0:
        raise Unusable(f"check-fault-manifest.py --list-selftest-red exited {run.returncode}: {run.stderr.strip()}")
    faults = {}
    for line in run.stdout.splitlines():
        ident, _, kind = line.partition("\t")
        if ident:
            faults[ident] = kind
    if not faults:
        raise Unusable("the manifest lists no selftest fault")
    return faults


def dependencies(root: pathlib.Path, text: str) -> dict[str, tuple[set[str], set[str]]]:
    """id -> (files, verify.sh functions/case keys) the fault depends on,
    for every fault this script can model. One it cannot is re-proven."""
    bodies = functions(text)
    deps: dict[str, tuple[set[str], set[str]]] = {}

    for ident, case in case_lines(text).items():
        _label, check, inject, _sig, scope = case
        files = check_scripts(bodies, check) | CHECK_INPUTS.get(check, set())
        if check == "test":
            files |= scope_files(scope, root)
        deps[ident] = (files, {inject, f"case:{ident}"} | check_units(bodies, check))

    for manifest in sorted(root.glob("diet/*/gate.toml")):
        lane = tomllib.loads(manifest.read_text(encoding="utf-8"))
        where = str(manifest.relative_to(root))
        for fault in lane.get("fault", []):
            # The lane's own directory, gate.toml and fixtures with it.
            files = {str(manifest.parent.relative_to(root)) + "/", fault.get("target", ANY_RUST)}
            files |= catches_files(fault.get("catches", []), root)
            deps[fault["id"]] = (files, {"check_lanes"})

    for kind in ("hygiene", "pages"):
        table = f"scripts/{kind}-patterns.tsv"
        seeder = f"scripts/seed-{kind}-fault.sh"
        files = {table, seeder} | check_scripts(bodies, kind)
        units = check_units(bodies, kind)
        for line in (root / table).read_text(encoding="utf-8").splitlines():
            label = line.split("\t", 1)[0].strip()
            if label and not label.startswith("#"):
                deps.setdefault(f"{kind}.{label}", (set(files), set(units)))

    bad = root / "tests" / "fixtures" / "results-bad"
    if bad.is_dir():
        files = check_scripts(bodies, "results") | CHECK_INPUTS["results"]
        for fixture in sorted(p for p in bad.iterdir() if p.is_dir()):
            deps[f"results.{fixture.name}"] = (
                files | {f"tests/fixtures/results-bad/{fixture.name}/"},
                check_units(bodies, "results"),
            )
    return deps


# --------------------------------------------------------------------------
# the main run's census
# --------------------------------------------------------------------------


def read_census(directory: pathlib.Path) -> tuple[dict[str, str], dict[str, set[str]]]:
    """(id -> the commit it last ran red at, id -> files its injection touched).

    Only the three row kinds this needs are read; the rest is
    check-selftest-census.py's to grade. A fault the `main` run INHERITED was
    not seen red there, so it contributes no sha.
    """
    red: dict[str, str] = {}
    touched: dict[str, set[str]] = {}
    for path in sorted(p for p in directory.rglob("*") if p.is_file()):
        commit = None
        ran: list[str] = []
        for line in path.read_text(encoding="utf-8").splitlines():
            parts = line.split("\t")
            if parts[0] == "commit" and len(parts) == 2:
                commit = parts[1].strip()
            elif parts[0] == "ordinal" and len(parts) == 3:
                ran.append(parts[2])
            elif parts[0] == "touched" and len(parts) == 3:
                touched.setdefault(parts[1], set()).add(parts[2])
        if commit and re.fullmatch(r"[0-9a-f]{7,40}", commit):
            for ident in ran:
                red[ident] = commit
    return red, touched


# --------------------------------------------------------------------------
# the decision
# --------------------------------------------------------------------------


def reaches(files: set[str], changed: set[str]) -> str | None:
    """The first changed file one of these dependency entries names."""
    for entry in sorted(files):
        if entry == ANY_RUST:
            hit = next((c for c in sorted(changed) if c.startswith("diet/") and c.endswith(".rs")), None)
        elif entry.endswith("/"):
            hit = next((c for c in sorted(changed) if c.startswith(entry)), None)
        else:
            hit = entry if entry in changed else None
        if hit:
            return hit
    return None


def decide(
    faults: dict[str, str],
    deps: dict[str, tuple[set[str], set[str]]],
    red: dict[str, str],
    touched: dict[str, set[str]],
    changed_files: set[str],
    changed_units: set[str],
    reachable,
) -> tuple[dict[str, str], dict[str, str]]:
    """(id -> why it is re-proven, id -> the sha it is inherited at)."""
    machinery = sorted(changed_files & MACHINERY_FILES) + sorted(changed_units & (MACHINERY_FUNCTIONS | {"selftest", "<top-level>"}))
    rerun: dict[str, str] = {}
    inherit: dict[str, str] = {}
    for ident in sorted(faults):
        if machinery:
            rerun[ident] = f"the selftest machinery changed: {machinery[0]}"
            continue
        sha = red.get(ident)
        if not sha:
            rerun[ident] = "no last-red commit on main"
            continue
        if not reachable(sha):
            rerun[ident] = f"last seen red at {sha}, which this branch does not contain"
            continue
        if ident not in deps:
            rerun[ident] = "no dependency model for this fault"
            continue
        files, units = deps[ident]
        hit = reaches(touched.get(ident, set()), changed_files)
        if hit:
            rerun[ident] = f"its injection touches {hit}"
            continue
        hit = reaches(files, changed_files)
        if hit:
            rerun[ident] = f"its catcher depends on {hit}"
            continue
        unit = next(iter(sorted(units & changed_units)), None)
        if unit:
            rerun[ident] = f"verify.sh changed {unit}"
            continue
        inherit[ident] = sha
    return rerun, inherit


def changed_since(since: str, root: pathlib.Path) -> tuple[set[str], set[str]]:
    """(files that differ, verify.sh units that changed) between `since` and
    HEAD.

    Measured from the commit a fault was last seen red at, NOT from the PR's
    merge base: whatever landed on `main` after that run is part of what the
    inheritance would be vouching for (found by #130's review). And
    `--no-renames`, because a rename is listed under its new path only, and
    the old path is the one a fault depends on.
    """
    diff = subprocess.run(
        ["git", "diff", "--no-renames", "--name-only", since, "HEAD"],
        capture_output=True, text=True, cwd=root,
    )
    if diff.returncode != 0:
        raise Unusable(f"git diff {since} HEAD exited {diff.returncode}: {diff.stderr.strip()}")
    files = {line for line in diff.stdout.splitlines() if line}
    units: set[str] = set()
    if "verify.sh" in files:
        old = subprocess.run(
            ["git", "show", f"{since}:verify.sh"], capture_output=True, text=True, cwd=root
        )
        if old.returncode != 0:
            raise Unusable(f"verify.sh at {since} could not be read")
        new = (root / "verify.sh").read_text(encoding="utf-8")
        units = changed_functions(old.stdout, new)
        if outside_functions(old.stdout) != outside_functions(new):
            units.add("<top-level>")
        before, after = case_lines(old.stdout), case_lines(new)
        units |= {f"case:{i}" for i in set(before) | set(after) if before.get(i) != after.get(i)}
    return files, units


def is_ancestor(root: pathlib.Path):
    def reachable(sha: str) -> bool:
        return subprocess.run(
            ["git", "merge-base", "--is-ancestor", sha, "HEAD"], cwd=root, capture_output=True
        ).returncode == 0
    return reachable


# --------------------------------------------------------------------------
# fixtures
# --------------------------------------------------------------------------

FIXTURES: list = []


def fixture(name: str):
    def take(fn):
        FIXTURES.append((name, fn))
        return fn

    return take


def _decide(changed_files=(), changed_units=(), red=None, touched=None, deps=None):
    faults = {"test.a": "seeded-gate", "lanes.b": "lane-fault", "results.c": "results-fixture"}
    deps = deps if deps is not None else {
        "test.a": ({"diet/src/formats/regimen.rs"}, {"inject_a", "check_test", "case:test.a"}),
        "lanes.b": ({"diet/client/gate.toml", "diet/src/client/mod.rs"}, {"check_lanes"}),
        "results.c": ({"tests/fixtures/results-bad/c/", "scripts/check-results.py"}, {"check_results"}),
    }
    red = red if red is not None else {i: "abc1234" for i in faults}
    return decide(faults, deps, red, touched or {}, set(changed_files), set(changed_units), lambda sha: True)


@fixture("a diff touching only results/ inherits every other fault")
def _results_only():
    rerun, inherit = _decide(changed_files={"tests/fixtures/results-bad/c/README.md"})
    if set(rerun) != {"results.c"}:
        return f"re-proved {sorted(rerun)}, not only results.c"
    if inherit != {"test.a": "abc1234", "lanes.b": "abc1234"}:
        return f"inherited {inherit}"
    return None


@fixture("a results/-only diff re-proves the results lane and no test fault")
def _results_lane_real_tree():
    text = (ROOT / "verify.sh").read_text(encoding="utf-8")
    faults = listed_faults(ROOT)
    deps = dependencies(ROOT, text)
    red = {i: "abc1234" for i in faults}
    rerun, _ = decide(faults, deps, red, {}, {"results/some-run/README.md"}, set(), lambda sha: True)
    if any(i.startswith("test.") for i in rerun):
        return f"a results/ change re-proved test faults: {sorted(i for i in rerun if i.startswith('test.'))[:3]}"
    lanes = {i.split(".", 1)[0] for i in rerun}
    if not rerun or not lanes <= {"results", "recompute"}:
        return f"a results/ change re-proved {sorted(lanes)}, not the results lane"
    return None


def _real_rerun(files=(), units=()):
    text = (ROOT / "verify.sh").read_text(encoding="utf-8")
    faults = listed_faults(ROOT)
    red = {i: "abc1234" for i in faults}
    rerun, _ = decide(faults, dependencies(ROOT, text), red, {}, set(files), set(units), lambda sha: True)
    return rerun


@fixture("a script a one-line check function calls re-proves that check's faults")
def _one_line_checks():
    for script, check in (("scripts/check-results.py", "results"), ("scripts/check-library.py", "library"),
                          ("scripts/check-fault-manifest.py", None), ("scripts/hygiene.sh", "hygiene")):
        rerun = _real_rerun(files={script})
        if check and not any(i.startswith(f"{check}.") for i in rerun):
            return f"{script} changed and no {check}.* fault was re-proven"
    rerun = _real_rerun(files={"verify.sh"}, units={"build_diet"})
    if not any(i.startswith("results.") for i in rerun):
        return "build_diet, which check_results calls, changed and no results.* fault was re-proven"
    return None


@fixture("a fixture the catching tests read re-proves the fault")
def _catcher_fixtures():
    rerun = _real_rerun(files={"diet/formats/record/fixtures/invalid/prefix-change-that-is-not-a-change.jsonl"})
    if "test.record_prefix_change_not_a_change" not in rerun:
        return f"a record fixture changed and its fault was inherited; re-proven: {sorted(rerun)[:5]}"
    return None


@fixture("a rename is seen at its old path")
def _renames():
    import tempfile
    with tempfile.TemporaryDirectory() as box:
        run = lambda *a: subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", *a],
                                        cwd=box, capture_output=True, text=True, check=True)
        run("init", "-q")
        (pathlib.Path(box) / "a.rs").write_text("fn a() {}\n" * 20, encoding="utf-8")
        run("add", "-A"); run("commit", "-qm", "one")
        base = run("rev-parse", "HEAD").stdout.strip()
        run("mv", "a.rs", "b.rs"); run("commit", "-qm", "two")
        files, _ = changed_since(base, pathlib.Path(box))
    if "a.rs" not in files:
        return f"a rename was listed as {sorted(files)}, without the old path"
    return None


@fixture("a fault with no last-red commit on main runs regardless of scope")
def _no_last_red():
    rerun, _ = _decide(red={"lanes.b": "abc1234", "results.c": "abc1234"})
    if "test.a" not in rerun or "no last-red" not in rerun["test.a"]:
        return f"a fault never seen red on main was not re-proven: {rerun}"
    return None


@fixture("a last-red commit this branch does not contain is not a last-red commit")
def _unreachable_sha():
    faults = {"test.a": "seeded-gate"}
    rerun, inherit = decide(faults, {"test.a": (set(), set())}, {"test.a": "abc1234"}, {}, set(), set(), lambda sha: False)
    if inherit or "does not contain" not in rerun.get("test.a", ""):
        return f"an unreachable sha was inherited: {inherit}"
    return None


@fixture("a file the fault's injection touched re-proves it, read off the main run")
def _touched_file():
    rerun, _ = _decide(changed_files={"diet/src/object.rs"}, touched={"test.a": {"diet/src/object.rs"}})
    if set(rerun) != {"test.a"}:
        return f"a touched-file change re-proved {sorted(rerun)}"
    return None


@fixture("a change to the catcher's file re-proves it")
def _catcher_file():
    rerun, _ = _decide(changed_files={"diet/src/client/mod.rs"})
    if set(rerun) != {"lanes.b"}:
        return f"a catcher-file change re-proved {sorted(rerun)}"
    return None


@fixture("inside verify.sh the unit is the function, not the file")
def _function_level():
    rerun, _ = _decide(changed_files={"verify.sh"}, changed_units={"check_results"})
    if set(rerun) != {"results.c"}:
        return f"a check-function change re-proved {sorted(rerun)}"
    rerun, _ = _decide(changed_files={"verify.sh"}, changed_units={"case:test.a"})
    if set(rerun) != {"test.a"}:
        return f"a seeded_case line change re-proved {sorted(rerun)}"
    return None


@fixture("a change to the selftest machinery re-proves everything")
def _machinery():
    for files, units in (({"verify.sh"}, {"seeded_case"}), ({"verify.sh"}, {"<top-level>"}), ({"scripts/hermetic.sh"}, set())):
        rerun, inherit = _decide(changed_files=files, changed_units=units)
        if inherit:
            return f"{sorted(files | units)} changed and {sorted(inherit)} was still inherited"
    return None


@fixture("a fault with no dependency model is re-proven, never inherited")
def _unmodelled():
    rerun, inherit = _decide(deps={})
    if inherit or len(rerun) != 3:
        return f"an unmodelled fault was inherited: {inherit}"
    return None


@fixture("a catcher this script cannot place is any Rust change")
def _unplaced_catcher():
    root = ROOT
    if scope_files("lib/no_such_module_anywhere", root) != {ANY_RUST}:
        return f"an unplaceable filter placed: {scope_files('lib/no_such_module_anywhere', root)}"
    if reaches({ANY_RUST}, {"diet/src/anything.rs"}) is None:
        return "ANY_RUST did not reach a Rust change"
    if reaches({ANY_RUST}, {"results/x/README.md"}) is not None:
        return "ANY_RUST reached a non-Rust change"
    return None


@fixture("a selftest body changed only by an added case is not a machinery change")
def _selftest_cases_only():
    base = "selftest() {\n  local x=1\n  seeded_case \"a\" test inject_a \\\n    'sig' 'lib'\n}\n"
    head = base.replace("}\n", "  seeded_case \"b\" test inject_b \\\n    'sig' 'lib'\n}\n")
    if "selftest" in changed_functions(base, head):
        return "adding a case read as a machinery change"
    if "selftest" not in changed_functions(base, head.replace("local x=1", "local x=2")):
        return "a real change to selftest's own code was not seen"
    return None


@fixture("no mechanics assertion lives in a function whose change re-proves everything")
def _mechanics_outside_machinery():
    # A mechanics assertion runs on every selftest whatever the scope, so it is
    # no fault's dependency. Written inside a machinery function, every edit to
    # one re-proved all 578 faults: #147 added three to `selftest` and scoped
    # nothing, measured on #112. The `expect_exit` DEFINITION is machinery and
    # is the one exception.
    bodies = functions((ROOT / "verify.sh").read_text(encoding="utf-8"))
    inside = sorted(
        name for name, body in bodies.items()
        if name in MACHINERY_FUNCTIONS | {"selftest"} and name != "expect_exit"
        and re.search(r"^\s*expect_exit ", body, re.M)
    )
    if inside:
        return f"expect_exit is called inside machinery: {inside}"
    return None


@fixture("the census gives each fault the commit it last ran red at, and what it touched")
def _census():
    import tempfile
    with tempfile.TemporaryDirectory() as box:
        path = pathlib.Path(box) / "shard-1" / "census.tsv"
        path.parent.mkdir()
        path.write_text(
            "shard\t1\nshards\t1\ntotal\t3\ncommit\tabc1234\n"
            "ordinal\t1\ttest.a\nordinal\t2\tlanes.b\n"
            "inherited\t3\tresults.c\tdef5678\n"
            "touched\ttest.a\tdiet/src/object.rs\n",
            encoding="utf-8",
        )
        red, touched = read_census(pathlib.Path(box))
    if red != {"test.a": "abc1234", "lanes.b": "abc1234"}:
        return f"red came back as {red}; an inherited fault must not count as seen red"
    if touched != {"test.a": {"diet/src/object.rs"}}:
        return f"touched came back as {touched}"
    return None


@fixture("every fault this repository lists has a dependency model")
def _real_tree_modelled():
    text = (ROOT / "verify.sh").read_text(encoding="utf-8")
    unmodelled = sorted(set(listed_faults(ROOT)) - set(dependencies(ROOT, text)))
    if unmodelled:
        return f"{len(unmodelled)} fault(s) would always re-run for want of a model: {unmodelled[:5]}"
    return None


def selftest() -> int:
    broken = []
    for name, run in FIXTURES:
        try:
            why = run()
        except Exception as err:  # a fixture that explodes is a fixture that failed
            why = f"{type(err).__name__}: {err}"
        print(f"{'ok  ' if why is None else 'FAIL'}  {name}")
        if why is not None:
            broken.append((name, why))
    for name, why in broken:
        print(f"  {name}\n    {why}", file=sys.stderr)
    print(
        f"scope-selftest: {len(FIXTURES)} fixture(s), {len(broken)} failed",
        file=sys.stderr if broken else sys.stdout,
    )
    return 1 if broken else 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    parser.add_argument("--base", help="the ref the PR targets; recorded in the plan. Each inheritance is measured from the commit its fault was last seen red at")
    parser.add_argument("--census", help="a directory of census files from main's full run")
    parser.add_argument("--out", help="where to write the plan")
    parser.add_argument("--selftest", action="store_true", help="run the fixtures and exit")
    args = parser.parse_args(argv)
    if args.selftest:
        return selftest()
    if not (args.base and args.census and args.out):
        print("scope-selftest: --base, --census and --out are all required", file=sys.stderr)
        return EXIT_BROKEN
    try:
        faults = listed_faults(ROOT)
        text = (ROOT / "verify.sh").read_text(encoding="utf-8")
        deps = dependencies(ROOT, text)
        census = pathlib.Path(args.census)
        red, touched = read_census(census) if census.is_dir() else ({}, {})
        reachable = is_ancestor(ROOT)
        # One diff per commit the census names (a `main` run has one), each
        # measured from that commit to this head.
        rerun: dict[str, str] = {}
        inherit: dict[str, str] = {}
        by_sha: dict[str, dict[str, str]] = {}
        for ident, kind in faults.items():
            sha = red.get(ident)
            if sha and reachable(sha):
                by_sha.setdefault(sha, {})[ident] = kind
        placed = {i for group in by_sha.values() for i in group}
        unplaced = {i: k for i, k in faults.items() if i not in placed}
        got_rerun, got_inherit = decide(unplaced, deps, red, touched, set(), set(), reachable)
        rerun.update(got_rerun)
        inherit.update(got_inherit)
        for sha, group in sorted(by_sha.items()):
            files, units = changed_since(sha, ROOT)
            got_rerun, got_inherit = decide(group, deps, red, touched, files, units, reachable)
            rerun.update(got_rerun)
            inherit.update(got_inherit)
    except (Unusable, OSError) as err:
        print(f"scope-selftest: {err}", file=sys.stderr)
        return EXIT_BROKEN
    body = "".join(f"inherit\t{i}\t{s}\n" for i, s in sorted(inherit.items()))
    pathlib.Path(args.out).write_text(
        f"# scope-selftest: {len(rerun)} re-proven, {len(inherit)} inherited, against {args.base}\n" + body,
        encoding="utf-8",
    )
    print(f"scope-selftest: {len(rerun)} fault(s) re-proven, {len(inherit)} inherited")
    reasons: dict[str, int] = {}
    for why in rerun.values():
        reasons[why] = reasons.get(why, 0) + 1
    for why, count in sorted(reasons.items(), key=lambda kv: -kv[1])[:10]:
        print(f"  {count:4d}  {why}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
