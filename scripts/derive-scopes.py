#!/usr/bin/env python3
"""Derive each `test` seeded case's `--scope` from an unscoped run, and check
the declared ones against it.

`verify.sh --selftest` runs 181 `test` cases, and each declares the tests it
needs so the case can run those instead of the workspace. That flag makes the
gate run LESS, which is a hazard by construction: a scope that selects the
wrong tests is a case that went green over code it never touched.

Three guards already hold it -- the scope is a flag and never an environment
variable, the coverage check refuses it in a gating workflow, and a scope
selecting nothing is a failure rather than a fast green. What none of them
answer is whether a declaration is still TRUE: that the tests it names are the
tests the seeded fault actually breaks. A scope that was right when it was
written and wrong after a rename is exactly the shape those three miss, and
181 hand-maintained declarations are 181 chances at it.

So the declarations are a HARVEST. `verify.sh --selftest --derive-scopes`
re-runs every `test` case with no scope at all, and this reads what broke:

    ./verify.sh --selftest --derive-scopes DIR   # writes DIR/derive-scopes.tsv
    python3 scripts/derive-scopes.py --index DIR/derive-scopes.tsv
    python3 scripts/derive-scopes.py --index ... --emit    # the declarations

Ruled 2026-09-11 as the condition on keeping `--scope`: the derivation is
re-runnable, so the list is never hand-maintained.

WHAT MAKES A DECLARATION SOUND IS THAT IT SELECTS SOMETHING THE FAULT BREAKS.
Not everything. A case is RED because the run it makes fails, and one failing
test is enough to fail a run, so a scope naming one test out of thirty-three
is as red as a scope naming all thirty-three. The narrow one is more FRAGILE
-- rename that test and the case goes green -- but fragility is caught by the
ordinary selftest the next time it runs, and a narrow blast radius is the
whole reason `--scope` exists. Demanding containment of every failure would
call ten sound declarations broken for being cheap on purpose.

So: selecting none is a defect and exits 1. Selecting some is sound and is
reported as MARGIN, beside the scope a harvest would have proposed.

A fault that breaks the BUILD is sound under any scope that names the target
that failed to compile: cargo never gets as far as selecting tests, so there
is nothing for a filter to miss. Such a log has no failure list in it, and
reading that as "the derivation broke" is wrong.

Exit 0 when every declaration selects something its fault breaks, 1 when one
selects nothing, 2 when the derivation cannot run at all.

THE READER IS EXERCISED BEFORE IT IS TRUSTED. `--selftest` runs the fixture
suite at the foot of this file, and `verify.sh --only derive` is that suite.
The harvest it reads costs forty minutes, which is exactly the shape of tool
that never gets run against a case it has not already seen -- and the first
defect in this file was of that kind: the compile-failure pattern was anchored
`^` without `re.MULTILINE`, so it matched only a log that BEGAN with the error
and every build failure came back as "the derivation broke". Nothing found it
but a harvest. A fixture does now.
"""

from __future__ import annotations

import argparse
import contextlib
import io
import pathlib
import re
import sys
import tempfile

EXIT_BAD = 1
EXIT_BROKEN = 2

# `Running unittests src/lib.rs (target/debug/deps/discipline_diet-4d1a...)` and
# `Running tests/conformance.rs (target/debug/deps/conformance-9f2c...)`. The
# path before the parenthesis is what names the target; the hash inside is not
# stable and is never read.
RUNNING = re.compile(r"^\s*Running (?P<what>\S+)(?: \((?P<bin>[^)]*)\))?\s*$")
UNITTESTS = re.compile(r"^\s*Running unittests (?P<path>\S+) \(")
# A test harness's failure list: `failures:` then indented names, ending at a
# blank line or the summary. Cargo prints the block TWICE -- once with the
# output of each failure, once as a bare list -- and both are parsed, because
# reading only the first would miss a failure whose output was empty.
FAILURES = re.compile(r"^failures:\s*$")
FAILURE_NAME = re.compile(r"^\s{4}(?P<name>[A-Za-z_][A-Za-z0-9_:]*)\s*$")
# `error: could not compile `discipline-diet` (lib test) due to 2 previous errors`.
# The parenthesised unit is the compilation unit, which is what says which
# scope target the build failure lands in.
WRECKED = re.compile(
    r"^error: could not compile `[^`]+` \((?P<unit>[^)]*)\)", re.MULTILINE
)


