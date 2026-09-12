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

The suite was then put to a second, harder question. Not "does each fixture
catch its own lesion" -- all thirteen do, each seen red under a lesion to the
behaviour it names, none collateral -- but "could that behaviour be broken
ANOTHER way with the suite still green?" It could, eleven times. Four are
closed, in the commit that added the thirteenth fixture.

Of the remaining eight, one is now closed too -- the entry point, below --
leaving SEVEN OPEN. Not one of the seven is a hypothesis: every one names a
mutation that was applied to a copy of this tree and left the suite reporting
`0 failed`. They are recorded here, rather than only in a tracker, because
the fixture each one indicts is a few lines further down this file. They are
listed in the order their consequence matters, worst first.

  1. `a seeded case spelled across continuation lines is seen` -- drop the
     `held = []` reset in gatelib.logical_lines and the real verify.sh reads
     0 cases, down from 105. Green: the assertion is a SET of one field, and
     the input is a lone call with no line before it or after it.

  2. `a commented-out seeded case is not a case` -- anchor only half the
     guard and `#seeded_case ...` with NO space after the hash (what an
     editor's comment-toggle emits) comes back as a live case naming an
     injection that exists nowhere: the precise false red this guard was
     written to prevent. Green by token-offset coincidence -- the fixture's
     own input spells it `# ` with a space, which shifts the fields.

  3. `a seeded case keeps its label and signature through the reader` --
     truncate a label at its first comma, or strip a signature's
     backslashes, and check-fault-manifest.py disagrees with verify.sh about
     the case that proves each fault. Green: the one synthetic call has no
     comma, no backslash and no metacharacter in it, and no fixture anywhere
     holds a FIELD against the real tree.

  4. `no second reader parses a seeded case's FIELDS` -- the assertion is a
     two-substring grep on one physical line, so it pins a SPELLING and not
     a behaviour. The same regression was reproduced three ways, all green:
     `re.compile(` wrapped onto the next line (an idiom already present in
     this directory), an inline `re.findall`, and a hand-rolled `.split()`
     reader -- which reads 0 of 105 cases and turns check-injections' "does
     every case name an injection that exists?" into a no-op that passes
     everything.

  5. `the shared reader agrees with verify.sh's own count of cases` -- the
     count leg is strong and tree-anchored. The `c.injection not in text`
     leg has no discriminating power at all, because any token the reader
     copies out of a line is by construction a substring of the file. Pair
     every case with the NEXT case's injection name: count, names and set
     all stay right, the whole suite stays green, and every label and
     signature is now attached to the wrong injection -- exactly the pairing
     check-fault-manifest.py depends on.

  6. `a total this tool recounts is not a difference between the sides` --
     the negative control is `something_else = true`, a boolean, so it is
     immune to the over-normalizations that actually happen to a numeric
     normalizer. Three were run green, among them one under which
     `migrated = 0` and `migrated = 7` read as no difference at all, so one
     side's authored count is silently written to disk.

  7. `a pre-flight that reported no summary is not read as nothing-inert` --
     partly closed. Its caller escape is fixed: `repair` no longer reads a
     None as nothing-inert. Two remain. The POSITIVE arm is unpinned and is
     coupled across two files by a bare string literal -- check-injections.py
     prints the summary, merge-gate.py recognises it by `startswith` -- so
     rewording either leaves inert_injections returning None forever. And
     the stub writes nothing at all to stdout, so the fixture cannot tell
     "keys on the summary line" from "keys on any output whatsoever".

CLOSED, and the reason the rest are worth reading: no fixture in this file
called `union_file` at all -- the resolver was given a gate, and the gate's
fixtures reached its helpers rather than its entry point, which is the
review's finding 12 recurring one level down. Turning the refusal into
warn-and-proceed, and normalizing `expect_exit` away inside `skeleton_of`,
were both invisible. Two fixtures now drive `union_file` itself, and both
mutations go red under them. Take that as the standing rule for anyone
adding a fixture here: assert through the function a merge actually calls.
"""

import contextlib
import importlib.util
import io
import os
import re
import shutil
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


@fixture("a case the reader cannot name is refused, never dropped")
def _unkeyable_case_refused():
    # THE SHAPE: valid bash, a real `seeded_case` call, and missing an
    # argument the field reader needs. `CASE` matches it, `key_of(CASE)`
    # returns nothing -- and the same pattern strips it before the skeleton
    # comparison, so the "differ outside the named blocks" refusal cannot see
    # it either. Dropped, it is a block a union writes a file without, at
    # exit 0. That is the failure this whole script exists to prevent.
    text = (
        "  seeded_case inject_only_one_argument \\\n"
        "    'a signature'\n"
    )
    try:
        found = MG.find_blocks(MG.CASE, text)
    except MG.Unkeyable:
        return None
    return (
        f"a case the reader cannot name came back as {found!r} instead of a "
        f"refusal, so a union would drop it and report success"
    )


@fixture("a refusal exits 2, not the code for a finding")
def _refusal_exit_code():
    # `sys.exit(f"...")` prints and exits ONE. One is this gate's code for
    # "the scan ran and found something", so every refusal reported itself as
    # a finding. The two have to be different numbers or a caller cannot tell
    # "clean" from "broken" apart from "dirty".
    import subprocess

    done = subprocess.run(
        [
            sys.executable,
            str(ROOT / "scripts" / "merge-gate.py"),
            "--union",
            "a-ref-this-repository-does-not-have",
            "HEAD",
        ],
        capture_output=True,
        text=True,
        cwd=ROOT,
    )
    if done.returncode != 2:
        return (
            f"pointing the union at a ref that is not there exited "
            f"{done.returncode}, and a refusal is 2: {done.stderr.strip()[:120]}"
        )
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


# Two commits of one gate file, so `union_file` can be driven the way a merge
# drives it -- through `git show`, against a real path on disk. Everything
# below this point asserts on the ENTRY POINT rather than on a helper: the
# review's finding 12 was a resolver reachable from nothing, and a suite that
# only reaches `skeleton_of` and `find_blocks` reproduces it one level down.
CHECK_BODY = """\
check_thing() {
  local box="${1}"
  mkdir -p "${box}/one"
  expect_exit 'the first thing' 0 true
}

inject_alpha() {
  :
}
"""

# An assertion added inside an existing check function, bringing no setup with
# it. This is the common shape, and the one an `expect_exit` normalizer in
# `skeleton_of` would make compare EQUAL.
CHECK_GROWN = CHECK_BODY.replace(
    "  expect_exit 'the first thing' 0 true\n",
    "  expect_exit 'the first thing' 0 true\n"
    "  expect_exit 'the second thing' 1 false\n",
)

BETA = "\n# The deliberate pair of inject_alpha.\ninject_beta() {\n  :\n}\n"


def two_commits(box: Path, ours_text: str, theirs_text: str) -> tuple[str, str]:
    """One file, committed twice. Returns the two SHAs."""
    def git(*args: str) -> str:
        run = subprocess.run(
            ("git",) + args, cwd=box, capture_output=True, text=True
        )
        if run.returncode != 0:
            raise RuntimeError(f"git {' '.join(args)}: {run.stderr.strip()}")
        return run.stdout.strip()

    git("init", "-q")
    git("config", "user.email", "gate@example.invalid")
    git("config", "user.name", "gate")
    shas = []
    for text in (ours_text, theirs_text):
        (box / "verify.sh").write_text(text, encoding="utf-8")
        git("add", "verify.sh")
        git("commit", "-qm", "side")
        shas.append(git("rev-parse", "HEAD"))
    return shas[0], shas[1]


def drive_union(ours_text: str, theirs_text: str):
    """Run `union_file` in a throwaway repo. Returns (took, stderr, on disk)."""
    with tempfile.TemporaryDirectory() as tmp:
        box = Path(tmp)
        ours, theirs = two_commits(box, ours_text, theirs_text)
        here = os.getcwd()
        err = io.StringIO()
        try:
            os.chdir(box)
            with contextlib.redirect_stderr(err), contextlib.redirect_stdout(io.StringIO()):
                took = MG.union_file(Path("verify.sh"), ours, theirs)
        finally:
            os.chdir(here)
        return took, err.getvalue(), (box / "verify.sh").read_text(encoding="utf-8")


# A REAL merge shape, unlike two_commits above: a base, and two branches off
# it. `two_commits` makes ours the PARENT of theirs, so their merge base is
# ours -- which is a fine way to drive `union_file` but cannot exercise a rule
# about what the base says, because the base and one side are the same commit.
BASE_PAIR = """\
inject_alpha() {
  echo base > a.txt
}

inject_beta() {
  echo base > b.txt
}
"""
# No backslashes anywhere in these fixtures, deliberately: the first version
# used `printf 'base\\n'`, and the assertions then compared a real newline
# against the two characters on disk and reported a working union as broken.
ALPHA_ONE = BASE_PAIR.replace("echo base > a.txt", "echo one > a.txt")
ALPHA_TWO = BASE_PAIR.replace("echo base > a.txt", "echo two > a.txt")

# Manifest-shaped, for the presence rule over `tools/gate/faults.toml`.
#
# A CORRECTION. The commit that added these said they were driven over the
# manifest rather than verify.sh because "removing an injection moves the
# text around it, so the skeleton refusal fires first and the presence rule
# is never reached". THAT IS FALSE, and the line that falsifies it is the
# last line of `skeleton_of` -- `re.sub(r"\n{2,}", "\n\n", text)`, whose own
# comment says runs of blank lines are not a difference. Measured over the
# real verify.sh at the head that shipped the claim:
#
#   remove each of the 223 injections    223 skeletons identical, 0 differ
#   remove each of the 223 seeded cases  223 skeletons identical, 0 differ
#
# So the presence rule was reachable in verify.sh all along, and the reason
# the fixtures covered one of three block kinds was not a constraint but an
# untested assumption stated as one. The verify.sh-shaped fixtures below
# close the other two.
FAULTS_PAIR = (
    '[[fault]]\nid = "a.alpha"\nkind = "seeded-gate"\nmigrated = false\n\n'
    '[[fault]]\nid = "a.beta"\nkind = "seeded-gate"\nmigrated = false\n'
)
FAULTS_BETA_ONLY = (
    '[[fault]]\nid = "a.beta"\nkind = "seeded-gate"\nmigrated = false\n'
)
FAULTS_ALPHA_EDITED = FAULTS_PAIR.replace(
    'id = "a.alpha"\nkind = "seeded-gate"\n',
    'id = "a.alpha"\nkind = "seeded-gate"\nlabel = "edited by theirs"\n',
)

# The pair with `inject_alpha` REMOVED -- a branch that retired a block on
# purpose, which is a different thing from a branch that never had it and the
# reason the union has to consult the base for presence as well as content.
BETA_ONLY = """\
inject_beta() {
  echo base > b.txt
}
"""

# verify.sh-shaped, carrying BOTH block kinds the resolver knows there.
# `union_file` runs FUNC and CASE as separate patterns over the same text, so
# a fixture that carries only injections proves the presence rule for one of
# the two -- and "one of two" is how this suite came to cover one of three.
VERIFY_PAIR = """\
inject_alpha() {
  echo base > a.txt
}

inject_beta() {
  echo base > b.txt
}

run_cases() {
  seeded_case "alpha goes red" fmt inject_alpha \\
    'alpha signature'
  seeded_case "beta goes red" fmt inject_beta \\
    'beta signature'
}
"""

# One variant per PATTERN, not one per file: removing the injection exercises
# FUNC with CASE untouched, and removing the case exercises CASE with FUNC
# untouched. Together they say which pattern a failure is in, which a single
# variant that removed both would not.
# EXACTLY the block `find_blocks` returns, and not the blank line framing it.
# Taking the blank line too is a different edit, and here it is a detectable
# one: these blocks sit at the head of the file, so `\n\n\n` at the start
# collapses to `\n\n` while `\n\n` stays `\n\n`, and the skeleton refusal
# fires before the presence rule is reached. Which is a real edge in
# `skeleton_of` -- its collapse of blank-line runs is not a no-op at a file
# boundary -- and is NOT the claim corrected above: removing each of the real
# verify.sh's 223 injections, as `find_blocks` delimits them, leaves the
# skeleton identical 223 times out of 223.
VERIFY_NO_ALPHA_FUNC = VERIFY_PAIR.replace(
    "inject_alpha() {\n  echo base > a.txt\n}\n", ""
)
VERIFY_NO_ALPHA_CASE = VERIFY_PAIR.replace(
    '  seeded_case "alpha goes red" fmt inject_alpha \\\n    \'alpha signature\'\n', ""
)
VERIFY_ALPHA_FUNC_EDITED = VERIFY_PAIR.replace(
    "echo base > a.txt", "echo edited-by-theirs > a.txt"
)
VERIFY_ALPHA_CASE_EDITED = VERIFY_PAIR.replace(
    "'alpha signature'", "'alpha signature, edited by theirs'"
)

# The presence rule has four rows and three block kinds. Naming the shapes
# once and generating the fixtures from them is what stops a fifth row, or a
# fourth kind, from being added for one and forgotten for the others -- which
# is the shape of the omission that left this suite covering a third of what
# it claimed.
#
# (label, file, both, alpha retired, alpha edited, alpha marker, beta marker)
PRESENCE_SHAPES = [
    (
        "a fault entry",
        "tools/gate/faults.toml",
        None,  # filled below: the manifest fixtures are defined above
        None,
        None,
        "a.alpha",
        "a.beta",
    ),
    (
        "an injection",
        "verify.sh",
        None,
        None,
        None,
        "inject_alpha() {",
        "inject_beta() {",
    ),
    (
        "a seeded case",
        "verify.sh",
        None,
        None,
        None,
        'seeded_case "alpha goes red"',
        'seeded_case "beta goes red"',
    ),
]
PRESENCE_SHAPES[0] = PRESENCE_SHAPES[0][:2] + (
    FAULTS_PAIR,
    FAULTS_BETA_ONLY,
    FAULTS_ALPHA_EDITED,
) + PRESENCE_SHAPES[0][5:]
PRESENCE_SHAPES[1] = PRESENCE_SHAPES[1][:2] + (
    VERIFY_PAIR,
    VERIFY_NO_ALPHA_FUNC,
    VERIFY_ALPHA_FUNC_EDITED,
) + PRESENCE_SHAPES[1][5:]
PRESENCE_SHAPES[2] = PRESENCE_SHAPES[2][:2] + (
    VERIFY_PAIR,
    VERIFY_NO_ALPHA_CASE,
    VERIFY_ALPHA_CASE_EDITED,
) + PRESENCE_SHAPES[2][5:]


def drive_three_way(
    base_text: str, ours_text: str, theirs_text: str, name: str = "verify.sh"
):
    """Run `union_file` over a base and two branches off it.

    `name` decides which patterns `union_file` uses -- (FUNC, CASE) for
    verify.sh, (ENTRY,) for anything else -- so it is what lets one set of
    presence fixtures range over all three block kinds. It was introduced
    on the false premise that removing a block from verify.sh trips the
    skeleton refusal; see the correction above the manifest fixtures."""
    with tempfile.TemporaryDirectory() as tmp:
        box = Path(tmp)

        def git(*args: str) -> str:
            run = subprocess.run(("git",) + args, cwd=box, capture_output=True, text=True)
            if run.returncode != 0:
                raise RuntimeError(f"git {' '.join(args)}: {run.stderr.strip()}")
            return run.stdout.strip()

        git("init", "-q", "-b", "base")
        git("config", "user.email", "gate@example.invalid")
        git("config", "user.name", "gate")
        (box / name).parent.mkdir(parents=True, exist_ok=True)
        (box / name).write_text(base_text, encoding="utf-8")
        git("add", name)
        git("commit", "-qm", "base")
        git("checkout", "-q", "-b", "ours")
        (box / name).write_text(ours_text, encoding="utf-8")
        # --allow-empty: a fixture in which OURS is the base is the whole
        # point of one of the cases below, and git will not commit nothing.
        git("commit", "-qam", "ours", "--allow-empty")
        ours = git("rev-parse", "HEAD")
        git("checkout", "-q", "base")
        git("checkout", "-q", "-b", "theirs")
        (box / name).write_text(theirs_text, encoding="utf-8")
        git("commit", "-qam", "theirs", "--allow-empty")
        theirs = git("rev-parse", "HEAD")
        # Standing on ours, merging theirs -- which is where a resolver runs,
        # and which makes "a refused union rewrote nothing" a claim about the
        # file the operator actually has.
        git("checkout", "-q", "ours")

        here = os.getcwd()
        err = io.StringIO()
        out = io.StringIO()
        try:
            os.chdir(box)
            with contextlib.redirect_stderr(err), contextlib.redirect_stdout(out):
                took = MG.union_file(Path(name), ours, theirs)
        finally:
            os.chdir(here)
        return took, out.getvalue(), err.getvalue(), (box / name).read_text(encoding="utf-8")


@fixture("a block only theirs changed is taken from theirs, not kept as ours")
def _union_takes_the_incumbents_correction():
    # The defect this rule exists for. `built = ours` plus append-what-ours-
    # lacks says nothing about a block BOTH sides carry, so keeping ours
    # reverted whichever side had corrected it -- silently, because from the
    # union's point of view nothing was added. Twelve reversions across four
    # lanes, one of them graded by nothing at all.
    took, out, err, on_disk = drive_three_way(BASE_PAIR, BASE_PAIR, ALPHA_ONE)
    if took is not True:
        return f"the union refused a block only one side changed: {err.strip()[:160]!r}"
    if "echo one > a.txt" not in on_disk:
        return "ours was kept over theirs, reverting the correction"
    if "inject_alpha" not in out:
        return "the correction was taken without saying so, which is how it went unnoticed"
    return None


@fixture("a block only ours changed stays ours")
def _union_keeps_our_own_change():
    # The other half, and the reason this is a three-way comparison rather
    # than "prefer theirs": preferring theirs would revert our change with
    # exactly the same silence, in the opposite direction.
    took, _out, err, on_disk = drive_three_way(BASE_PAIR, ALPHA_ONE, BASE_PAIR)
    if took is not True:
        return f"the union refused a block only we changed: {err.strip()[:160]!r}"
    if "echo one > a.txt" not in on_disk:
        return "our own change was replaced by the base"
    return None


# THE PRESENCE MATRIX. Six rows, three block kinds, generated rather than
# written out eighteen times.
#
# The rule that decides a block only one side carries has two halves and each
# half has three rows, and the suite that shipped it had three fixtures: one
# half, one kind. Both omissions were invisible because a fixture that is
# absent looks exactly like a fixture that passes.
#
#   ours has it, theirs does not          theirs has it, ours does not
#   --------------------------------      --------------------------------
#   not in base   ours added it, keep     not in base   theirs added, take
#   ours == base  THEIRS retired, drop    theirs==base  ours retired, keep
#   ours != base  a person's              theirs!=base  a person's
#
# Each half's "keep/take" row is the CONTROL for its "drop" row. Without it,
# a resolver that dropped every one-sided block satisfies the drop row and a
# resolver that kept every one-sided block satisfies nothing else.


def _presence_matrix():
    for label, name, both, retired, edited, alpha, beta in PRESENCE_SHAPES:

        @fixture(f"{label} ours retired and theirs never touched stays retired")
        def _ours_retired(both=both, retired=retired, name=name, alpha=alpha, beta=beta):
            # A DELETION IS NOT AN ABSENCE, and the first version of this
            # rule could not tell them apart: it appended every block theirs
            # had and ours lacked without asking the base, so a block this
            # branch retired on purpose came straight back.
            #
            # Found in the field rather than here: #65 retired
            # `recompute.recompute_nothing_recomputable` and replaced it with
            # a case that proves something else; the union put the entry back
            # while its seeded case stayed gone, and check-fault-manifest.py
            # refused the result -- a manifest claiming a fault verify.sh
            # does not prove. The layered check caught what this tool should
            # never have written.
            took, out, err, on_disk = drive_three_way(both, retired, both, name)
            if took is not True:
                return f"the union refused a plain retirement: {err.strip()[:160]!r}"
            if alpha in on_disk:
                return "a block ours deliberately retired was resurrected by the union"
            if beta not in on_disk:
                return "the union dropped a block neither side touched"
            if "RETIRED by" not in out:
                return (
                    "the retirement was kept silently; a resolver that acts on a "
                    "block without saying so is this tool's own defect pointing "
                    "the other way"
                )
            return None

        @fixture(f"{label} theirs genuinely added is still taken")
        def _theirs_added(both=both, retired=retired, name=name, alpha=alpha):
            # The control that stops the row above from becoming "never add
            # anything". Without it, a union that dropped every one-sided
            # block would satisfy the retirement row perfectly.
            took, out, _err, on_disk = drive_three_way(retired, retired, both, name)
            if took is not True:
                return "the union refused a block theirs added"
            if alpha not in on_disk:
                return "a block theirs added was dropped as though ours had retired it"
            if "added" not in out:
                return "the addition was taken without saying so"
            return None

        @fixture(f"{label} ours retired and theirs edited is a person's")
        def _ours_retired_theirs_edited(both=both, retired=retired, edited=edited, name=name):
            # The case neither side can be given: ours removed the block,
            # theirs changed it. Taking theirs reverts a deliberate
            # retirement; keeping ours discards an authored edit.
            took, _out, err, on_disk = drive_three_way(both, retired, edited, name)
            if took is not False:
                return "the union chose between a retirement and an edit of the same block"
            if on_disk != retired:
                return "a refused union rewrote the file anyway"
            if "retired by ours" not in err:
                return "the refusal did not say the block was one ours had retired"
            return None

        # --- and the same three, with the sides swapped ------------------
        #
        # `built` starts as `ours`, so every block ours carried survived by
        # default and in silence -- no rule, no printed line, not even the
        # careful-reader escape hatch the half above at least provides.

        @fixture(f"{label} theirs retired and ours never touched is dropped")
        def _theirs_retired(both=both, retired=retired, name=name, alpha=alpha, beta=beta):
            took, out, err, on_disk = drive_three_way(both, both, retired, name)
            if took is not True:
                return f"the union refused a plain retirement by theirs: {err.strip()[:160]!r}"
            if alpha in on_disk:
                return (
                    "a block THEIRS deliberately retired survived the union, "
                    "because ours carried it and nothing asked the base"
                )
            if beta not in on_disk:
                return "the union dropped a block neither side touched"
            if "RETIRED by" not in out:
                return "the block was dropped silently, which is worse than keeping it"
            if on_disk != retired:
                # Byte-for-byte, not just "the marker is gone". In THIS
                # fixture ours is the base, so once the retirement is honoured
                # the assembled file is theirs exactly -- which is what makes
                # the strong form available here, and what catches the blank
                # lines a removal leaves behind. It is a fact about the
                # fixture, not a rule about every merge.
                return (
                    f"the retirement was honoured but left residue: "
                    f"{on_disk!r} != {retired!r}"
                )
            return None

        @fixture(f"{label} ours genuinely added is still kept")
        def _ours_added(both=both, retired=retired, name=name, alpha=alpha):
            # The control for the row above, and it is not redundant with
            # "theirs added": that one enters through `insert_after_last`,
            # this one through never being removed. Different code, same
            # question.
            took, _out, _err, on_disk = drive_three_way(retired, both, retired, name)
            if took is not True:
                return "the union refused a block ours added"
            if alpha not in on_disk:
                return "a block ours added was dropped as though theirs had retired it"
            return None

        @fixture(f"{label} theirs retired and ours edited is a person's")
        def _theirs_retired_ours_edited(both=both, retired=retired, edited=edited, name=name):
            took, _out, err, on_disk = drive_three_way(both, edited, retired, name)
            if took is not False:
                return "the union chose between a retirement and an edit of the same block"
            if on_disk != edited:
                return "a refused union rewrote the file anyway"
            if "retired by theirs" not in err:
                return "the refusal did not say the block was one theirs had retired"
            return None


_presence_matrix()


@fixture("a block both sides edited is refused and printed")
def _union_refuses_a_contested_block():
    took, _out, err, on_disk = drive_three_way(BASE_PAIR, ALPHA_ONE, ALPHA_TWO)
    if took is not False:
        return "the union chose between two authored versions of one block"
    if on_disk != ALPHA_ONE:
        return "a refused union rewrote the file anyway"
    if "inject_alpha" not in err:
        return "the refusal named no block, so the reader cannot act on it"
    if "echo two > a.txt" not in err:
        return "the refusal did not print the block, only its name"
    return None


@fixture("the union refuses an added mechanics assertion and rewrites nothing")
def _union_refuses_and_does_not_write():
    # The refusal itself, not a proxy for it. No fixture reached `union_file`
    # before this one, so "fix the refusal into a guess" -- the edit the
    # neighbouring fixture says it exists to prevent -- was invisible, and so
    # was an `expect_exit` normalizer in `skeleton_of`, under which this shape
    # compares equal and the assertion is dropped in silence.
    took, err, on_disk = drive_union(CHECK_BODY, CHECK_GROWN)
    if took is not False:
        return f"the union proceeded on an expect_exit-only difference: took={took!r}"
    if on_disk != CHECK_GROWN:
        return "a refused union rewrote the file anyway"
    if "mechanics assertion(s) differ" not in err:
        return f"the refusal did not say what it met: {err.strip()[:160]!r}"
    if "the second thing" not in err:
        return "the refusal named no assertion, so the reader cannot act on it"
    return None


@fixture("the union takes a block by name and writes the assembled file")
def _union_takes_a_block_and_writes():
    # The positive arm, for the same reason: a refusal fixture alone is
    # satisfied by a `union_file` that refuses everything.
    took, err, on_disk = drive_union(CHECK_BODY, CHECK_BODY + BETA)
    if took is not True:
        return f"the union refused a difference confined to a named block: {err.strip()[:160]!r}"
    if "inject_beta() {" not in on_disk:
        return "the union returned True without writing the block it took"
    if "# The deliberate pair of inject_alpha." not in on_disk:
        return "the block travelled without the comment that explains it"
    if "inject_alpha() {" not in on_disk:
        return "the union dropped ours while taking theirs"
    return None


# The exit codes, which are a contract in this repository and not a habit:
#
#   0  every fixture ran and passed
#   1  a fixture ran and FOUND SOMETHING
#   2  the suite COULD NOT RUN, and therefore found nothing either way
#
# 2 is the one a check like this gets wrong. A suite that cannot run prints a
# tidy summary line and exits 0, and 0 is indistinguishable from "the resolver
# is sound" to every caller -- CI, verify.sh, a person reading a log. A check
# of nothing is not a pass.
EXIT_FOUND = 1
EXIT_CANNOT_RUN = 2


def main() -> int:
    # A CHECK OF NOTHING IS NOT A PASS. If `FIXTURES` is empty -- a refactor
    # that drops the decorator, an import that half-fails, a future harness
    # that filters -- everything below succeeds vacuously and prints
    # `0 fixture(s), 0 failed`. The resolver every lane merge in this
    # repository runs through would then be gated by a green line meaning
    # nothing was asked.
    if not FIXTURES:
        print(
            "check-merge-gate: no fixtures were collected, so nothing was "
            "asked of the resolver; that is a broken harness, not a pass",
            file=sys.stderr,
        )
        return EXIT_CANNOT_RUN

    # Every fixture drives a real git repository. Without git there is no
    # verdict to give, and reporting one either way would be a lie -- 0 says
    # the resolver is sound and 1 says it is broken, and neither was measured.
    if shutil.which("git") is None:
        print(
            "check-merge-gate: no `git` on PATH; every fixture drives a real "
            "repository, so this suite has no verdict to give",
            file=sys.stderr,
        )
        return EXIT_CANNOT_RUN

    failed = []
    for name, fn in FIXTURES:
        try:
            why = fn()
        except Exception as err:
            # A fixture that explodes stays a FINDING, not a could-not-run.
            # These call into `merge-gate.py`, and the crash this most often
            # is, is the resolver raising on input it should have refused
            # cleanly -- which is exactly what this suite exists to catch.
            # The two guards above cover the cases where the harness itself
            # is what is missing.
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
    return EXIT_FOUND if failed else 0


if __name__ == "__main__":
    sys.exit(main())
