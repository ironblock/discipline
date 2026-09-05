#!/usr/bin/env python3
"""The merge resolver is exercised before it is trusted to resolve a merge.

`scripts/merge-gate.py` rebuilds `verify.sh` and `tools/gate/faults.toml`
from two sides of a merge by NAME, because a line-oriented resolver splices
one injection body into the next and leaves anchors pointing at text that
moved. It is the tool every lane merge in this repository runs through.

It was also, for its first three hundred lines, reachable from nothing: no
check, no CI job, no seeded fault. A resolver that cannot fail is worse than
a resolver nobody wrote, because its output looks like a decision. Five of
the defects a review found in it -- an entry spliced ahead of the last line
of a manifest that ends without a newline, a block keyed by a name in its
own rationale comment, a case regex blind to a continuation line and loud
about a commented-out one, an inert report read as an empty one -- were all
behaviours nothing had ever executed.

So each of those is a fixture here, named for what it protects, and the
suite is the check. A fixture that goes green for the wrong reason is worth
nothing, so each asserts on the specific value it is about rather than on
"no exception was raised".
"""

import importlib.util
import re
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def load(name: str, filename: str):
    spec = importlib.util.spec_from_file_location(name, ROOT / "scripts" / filename)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


MG = load("merge_gate", "merge-gate.py")
GATELIB = load("gatelib", "gatelib.py")

FIXTURES: list[tuple[str, object]] = []


def fixture(name: str):
    def take(fn):
        FIXTURES.append((name, fn))
        return fn

    return take


# --------------------------------------------------------------------------
# splicing
# --------------------------------------------------------------------------

MANIFEST = (
    '[[fault]]\nid = "a.first"\nkind = "seeded-gate"\nmigrated = false\n\n'
    '[[fault]]\nid = "a.second"\nkind = "seeded-gate"\nmigrated = false\n'
)
NEW = '[[fault]]\nid = "a.third"\nkind = "seeded-gate"\nmigrated = false\n'


@fixture("an entry splices into a manifest whose last line has no newline")
def _splice_without_newline():
    # The repository's own faults.toml ended `migrated = false` with no
    # newline for its whole life. The entry pattern could not consume that
    # last line, so the new entry landed AHEAD of it and duplicated the key.
    for label, text in (("with", MANIFEST), ("without", MANIFEST.rstrip("\n"))):
        out = MG.insert_after_last(text, MG.ENTRY, [NEW])
        try:
            doc = tomllib.loads(out)
        except ValueError as err:
            return f"{label} a trailing newline, the splice is not TOML: {err}"
        ids = [f["id"] for f in doc["fault"]]
        if ids != ["a.first", "a.second", "a.third"]:
            return f"{label} a trailing newline, the entries came out as {ids}"
    return None


# --------------------------------------------------------------------------
# keying
# --------------------------------------------------------------------------


@fixture("a block is keyed by its own function, not by a name in its comment")
def _key_is_the_function():
    # A rationale comment naming another injection -- "the deliberate pair of
    # inject_alpha" -- mis-keyed the block it introduced. The union then saw
    # a name it already had and skipped the genuinely new body, silently.
    text = (
        "# The deliberate pair of inject_alpha: the two anchor on one line,\n"
        "# which is how a merge once deleted both.\n"
        "inject_beta() {\n  :\n}\n"
    )
    found = MG.find_blocks(MG.FUNC, text)
    if [name for name, _ in found] != ["inject_beta"]:
        return f"keyed as {[n for n, _ in found]}, not ['inject_beta']"
    return None


@fixture("a union keeps a block whose comment names another injection")
def _union_keeps_miskeyed():
    ours = "inject_alpha() {\n  :\n}\n"
    theirs = ours + (
        "# The deliberate pair of inject_alpha.\ninject_beta() {\n  :\n}\n"
    )
    mine = {n for n, _ in MG.find_blocks(MG.FUNC, ours)}
    added = [b for n, b in MG.find_blocks(MG.FUNC, theirs) if n not in mine]
    if len(added) != 1 or "inject_beta()" not in added[0]:
        return f"the union took {len(added)} block(s); inject_beta was dropped"
    return None


# --------------------------------------------------------------------------
# reading a seeded case
# --------------------------------------------------------------------------

CONTINUED = (
    '  seeded_case "a case over three lines" \\\n'
    "    injections \\\n"
    "    inject_spelled_across_lines \\\n"
    "    'a signature'\n"
)
COMMENTED = (
    '  # seeded_case "a parked case" injections inject_not_written_yet \\\n'
    "  #   'a signature'\n"
)


@fixture("a seeded case spelled across continuation lines is seen")
def _case_over_continuations():
    named = {c.injection for c in GATELIB.seeded_cases(CONTINUED)}
    if named != {"inject_spelled_across_lines"}:
        return f"the reader saw {named or 'nothing'}, not the case it was given"
    return None