class Unusable(Exception):
    """The derivation cannot be run at all."""


def target_of(line: str) -> str | None:
    """The scope target a `Running` line names, or None if it names no target."""
    unit = UNITTESTS.match(line)
    if unit:
        # `unittests src/lib.rs` is the library; `unittests src/bin/diet.rs` is
        # a binary. They are different scope targets and the path is what
        # separates them.
        return "lib" if unit.group("path").endswith("src/lib.rs") else "bins"
    running = RUNNING.match(line)
    if running:
        what = running.group("what")
        if what.startswith("tests/") and what.endswith(".rs"):
            return f"test:{what[len('tests/'):-len('.rs')]}"
    return None


def unit_target(unit: str) -> str:
    """The scope target cargo's compilation unit belongs to.

    Cargo spells it `lib`, `lib test`, `bin "diet"`, `bin "diet" test`,
    `test "conformance"`. An unrecognised spelling is REFUSED rather than
    guessed at: a unit this cannot name is a unit whose failure it cannot
    place, and placing it wrongly is how a build failure gets attributed to a
    target that still builds.
    """
    words = unit.split()
    kind = words[0] if words else ""
    named = re.search(r'"([^"]*)"', unit)
    if kind == "lib":
        return "lib"
    if kind == "bin":
        return "bins"
    if kind == "test" and named:
        return f"test:{named.group(1)}"
    raise Unusable(
        f"cargo says the build failed in {unit!r}, which is not a unit this can "
        f"name as a scope target"
    )


def broke(log: str) -> dict[str, set[str]]:
    """The failing test names in `log`, by the target that ran them."""
    found: dict[str, set[str]] = {}
    target: str | None = None
    collecting = False
    for line in log.splitlines():
        named = target_of(line)
        if named is not None:
            target, collecting = named, False
            continue
        if FAILURES.match(line):
            collecting = True
            continue
        if collecting:
            name = FAILURE_NAME.match(line)
            if name:
                if target is None:
                    # A failure before any `Running` line: cargo did not get as
                    # far as naming a target, so nothing here can say which one
                    # to scope to. Reported rather than guessed.
                    raise Unusable("a failure appears before any target was named")
                found.setdefault(target, set()).add(name.group("name"))
            elif line.strip():
                collecting = False
    return found


def wrecked(log: str) -> set[str]:
    """The scope targets in `log` that failed to COMPILE.

    A target that does not build has no failure list -- there were no tests to
    run -- and every scope naming it is red, filter or no filter.
    """
    return {unit_target(found.group("unit")) for found in WRECKED.finditer(log)}


def module_prefix(names: set[str]) -> str:
    """The longest `::`-delimited prefix every name in `names` shares.

    Truncated at a module boundary rather than at a character: `formats::rec`
    is not a module, and a filter that is half an identifier matches by
    substring and would quietly select a different set on the next rename.
    """
    if not names:
        return ""
    split = [name.split("::") for name in sorted(names)]
    shared: list[str] = []
    for index in range(min(len(parts) for parts in split)):
        segment = {parts[index] for parts in split}
        if len(segment) != 1:
            break
        shared.append(segment.pop())
    # THE LAST SEGMENT IS THE TEST FUNCTION, and a filter naming one function
    # is the narrowest possible scope and the most brittle: rename the test and
    # the declaration selects nothing, which the selected-count control then
    # refuses -- correctly, and a rename away from where the rename happened.
    # The module is what the committed declarations name, and it is what a
    # harvest should propose.
    #
    # Kept whole only when dropping it would leave nothing: a test at the crate
    # root has no module to name.
    if len(shared) == min(len(parts) for parts in split) and len(shared) > 1:
        shared = shared[:-1]
    return "::".join(shared)


