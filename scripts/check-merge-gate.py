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
    # Tabled over the spellings a rationale comment actually uses. One
    # sample lets a keyer that merely drops the `^...() {` anchor pass: a
    # comment written "inject_alpha()" then keys the block again, which is
    # the partial fix a single-sample green would bless.
    for comment in (
        "# The deliberate pair of inject_alpha: the two anchor on one line.\n",
        "# The deliberate pair of inject_alpha(): the two anchor on one line.\n",
        "# Pairs with inject_alpha() { ... }, which a merge once deleted.\n",
    ):
        text = comment + "inject_beta() {\n  :\n}\n"
        found = MG.find_blocks(MG.FUNC, text)
        if [name for name, _ in found] != ["inject_beta"]:
            return (
                f"keyed as {[n for n, _ in found]}, not ['inject_beta'], "
                f"for a comment spelled {comment.strip()!r}"
            )
        # And the comment must still be part of the block. If it stops being
        # swallowed, keying gets easier and this fixture's own precondition
        # quietly disappears -- along with the rationale a merge is supposed
        # to carry with the body.
        if comment not in found[0][1]:
            return "the block was keyed correctly but left its rationale comment behind"

    # A comment naming an injection with no definition under it is not a
    # block: a keyer that reads names out of prose invents them.
    if MG.find_blocks(MG.FUNC, "# inject_alpha is mentioned and not defined\n"):
        return "a comment with no definition under it was read as a block"
    return None


@fixture("a union keeps a block whose comment names another injection")
def _union_keeps_miskeyed():
    # Exact equality, not a count and a substring. The extent is the thing:
    # a greedy pattern swallows alpha's body into beta's block, a pattern
    # that drops the comment group leaves the rationale behind, and one that
    # loses the trailing newline breaks the splice -- and a substring test
    # sees none of the three.
    beta = "# The deliberate pair of inject_alpha.\ninject_beta() {\n  :\n}\n"
    ours = "inject_alpha() {\n  :\n}\n"
    theirs = ours + beta
    mine = {n for n, _ in MG.find_blocks(MG.FUNC, ours)}
    added = [b for n, b in MG.find_blocks(MG.FUNC, theirs) if n not in mine]
    if added != [beta]:
        return f"the union took {added!r}, not exactly theirs' new block with its comment"
    return None


@fixture("the resolver spans every injection verify.sh defines")
def _func_spans_the_tree():
    # The counterpart the suite was missing. Seeded cases are held against
    # the real file twice over; injection bodies were held against nothing,
    # so narrowing the name class to `inject_[a-z]+` found 12 blocks where
    # the tree has 105 -- a union would have dropped 93 injections in
    # silence, with every fixture green.
    text = (ROOT / "verify.sh").read_text(encoding="utf-8")
    found = MG.find_blocks(MG.FUNC, text)
    spanned = {name for name, _ in found}
    # Counted a second way, without the block pattern, so the two readings
    # cannot drift together.
    defined = set(re.findall(r"^(inject_[a-z0-9_]+)\(\) \{", text, re.M))
    if not defined:
        return "no injection definitions found in verify.sh at all"
    if spanned != defined:
        missed = sorted(defined - spanned)[:3]
        invented = sorted(spanned - defined)[:3]
        return (
            f"the resolver spans {len(spanned)} injection(s); verify.sh "
            f"defines {len(defined)}. Missed {missed}; invented {invented}"
        )
    lines = text.splitlines(True)
    for name, block in found:
        # Every block ends at its own closing brace, with the newline the
        # splice depends on.
        if not block.endswith("}\n"):
            return f"{name}'s block does not end at a closing brace and newline"
        # And where the file puts a comment immediately above a definition,
        # that comment is part of the block -- the reason travels with the
        # body, which is this resolver's own stated law.
        start = next(
            (i for i, line in enumerate(lines) if line.startswith(f"{name}() {{")),
            None,
        )
        if start and lines[start - 1].startswith("#") and lines[start - 1] not in block:
            return f"{name} left the rationale comment directly above it behind"
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
# what counts as a difference
# --------------------------------------------------------------------------

META = (
    "[meta]\nred_faults = {red}\nmechanics_assertions = {mech}\n\n"
    '[[fault]]\nid = "a.first"\nkind = "seeded-gate"\nmigrated = false\n'
)


@fixture("a total this tool recounts is not a difference between the sides")
def _recountable_totals_are_not_differences():
    # Both sides derive `red_faults` and `mechanics_assertions` from the
    # assembled tree, and the union recomputes both. Refusing a merge over a
    # number it is about to overwrite is a refusal with nothing in it -- and
    # it fired on every branch that added a mechanics assertion, which is
    # most of them.
    ours = META.format(red=165, mech=27)
    theirs = META.format(red=166, mech=28)
    if MG.skeleton_of(ours, (MG.ENTRY,)) != MG.skeleton_of(theirs, (MG.ENTRY,)):
        return "two manifests differing only in their recounted totals read as different"
    # ... but a real difference outside the blocks must still be one.
    altered = theirs.replace("[meta]", "[meta]\nsomething_else = true")
    if MG.skeleton_of(ours, (MG.ENTRY,)) == MG.skeleton_of(altered, (MG.ENTRY,)):
        return "a genuine difference outside the blocks was normalized away"
    return None


@fixture("an added mechanics assertion is a difference, not a silent union")
def _mechanics_assertion_is_a_difference():
    # `expect_exit` is deliberately NOT given a block pattern. It is a
    # statement inside a check function with undelimited setup above it, so a
    # pattern guessing its extent would splice one assertion's setup onto
    # another -- the exact hazard this tool exists to prevent. Refusing is
    # the correct answer; what was wrong was refusing without saying why.
    # This fixture pins the refusal, so nobody "fixes" it into a guess.
    body = (
        "check_hygiene() {\n"
        '  mkdir -p "${box}/one"\n'
        '  expect_exit "the first mechanic" 1 \\\n'
        '    bash scripts/hygiene.sh --tree "${box}/one"\n'
        "}\n"
    )
    grown = body.replace(
        "}\n",
        '\n  mkdir -p "${box}/two"\n'
        '  expect_exit "a brand new mechanic" 2 \\\n'
        '    bash scripts/hygiene.sh --tree "${box}/two"\n}\n',
    )
    if MG.skeleton_of(body, (MG.FUNC, MG.CASE)) == MG.skeleton_of(
        grown, (MG.FUNC, MG.CASE)
    ):
        return "an added mechanics assertion was treated as no difference at all"
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