@fixture("a commented-out seeded case is not a case")
def _commented_case():
    named = {c.injection for c in GATELIB.seeded_cases(COMMENTED)}
    if named:
        return f"a commented-out case was read as live: {named}"
    return None


@fixture("a seeded case keeps its label and signature through the reader")
def _case_carries_its_fields():
    # The manifest checks each entry's label and signature against the case
    # that proves it, so a reader that finds the case but loses its fields
    # turns that check into a comparison of None with None.
    cases = GATELIB.seeded_cases(CONTINUED)
    if len(cases) != 1:
        return f"{len(cases)} case(s) read from one call"
    case = cases[0]
    if (case.label, case.check, case.signature) != (
        "a case over three lines",
        "injections",
        "a signature",
    ):
        return f"the fields came back as {case!r}"
    return None


@fixture("the resolver's case blocks and the shared reader name the same cases")
def _span_and_reader_agree():
    # merge-gate keeps a pattern of its own for seeded cases, and must: it
    # splices whole blocks, so it needs their extent, which a field reader
    # does not give it. What it may not do is disagree about WHICH cases
    # exist -- a case its pattern misses is a case the union silently drops,
    # and a case it invents is a block spliced out of somebody's comment.
    text = (ROOT / "verify.sh").read_text(encoding="utf-8")
    spanned = {name for name, _ in MG.find_blocks(MG.CASE, text)}
    read = {case.injection for case in GATELIB.seeded_cases(text)}
    if spanned != read:
        only_span = sorted(spanned - read)[:3]
        only_read = sorted(read - spanned)[:3]
        return (
            f"the resolver spans {len(spanned)} case(s), the reader finds "
            f"{len(read)}; only the resolver sees {only_span}, only the "
            f"reader sees {only_read}"
        )
    return None


@fixture("no second reader parses a seeded case's FIELDS")
def _one_field_reader():
    # Extents are merge-gate's business. Label, check, injection and
    # signature are gatelib's, and two readers of those WILL drift -- they
    # did, invisibly, because each script passed its own checks against its
    # own reading.
    offenders = []
    for script in sorted((ROOT / "scripts").glob("*.py")):
        if script.name in ("gatelib.py", "check-merge-gate.py", "merge-gate.py"):
            continue
        for line in script.read_text(encoding="utf-8").splitlines():
            if "seeded_case" in line and "re.compile" in line:
                offenders.append(f"{script.name}: {line.strip()[:70]}")
    if offenders:
        return "a second field reader exists:\n      " + "\n      ".join(offenders)
    return None


@fixture("the shared reader agrees with verify.sh's own count of cases")
def _reader_matches_the_tree():
    # A parser that silently reads zero cases would satisfy every fixture
    # above. Hold it against the file it exists to read.
    text = (ROOT / "verify.sh").read_text(encoding="utf-8")
    cases = GATELIB.seeded_cases(text)
    calls = sum(
        1
        for line in text.splitlines()
        if line.strip().startswith("seeded_case ")
    )
    if not cases:
        return "the reader found no seeded cases in verify.sh at all"
    if len(cases) != calls:
        return f"the reader found {len(cases)} case(s); verify.sh spells {calls}"
    missing = [c.injection for c in cases if c.injection not in text]
    if missing:
        return f"named injections that do not appear in the file: {missing[:3]}"
    return None


# --------------------------------------------------------------------------
# asking the pre-flight
# --------------------------------------------------------------------------

STUB = """#!/usr/bin/env python3
import sys
print("check-injections: 1 seeded case(s) name an injection that is not defined",
      file=sys.stderr)
raise SystemExit(1)
"""


@fixture("a pre-flight that reported no summary is not read as nothing-inert")
def _no_summary_is_not_empty():
    # `check-injections.py` refuses a case naming no injection by returning 1
    # from an early branch that prints to stderr -- so stdout carries no
    # summary line. Reading that as "the list of inert injections is empty"
    # turns the one tree this tool exists for into a no-op.
    with tempfile.TemporaryDirectory() as box:
        root = Path(box)
        (root / "scripts").mkdir()
        (root / "scripts" / "check-injections.py").write_text(STUB, encoding="utf-8")
        answer = MG.inert_injections(root)
    if answer is not None:
        return f"read a summary-less refusal as {answer!r} instead of 'could not ask'"
    return None


def main() -> int:
    failed = []
    for name, fn in FIXTURES:
        try:
            why = fn()
        except Exception as err:  # a fixture that explodes is a fixture that failed
            why = f"{type(err).__name__}: {err}"
        print(f"{'ok  ' if why is None else 'FAIL'}  {name}")
        if why is not None:
            failed.append((name, why))
    for name, why in failed:
        print(f"  {name}\n    {why}", file=sys.stderr)
    print(
        f"check-merge-gate: {len(FIXTURES)} fixture(s), {len(failed)} failed",
        file=sys.stderr if failed else sys.stdout,
    )
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