def derived(failures: dict[str, set[str]], unbuilt: set[str]) -> str:
    """The narrowest scope spec that still selects every failure."""
    targets = set(failures) | unbuilt
    if not targets:
        raise Unusable(
            "no failing test and no build failure in the log, so the case named "
            "nothing to scope to"
        )
    if len(targets) > 1:
        # Two targets, and a scope names one of them or all of them. `all` is
        # the only target that keeps both -- but `all` still takes a filter,
        # and when every failing name shares a module that filter is narrower
        # than the bare target while still selecting all of them.
        #
        # A target that did not COMPILE contributes no names, and a filter
        # cannot narrow a build failure, so the presence of one forces the
        # bare `all`.
        if unbuilt:
            return "all"
        shared = module_prefix({name for names in failures.values() for name in names})
        return f"all/{shared}" if shared else "all"
    target = next(iter(targets))
    if target in unbuilt:
        # Nothing ran, so there are no names to filter on -- and a filter
        # written against names that never got compiled is a filter against a
        # guess.
        return target
    prefix = module_prefix(failures[target])
    return f"{target}/{prefix}" if prefix else target


def split_scope(scope: str) -> tuple[str, str]:
    """The same two-part reading `scope_args` in verify.sh does.

    A target and an optional filter the harness matches as a SUBSTRING.
    Substring, not prefix -- `cargo test -- foo` runs every test whose full
    name contains `foo` -- and reading it as a prefix here would call a working
    declaration broken.
    """
    target, _, filter_ = scope.partition("/")
    return target, filter_


def selected(scope: str, failures: dict[str, set[str]], unbuilt: set[str]) -> set[str]:
    """The failing test names `scope` selects.

    A target in `unbuilt` contributes no names because it ran no tests. Whether
    the scope reaches it is a separate question -- `reaches_unbuilt` -- and
    both have to be asked, because a scope over a target that did not compile
    selects nothing and is still red.
    """
    target, filter_ = split_scope(scope)
    hit: set[str] = set()
    for ran_in, names in failures.items():
        if target not in ("all", ran_in):
            continue
        hit |= {name for name in names if not filter_ or filter_ in name}
    return hit


def reaches_unbuilt(scope: str, unbuilt: set[str]) -> bool:
    """Whether `scope` names a target that failed to compile."""
    target, _ = split_scope(scope)
    return any(target in ("all", ran_in) for ran_in in unbuilt)


def why_none(scope: str, failures: dict[str, set[str]]) -> str:
    """Why `scope` selected no failing test, for the message that refuses it."""
    target, filter_ = split_scope(scope)
    named = [ran_in for ran_in in failures if target in ("all", ran_in)]
    if not named:
        where = ", ".join(sorted(failures)) or "no target at all"
        return f"the failures are in {where}, which the scope does not name"
    reachable = sorted(name for ran_in in named for name in failures[ran_in])
    return (
        f"the filter {filter_!r} is in none of the {len(reachable)} failing "
        f"test(s) the scope's target ran, the first being {reachable[0]}"
    )


def read_index(path: pathlib.Path) -> list[tuple[pathlib.Path, str, str]]:
    """The `log<TAB>label<TAB>declared-scope` rows verify.sh wrote."""
    try:
        text = path.read_text(encoding="utf-8")
    except OSError as err:
        raise Unusable(f"cannot read the index {path}: {err}") from err
    rows = []
    for number, line in enumerate(text.splitlines(), start=1):
        if not line.strip():
            continue
        parts = line.split("\t")
        if len(parts) != 3:
            raise Unusable(f"{path}:{number}: want log, label and scope, got {len(parts)} field(s)")
        rows.append((pathlib.Path(parts[0]), parts[1], parts[2]))
    if not rows:
        raise Unusable(
            f"{path} names no cases. A derivation over nothing is not a derivation, "
            f"and an empty index is what a `--derive-scopes` run that never ran looks like"
        )
    return rows


# --------------------------------------------------------------------------
# the fixture suite
# --------------------------------------------------------------------------
#
# What each fixture holds is a BEHAVIOUR of the reader, named for the thing
# that goes wrong when the behaviour is missing -- not "the function returned
# something". A harvest is forty minutes away; a fixture is not, and the only
# defect this file has had so far was one a harvest found.

FIXTURES: list[tuple[str, object]] = []


def fixture(name: str):
    def take(fn):
        FIXTURES.append((name, fn))
        return fn

    return take


LIB_RAN = "     Running unittests src/lib.rs (target/debug/deps/diet-feaf660c280b064e)\n"
BIN_RAN = "     Running unittests src/bin/diet.rs (target/debug/deps/diet-0009e66b3e31214f)\n"
CONFORMANCE_RAN = "     Running tests/conformance.rs (target/debug/deps/conformance-f5d8.)\n"
CLI_RAN = "     Running tests/cli.rs (target/debug/deps/cli-dba7956ed22ef0cd)\n"


def failed(*names: str, quiet: tuple[str, ...] = ()) -> str:
    """A cargo harness's failure report, in the two-block shape cargo emits.

    `quiet` names failures whose stdout section is empty -- a test that
    asserted without printing -- which appear ONLY in the second, bare list.
    """
    out = "\nfailures:\n\n"
    for name in names:
        out += (
            f"---- {name} stdout ----\n\n"
            f"thread '{name}' panicked at diet/src/lib.rs:20:9:\n"
            f"assertion `left == right` failed: seeded fault\n\n"
        )
    out += "\nfailures:\n"
    for name in sorted((*names, *quiet)):
        out += f"    {name}\n"
    out += (
        f"\ntest result: FAILED. 556 passed; {len(names) + len(quiet)} failed; "
        f"0 ignored; 0 measured; 0 filtered out; finished in 1.81s\n\n"
        "error: test failed, to rerun pass `--lib`\n"
    )
    return out


WRECKED_LOG = (
    "   Compiling discipline-diet v0.1.0 (/tmp/box/diet)\n"
    "error[E0004]: non-exhaustive patterns: `FieldKind::Seeded` not covered\n"
    "   --> diet/src/formats/interview.rs:80:15\n"
    "    |\n"
    "80  |         match self {\n"
    "    |               ^^^^ pattern `FieldKind::Seeded` not covered\n"
    "\n"
    "For more information about this error, try `rustc --explain E0004`.\n"
    "error: could not compile `discipline-diet` (lib) due to 2 previous errors\n"
    "warning: build failed, waiting for other jobs to finish...\n"
    "error: could not compile `discipline-diet` (lib test) due to 2 previous errors\n"
)


@fixture("a target is read from the path the harness ran")
def _targets_come_from_paths():
    # `unittests src/lib.rs` and `unittests src/bin/diet.rs` differ only in the
    # path, and they are different scope targets -- `--lib` does not run the
    # binaries. A reader keying on "unittests" alone calls both `lib` and
    # sends every binary failure to a scope that never runs it.
    want = {
        LIB_RAN: "lib",
        BIN_RAN: "bins",
        CONFORMANCE_RAN: "test:conformance",
        CLI_RAN: "test:cli",
    }
    for line, target in want.items():
        got = target_of(line.rstrip("\n"))
        if got != target:
            return f"{line.strip()!r} read as {got!r}, not {target!r}"
    # The hash in the parentheses is not stable between builds and is never
    # part of the answer: the same path with a different binary is the same
    # target.
    moved = LIB_RAN.replace("feaf660c280b064e", "0000000000000000")
    if target_of(moved.rstrip("\n")) != "lib":
        return "the binary's hash changed the target it was read as"
    # A line that names no target must say so rather than claim the last one.
    if target_of("test result: FAILED. 556 passed; 1 failed") is not None:
        return "a line that is not a `Running` line was read as naming a target"
    return None


@fixture("each failure is attributed to the target that ran it")
def _failures_stay_with_their_target():
    # Two harnesses fail in one run and a scope names one of them. Merging the
    # lists loses which -- and the derivation then proposes a scope that runs
    # the tests of a target whose failures were somebody else's.
    log = (
        LIB_RAN
        + failed("formats::record::tests::a_row")
        + BIN_RAN
        + failed("drive::tests::a_capture")
    )
    got = broke(log)
    want = {
        "lib": {"formats::record::tests::a_row"},
        "bins": {"drive::tests::a_capture"},
    }
    if got != want:
        return f"read as {got!r}, not {want!r}"
    return None


@fixture("a failure that printed nothing is still read")
def _quiet_failures_are_read():
    # Cargo prints the failure block TWICE: once with each failure's output,
    # once as a bare list. A test that fails without printing appears only in
    # the second. Reading the first block alone loses it, and losing it is how
    # a derived scope comes out narrower than the fault.
    log = LIB_RAN + failed("formats::record::tests::a_row", quiet=("formats::record::tests::b_row",))
    got = broke(log).get("lib", set())
    if "formats::record::tests::b_row" not in got:
        return f"the silent failure was dropped; read {sorted(got)}"
    if len(got) != 2:
        return f"read {sorted(got)}, not both failures"
    return None


@fixture("a build failure names the target that did not compile")
def _build_failure_is_placed():
    # THE DEFECT THIS FILE ACTUALLY HAD. The pattern was anchored `^` with no
    # re.MULTILINE, so it matched only a log that BEGAN with the error -- and
    # every real log begins with `Compiling`. A fault that breaks the build
    # came back as "no failing test and no build failure", which is exit 2,
    # which is the derivation refusing to run over a case that is perfectly
    # readable. So the fixture's log deliberately does NOT start with it.
    if WRECKED_LOG.startswith("error"):
        return "the fixture's log begins with the error, so it cannot catch the anchor"
    got = wrecked(WRECKED_LOG)
    if got != {"lib"}:
        return f"the wrecked target read as {sorted(got)}, not ['lib']"
    if broke(WRECKED_LOG):
        return "a log with no harness output produced failing test names"
    if derived({}, got) != "lib":
        return f"derived {derived({}, got)!r} from a build failure in the library"
    return None


@fixture("a compilation unit this cannot place is refused, not guessed")
def _unknown_unit_is_refused():
    # Cargo spells the unit in several shapes and this maps three of them. The
    # fourth -- an example, a bench, a build script -- is a failure this cannot
    # attribute, and attributing it anyway sends a build failure to a target
    # that still builds.
    for unit, target in (
        ("lib", "lib"),
        ("lib test", "lib"),
        ('bin "diet"', "bins"),
        ('bin "diet" test', "bins"),
        ('test "conformance"', "test:conformance"),
    ):
        got = unit_target(unit)
        if got != target:
            return f"{unit!r} placed at {got!r}, not {target!r}"
    try:
        placed = unit_target('example "demo"')
    except Unusable:
        return None
    return f"an example was placed at {placed!r} rather than refused"


@fixture("a scope selecting one of many failures is sound")
def _one_is_enough():
    # THE CRITERION. A case is red because its run failed, and one failing
    # test fails a run. A check demanding that the scope select EVERY failure
    # calls a deliberately cheap declaration broken -- and the whole reason
    # `--scope` exists is to be cheap.
    failures = {"lib": {"formats::record::tests::a_row", "capture::router::tests::b"}}
    hit = selected("lib/formats::record", failures, set())
    if hit != {"formats::record::tests::a_row"}:
        return f"selected {sorted(hit)}, not the one failure the filter names"
    return None


@fixture("a scope selecting none of them is refused")
def _none_is_a_fast_green():
    # The other direction, which is the defect: the scope runs tests that all
    # pass, the run exits 0, and the case reports GREEN -- the gate did not
    # fire, over a fault that was seeded.
    failures = {"lib": {"formats::record::tests::a_row"}}
    if selected("test:cli", failures, set()):
        return "a scope naming another target selected something"
    if selected("lib/formats::regimen", failures, set()):
        return "a filter in none of the failing names selected something"
    why = why_none("test:cli", failures)
    if "lib" not in why:
        return f"the refusal does not say where the failures are: {why!r}"
    why = why_none("lib/formats::regimen", failures)
    if "formats::regimen" not in why or "a_row" not in why:
        return f"the refusal names neither the filter nor a failure it missed: {why!r}"
    return None


@fixture("the filter matches a substring and not a prefix")
def _filter_is_a_substring():
    # `cargo test -- record` runs every test whose FULL name contains `record`,
    # which is what the committed declarations rely on: `lib/object` selects
    # `capture::object::tests::x`. Reading it as a prefix here calls a working
    # declaration broken and sends somebody to fix a scope that is right.
    failures = {"lib": {"capture::object::tests::the_regime_moves"}}
    if not selected("lib/object", failures, set()):
        return "a filter matched in the middle of the name selected nothing"
    return None


@fixture("a scope over a target that did not build is sound with nothing to select")
def _unbuilt_needs_no_name():
    # A target that does not compile ran no tests, so there is no name for a
    # filter to hit -- and the scope is red anyway, because cargo fails before
    # it selects anything. Reading "selected nothing" as the defect would
    # refuse every build-breaking fault in the manifest.
    if not reaches_unbuilt("lib", {"lib"}):
        return "a scope naming the target that failed to build did not reach it"
    if not reaches_unbuilt("all", {"lib"}):
        return "`all` did not reach a target that failed to build"
    if not reaches_unbuilt("lib/formats::record", {"lib"}):
        return "a filter stopped the scope from reaching a build failure it cannot narrow"
    if reaches_unbuilt("test:cli", {"lib"}):
        return "a scope naming another target reached the build failure"
    return None


@fixture("a derived filter names a module and never half an identifier")
def _prefix_stops_at_a_module():
    # `formats::record` and `formats::recall` share the characters `formats::rec`
    # and share the MODULE `formats`. A character-wise prefix is a filter that
    # matches by substring, so it selects both today and quietly selects a
    # different set the first time either is renamed.
    failures = {"lib": {"formats::record::tests::a", "formats::recall::tests::b"}}
    got = derived(failures, set())
    if got != "lib/formats":
        return f"derived {got!r}, not 'lib/formats'"
    return None


@fixture("a derived filter drops the test function and keeps a root-level name")
def _prefix_drops_the_function():
    # A filter naming one function is the narrowest scope there is and the
    # most brittle: rename the test and the declaration selects nothing, a
    # rename away from where the rename happened. The module is what a harvest
    # proposes -- unless dropping it would leave no filter at all.
    one = derived({"lib": {"formats::record::tests::a_row"}}, set())
    if one != "lib/formats::record::tests":
        return f"a single failure derived {one!r}, not the module that holds it"
    shorter = derived({"lib": {"capture::a", "capture::a::b"}}, set())
    if shorter != "lib/capture":
        return f"a name that is a prefix of another derived {shorter!r}, not 'lib/capture'"
    root = derived({"test:cli": {"a_usage_error_exits_two"}}, set())
    if root != "test:cli/a_usage_error_exits_two":
        return f"a crate-root test derived {root!r}, which names no test at all"
    return None


@fixture("two targets derive all, with a filter only when they share a module")
def _two_targets_derive_all():
    # No spec names two targets, so `all` is the only one that keeps both --
    # but `all` still takes a filter, and a shared module is narrower than the
    # bare target while still selecting every failure.
    shared = derived(
        {
            "lib": {"formats::record::tests::a"},
            "test:conformance": {"formats::record::b"},
        },
        set(),
    )
    if shared != "all/formats::record":
        return f"two targets under one module derived {shared!r}"
    apart = derived(
        {"lib": {"drive::tests::a"}, "test:conformance": {"formats::b"}},
        set(),
    )
    if apart != "all":
        return f"two targets with nothing in common derived {apart!r}, not 'all'"
    # A target that did not compile contributes no names, so a filter drawn
    # from the other target's names would exclude it.
    unbuilt = derived({"test:conformance": {"formats::b"}}, {"lib"})
    if unbuilt != "all":
        return f"a build failure beside a test failure derived {unbuilt!r}, not 'all'"
    return None


@fixture("a failure before any target is refused")
def _orphan_failure_is_refused():
    # Nothing in the log says which target ran it, so nothing in the log says
    # what to scope to. Attributing it to whatever `Running` line comes next
    # would put the fault's name under a harness that never saw it.
    try:
        broke(failed("formats::record::tests::a_row"))
    except Unusable:
        return None
    return "a failure with no target above it was attributed to one"


@fixture("an index of nothing is refused")
def _empty_index_is_refused():
    # An empty index is what a `--derive-scopes` run that never ran looks
    # like, and reading it as "every declaration checked out" is a check of
    # nothing reporting a pass.
    with tempfile.TemporaryDirectory() as box:
        empty = pathlib.Path(box) / "empty.tsv"
        empty.write_text("\n\n", encoding="utf-8")
        try:
            read_index(empty)
        except Unusable:
            pass
        else:
            return "an index naming no case was read as an index"
        short = pathlib.Path(box) / "short.tsv"
        short.write_text("a.log\ta label\n", encoding="utf-8")
        try:
            read_index(short)
        except Unusable:
            pass
        else:
            return "a row missing its declared scope was read as a row"
        missing = pathlib.Path(box) / "gone.tsv"
        try:
            read_index(missing)
        except Unusable:
            return None
        return "an index that does not exist was read as an index"


@fixture("the exit codes say which of the three things happened")
def _exit_codes_are_distinct():
    # The entry point, driven end to end. `check-merge-gate.py` records that
    # its own entry point went untested until a review found it, and the
    # lesson transfers: every leg above can be right while main returns 0 for
    # all of them.
    with tempfile.TemporaryDirectory() as box:
        room = pathlib.Path(box)
        (room / "sound.log").write_text(LIB_RAN + failed("formats::record::tests::a_row"))
        (room / "wrecked.log").write_text(WRECKED_LOG)

        def index(name: str, *rows: str) -> str:
            path = room / name
            path.write_text("".join(f"{row}\n" for row in rows), encoding="utf-8")
            return str(path)

        def drive(*argv: str) -> tuple[int, str, str]:
            """main() with its two streams caught, so a fixture reads them.

            Caught rather than let through: a suite whose own output is the
            tool's output is a suite nobody reads, and the exit code alone
            cannot say whether the refusal named the case it refused.
            """
            out, err = io.StringIO(), io.StringIO()
            with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
                code = main(list(argv))
            return code, out.getvalue(), err.getvalue()

        sound = index(
            "sound.tsv",
            f"{room / 'sound.log'}\ta sound case\tlib/formats::record",
            f"{room / 'wrecked.log'}\ta build-breaking case\tlib",
        )
        code, out, _ = drive("--index", sound)
        if code != 0:
            return "a sound index did not exit 0"
        if "2 declaration(s)" not in out:
            return f"the pass did not say how many declarations it checked: {out!r}"
        code, out, _ = drive("--index", sound, "--emit")
        if code != 0:
            return "emitting from a sound index did not exit 0"
        if sorted(out.splitlines()) != sorted(
            ["lib/formats::record::tests\ta sound case", "lib\ta build-breaking case"]
        ):
            return f"--emit printed {out!r}"

        unsound = index(
            "unsound.tsv", f"{room / 'sound.log'}\ta case scoped elsewhere\ttest:cli"
        )
        code, _, err = drive("--index", unsound)
        if code != EXIT_BAD:
            return f"a declaration selecting nothing did not exit {EXIT_BAD}"
        if "a case scoped elsewhere" not in err:
            return f"the refusal did not name the case it refused: {err!r}"

        absent = index("absent.tsv", f"{room / 'nowhere.log'}\ta case whose log is gone\tlib")
        if drive("--index", absent)[0] != EXIT_BROKEN:
            return f"an unreadable log did not exit {EXIT_BROKEN}"
        if drive("--selftest", "--emit")[0] != EXIT_BROKEN:
            return f"--selftest beside --emit did not exit {EXIT_BROKEN}"
        if drive()[0] != EXIT_BROKEN:
            return f"no index and no --selftest did not exit {EXIT_BROKEN}"
    return None


def selftest() -> int:
    """Run every fixture, reporting each by name."""
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
        f"derive-scopes: {len(FIXTURES)} fixture(s), {len(broken)} failed",
        file=sys.stderr if broken else sys.stdout,
    )
    return EXIT_BAD if broken else 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--index", help="the TSV --derive-scopes wrote")
    parser.add_argument(
        "--emit",
        action="store_true",
        help="print the derived declarations rather than checking the declared ones",
    )
    parser.add_argument(
        "--selftest",
        action="store_true",
        help="run the fixture suite over this file's reader and exit",
    )
    args = parser.parse_args(argv)

    if args.selftest:
        if args.index or args.emit:
            print(
                "derive-scopes: --selftest exercises the reader on fixtures and reads "
                "no index; it does not combine with --index or --emit",
                file=sys.stderr,
            )
            return EXIT_BROKEN
        return selftest()
    if not args.index:
        print("derive-scopes: --index is required unless --selftest is given", file=sys.stderr)
        return EXIT_BROKEN

    try:
        rows = read_index(pathlib.Path(args.index))
    except Unusable as err:
        print(f"derive-scopes: {err}", file=sys.stderr)
        return EXIT_BROKEN

    bad = 0
    margin: list[str] = []
    for log_path, label, declared in rows:
        try:
            log = log_path.read_text(encoding="utf-8", errors="replace")
        except OSError as err:
            print(f"derive-scopes: {label}: cannot read {log_path}: {err}", file=sys.stderr)
            return EXIT_BROKEN
        try:
            failures = broke(log)
            unbuilt = wrecked(log)
            want = derived(failures, unbuilt)
        except Unusable as err:
            print(f"derive-scopes: {label}: {err}", file=sys.stderr)
            return EXIT_BROKEN

        if args.emit:
            print(f"{want}\t{label}")
            continue

        hit = selected(declared, failures, unbuilt)
        if not hit and not reaches_unbuilt(declared, unbuilt):
            print(
                f"derive-scopes: {label}: the declared scope {declared!r} selects "
                f"nothing this fault breaks -- {why_none(declared, failures)}. A "
                f"case scoped past its own failure is a fast green. A harvest of "
                f"this run declares {want!r}",
                file=sys.stderr,
            )
            bad += 1
            continue

        total = sum(len(names) for names in failures.values())
        if len(hit) < total:
            margin.append(
                f"  {label}: {declared} selects {len(hit)} of the {total} test(s) "
                f"this fault breaks; a harvest declares {want}"
            )

    if args.emit:
        return 0
    if bad:
        print(
            f"derive-scopes: {bad} declaration(s) select no test their seeded fault "
            f"breaks; such a case runs tests that all pass and reports a fast green",
            file=sys.stderr,
        )
        return EXIT_BAD
    print(f"derive-scopes: {len(rows)} declaration(s) each select a test their fault breaks")
    if margin:
        print(
            f"derive-scopes: {len(margin)} of them are NARROWER than the fault's full "
            f"blast radius. That is what --scope is for and is not a defect; it is "
            f"listed because a scope riding on fewer tests is a scope a rename can "
            f"silence sooner."
        )
        print("\n".join(margin))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
