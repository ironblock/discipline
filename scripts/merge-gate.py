#!/usr/bin/env python3
"""Resolve a verify.sh / faults.toml merge by NAME, and prove the result.

`verify.sh` and `tools/gate/faults.toml` are lists of named blocks --
injection functions, seeded cases, fault entries -- and every branch of a
stack appends to all three. A line-oriented resolver cuts through a Python
heredoc and splices one injection into the next, which is SILENT: `verify.sh`
does not run its own injections, so the tree stays green while the seeded
cases prove nothing about the guards they name. That has happened twice,
once for fourteen faults at a stroke, and once for seventeen rationale
comments that were dropped because nothing executes a comment.

Two passes, because one is not enough:

  1. **Union by name.** Both sides' blocks, ours first, the incumbent's copy
     kept where both carry one -- reading the two sides whole rather than the
     conflict hunks, because a hunk boundary is wherever the diff happened to
     land. `red_faults` and `mechanics_assertions` are COUNTED from the
     assembled tree: base-plus-deltas double-counts whatever both branches
     inherited by two routes, which a stack of lanes produces routinely, and
     it was retracted forty minutes after it was adopted. A file that is
     neither of these two is refused: a source file's conflict belongs to a
     person.

  2. **Repair against the parents.** A conflict boundary falls wherever the
     sides diverge, which is often inside a heredoc -- so the two sides share
     a TAIL as well as a head, and a union gives that tail to whichever block
     ends the hunk and leaves the other truncated. The result is a body that
     is in neither parent. A merge adds and removes injections; it never
     edits one. So any function whose text matches neither parent is replaced
     with the parent's, and one that is in neither parent at all is reported
     rather than guessed at. Six of eight lanes needed this on one merge.

Neither pass is the proof. `./verify.sh --only injections` is: it applies
every injection to a copy of the tree and reports the ones that change
nothing. Run it afterwards, always -- these passes are how you get an answer
worth checking, not the check.

    merge-gate.py --union OURS THEIRS            # rebuild both files from the two sides
    merge-gate.py --repair OURS THEIRS           # restore any injection in neither parent

`--union` is the one to reach for. It ignores the conflict hunks entirely and
rebuilds each file from the two sides: ours, plus every block of theirs whose
name ours does not have. Hunks are an artifact of where the two texts happened
to diverge -- git aligns two DIFFERENT injections on the identical boilerplate
that opens their heredocs, and no hunk-level rule can tell that apart from one
injection edited two ways. Rebuilding from names cannot make that mistake.

It refuses when the two sides differ anywhere OUTSIDE the named blocks, and
prints what differs: that part is a person's to read.
"""

import argparse
import re
import subprocess
import sys
from pathlib import Path

# A merge of the gate files conflicts this script against itself, so the
# resolution is run from a pristine copy OUTSIDE the tree being merged. That
# copy is not on `sys.path` as a package, so the shared reader is found
# beside this file rather than by ambient import -- and `gatelib.py` has to
# travel with it.
sys.path.insert(0, str(Path(__file__).resolve().parent))
try:
    import gatelib
except ModuleNotFoundError:  # pragma: no cover -- an operator error, not a bug
    sys.exit(
        f"merge-gate: gatelib.py is not beside {Path(__file__).name}. It holds "
        f"the one reader of `seeded_case`; copy both, or run from scripts/."
    )

CASE = re.compile(r"^  seeded_case .*?\n(?:^    .*\n)+", re.M)
ENTRY = re.compile(r"^\[\[fault\]\]\n(?:^(?!\[\[fault\]\]).*(?:\n|$))+", re.M)
# The comment above an injection is the reason it exists, and it travels with
# the body: a merge that took one and left the other happened once already.
FUNC = re.compile(r"(?:^#[^\n]*\n)*^(inject_[a-z0-9_]+)\(\) \{\n.*?^\}\n", re.M | re.S)
RED = re.compile(r"^red_faults = (\d+)\n$")
RED_LINE = re.compile(r"^red_faults = (\d+)\n", re.M)
MECH_LINE = re.compile(r"^mechanics_assertions = (\d+)\n", re.M)

# What each pattern collects, for output that says which kind it took.
KIND: dict[int, str] = {}
CHECKS_LINE = re.compile(r"^readonly CHECKS=\(([^)]*)\)\n", re.M)

GATE_FILES = ("verify.sh", "faults.toml")


KIND.update({id(FUNC): "injection", id(CASE): "seeded case", id(ENTRY): "fault entry"})


def key_of(pattern):
    if pattern is ENTRY:
        return lambda block: (m := re.search(r'^id = "([^"]+)"', block, re.M)) and m.group(1)
    if pattern is CASE:
        # A case is a CALL, so it has no definition line to anchor on. It is
        # keyed by the injection it names, and which injection that is comes
        # from the one reader of that spelling rather than from a fourth
        # regex here.
        return lambda block: (
            (cases := gatelib.seeded_cases(block)) and cases[0].injection
        )
    # A definition is anchored on its definition line, not on the first name
    # anywhere in the block: the rationale comment above an injection
    # routinely names another one ("the deliberate pair of inject_alpha"),
    # and keying on that makes the union think it already holds a body it has
    # never seen.
    return lambda block: (
        m := re.search(r"^(inject_[a-z0-9_]+)\(\) \{", block, re.M)
    ) and m.group(1)


def find_blocks(pattern, text):
    """Every named block in a text, in order. Unlike `blocks`, this makes no
    claim that the text is nothing but blocks -- it is a finder, and `blocks`
    is a parser for one conflict hunk."""
    key = key_of(pattern)
    return [(key(m.group(0)), m.group(0)) for m in pattern.finditer(text) if key(m.group(0))]


def strip_blocks(text: str, patterns) -> str:
    """The file with every named block taken out, so what is left is the part
    a union does not decide."""
    for pattern in patterns:
        text = pattern.sub("", text)
    return text


def insert_after_last(text: str, pattern, additions: list[str]) -> str:
    """Put `additions` where the last block this pattern matches ends."""
    if not additions:
        return text
    end = None
    for m in pattern.finditer(text):
        end = m.end()
    if end is None:
        raise Missing(
            "nothing to insert after: the file carries no block of this kind, "
            "so there is no end to append to"
        )
    head, tail = text[:end], text[end:]
    # A file whose last line carries no newline ends its last block at EOF,
    # and splicing there appends the next block onto that line: `migrated =
    # false[[fault]]`, which is a syntax error, not a fault list. One byte.
    if head and not head.endswith("\n"):
        head += "\n"
    joined = "".join(block if block.endswith("\n") else block + "\n" for block in additions)
    return head + joined + tail


def skeleton_of(text: str, patterns) -> str:
    """The part of a gate file no union decides.

    Everything outside the named blocks, with the lines both sides derive
    rather than author normalized away. `red_faults` and
    `mechanics_assertions` are counted from the assembled tree, and the
    CHECKS array is unioned below -- refusing a merge over a number this
    tool is about to recompute is a refusal with no content in it.
    """
    text = strip_blocks(text, patterns)
    text = RED_LINE.sub("red_faults = N\n", text)
    text = MECH_LINE.sub("mechanics_assertions = N\n", text)
    text = CHECKS_LINE.sub("CHECKS = ...\n", text)
    # Removing a block leaves the blank lines that framed it, and how many
    # depends on where the block sat. Runs of them are not a difference.
    return re.sub(r"\n{2,}", "\n\n", text)


def union_file(path: Path, ours_ref: str, theirs_ref: str) -> bool:
    """Rebuild one gate file from the two sides, by name."""
    ours = show(ours_ref, str(path))
    theirs = show(theirs_ref, str(path))
    patterns = (FUNC, CASE) if path.name == "verify.sh" else (ENTRY,)

    def skeleton(text: str) -> str:
        return skeleton_of(text, patterns)

    if skeleton(ours) != skeleton(theirs):
        import difflib

        diff = list(
            difflib.unified_diff(
                skeleton(ours).splitlines(True),
                skeleton(theirs).splitlines(True),
                fromfile=f"{ours_ref}:{path} (outside the named blocks)",
                tofile=f"{theirs_ref}:{path} (outside the named blocks)",
                n=2,
            )
        )
        # A mechanics assertion is the difference this refusal meets most
        # often, and it is NOT given a block pattern on purpose: `expect_exit`
        # is a statement inside a check function, with setup above it that has
        # no delimiter, so a pattern guessing its extent would splice one
        # assertion's setup onto another -- the hazard this whole tool exists
        # to avoid. Refusing is right; refusing without saying why is not.
        added = [
            line[1:].strip()
            for line in diff
            if line.startswith(("+", "-")) and "expect_exit" in line
        ]
        print(
            f"{path}: the two sides differ outside the named blocks; that part "
            f"is not a union and is yours to resolve:",
            file=sys.stderr,
        )
        if added:
            print(
                f"  {len(added)} mechanics assertion(s) differ. These are not "
                f"unioned by name: `expect_exit` sits inside a check function "
                f"with undelimited setup above it, so its extent cannot be "
                f"guessed safely. Take both by hand.",
                file=sys.stderr,
            )
            for line in added[:5]:
                print(f"    {line[:90]}", file=sys.stderr)
        sys.stderr.writelines(diff[:80])
        return False

    built = ours
    for pattern in patterns:
        mine = {name for name, _ in find_blocks(pattern, ours)}
        added = [
            block for name, block in find_blocks(pattern, theirs) if name not in mine
        ]
        built = insert_after_last(built, pattern, added)
        if added:
            # Naming them, because "took 2 block(s)" is the same sentence
            # whether the union added a lane's new injection or resurrected
            # one this branch deliberately retired -- and those want
            # different reactions from the person reading the output.
            taken = [name for name, _ in find_blocks(pattern, theirs) if name not in mine]
            print(
                f"merge-gate: {path.name}: took {len(added)} "
                f"{KIND.get(id(pattern), 'block')}(s) from {theirs_ref}: "
                + ", ".join(taken)
            )

    # `red_faults` is COUNTED from the assembled list, not computed from the
    # two sides' deltas. The delta arithmetic is wrong whenever the branches
    # share history the merge base does not -- a criss-cross, which a stack of
    # lanes built on each other produces routinely -- and it double-counts
    # every fault both sides inherited that way. The count follows from the
    # list this union just built, and `check-fault-manifest.py --count-red` is
    # the one reader that knows how to do it.
    if RED_LINE.search(built):
        # Nothing is on disk yet, and nothing may be until every check below
        # has passed. An earlier version wrote a `red_faults = 0` placeholder
        # here so the counter could see the assembled file -- but the counter
        # reads `verify.sh`, never this one, and the failure return left that
        # zero on disk with the conflict markers already gone.
        counted = subprocess.run(
            [sys.executable, "scripts/check-fault-manifest.py", "--count-red"],
            capture_output=True,
            text=True,
        )
        if counted.returncode != 0 or not counted.stdout.strip().isdigit():
            print(
                f"{path}: could not count the assembled faults; the manifest says:\n"
                + (counted.stderr or counted.stdout),
                file=sys.stderr,
            )
            return False
        total = int(counted.stdout.strip())
        built = RED_LINE.sub(f"red_faults = {total}\n", built, count=1)
        print(f"merge-gate: red_faults counted from the assembled list: {total}")

    if MECH_LINE.search(built):
        counted = subprocess.run(
            [sys.executable, "scripts/check-fault-manifest.py", "--count-mechanics"],
            capture_output=True,
            text=True,
        )
        if counted.returncode != 0 or not counted.stdout.strip().isdigit():
            print(
                f"{path}: could not count the assembled mechanics assertions; "
                f"the manifest says:\n" + (counted.stderr or counted.stdout),
                file=sys.stderr,
            )
            return False
        mechanics = int(counted.stdout.strip())
        built = MECH_LINE.sub(f"mechanics_assertions = {mechanics}\n", built, count=1)
        print(
            f"merge-gate: mechanics_assertions counted from verify.sh: {mechanics}"
        )

    o, t = CHECKS_LINE.search(ours), CHECKS_LINE.search(theirs)
    if o and t and o.group(0) != t.group(0):
        mine = o.group(1).split()
        joined = mine + [c for c in t.group(1).split() if c not in mine]
        built = CHECKS_LINE.sub(f"readonly CHECKS=({' '.join(joined)})\n", built, count=1)
        print(f"merge-gate: CHECKS unioned to {len(joined)}: {' '.join(joined)}")

    path.write_text(built, encoding="utf-8")
    return True


class Missing(Exception):
    """A ref or a path this tool was pointed at is not there."""


def show(ref: str, path: str) -> str:
    run = subprocess.run(
        ["git", "show", f"{ref}:{path}"], capture_output=True, text=True
    )
    if run.returncode != 0:
        # A mistyped ref, or a side that does not carry this file at all. Both
        # are things an operator can fix in a second once told; neither is a
        # stack trace's business.
        raise Missing(
            f"cannot read {path} at {ref}: "
            f"{(run.stderr or '').strip().splitlines()[-1:] or ['no such object']}"[:200]
        )
    return run.stdout


def repair(path: Path, ours_ref: str, theirs_ref: str) -> bool:
    text = path.read_text(encoding="utf-8")
    parents = [
        {m.group(1): m.group(0) for m in FUNC.finditer(show(ref, str(path)))}
        for ref in (ours_ref, theirs_ref)
    ]
    restored, orphaned = [], []

    def fix(m):
        name, block = m.group(1), m.group(0)
        if any(parent.get(name) == block for parent in parents):
            return block
        for parent in parents:
            if name in parent:
                restored.append(name)
                return parent[name]
        orphaned.append(name)
        return block

    out, _ = FUNC.subn(fix, text)
    if orphaned:
        print(f"{path}: in neither parent: {', '.join(orphaned)}", file=sys.stderr)
        return False
    path.write_text(out, encoding="utf-8")

    # Where the two parents carry the same name with DIFFERENT bodies, the
    # incumbent's copy is not always the right one: an injection anchors on
    # source text, and the side that changed that source also changed its
    # anchor. Keeping ours then ships a fault whose assert fails -- it injects
    # nothing and its seeded case goes green proving nothing.
    #
    # Which body is right is not a textual question, so it is asked
    # empirically: run the pre-flight, and for every injection it reports
    # inert whose parents disagree, take the other parent's body and ask
    # again. The pre-flight is the authority on whether an injection bites;
    # this only chooses what to hand it.
    disagree = {
        name
        for name in set(parents[0]) & set(parents[1])
        if parents[0][name] != parents[1][name]
    }
    swapped = []
    for _ in range(2):
        # The repository root, not two levels up from whatever path was
        # passed: `verify.sh` sits AT the root, so `path.parent.parent` is
        # the directory above it -- right only by accident, because the
        # default --files is relative and `Path("verify.sh").parent` is `.`.
        inert = inert_injections(Path.cwd().resolve())
        if inert is None:
            break
        candidates = [n for n in inert if n in disagree and n not in swapped]
        if not candidates:
            break
        text = path.read_text(encoding="utf-8")
        for name in candidates:
            current = next(
                (m.group(0) for m in FUNC.finditer(text) if m.group(1) == name), None
            )
            other = next(
                (parent[name] for parent in parents if parent[name] != current), None
            )
            if other is None:
                continue
            text = text.replace(current, other, 1)
            swapped.append(name)
        path.write_text(text, encoding="utf-8")

    total = len(FUNC.findall(path.read_text(encoding="utf-8")))
    print(
        f"merge-gate: {total} injection(s), {len(restored)} restored from a parent"
        + (f" ({', '.join(restored)})" if restored else "")
        + (
            f"; {len(swapped)} taken from the other parent because ours no longer "
            f"bit ({', '.join(swapped)})"
            if swapped
            else ""
        )
    )
    return True


def inert_injections(root: Path) -> list[str] | None:
    """The injections the pre-flight says change nothing, or None if it could
    not be asked."""
    run = subprocess.run(
        [sys.executable, "scripts/check-injections.py"],
        capture_output=True,
        text=True,
        cwd=root,
    )
    if run.returncode not in (0, 1):
        return None
    # The pre-flight has an early refusal -- a seeded case naming an
    # injection defined nowhere -- that returns 1 having printed only to
    # stderr. Its stdout carries no summary, and reading that absence as an
    # empty list says "nothing is inert" about a tree nobody examined, on
    # exactly the merge this tool exists to repair.
    if not any(
        line.startswith("check-injections:") and "change nothing" in line
        for line in run.stdout.splitlines()
    ):
        return None
    return [
        line.split()[0]
        for line in run.stdout.splitlines()
        if line.startswith("  inject_")
    ]


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--union",
        nargs=2,
        metavar=("OURS", "THEIRS"),
        help="rebuild both gate files from the two sides, by name, ignoring "
        "the conflict hunks entirely",
    )
    parser.add_argument(
        "--repair",
        nargs=2,
        metavar=("OURS", "THEIRS"),
        help="the two refs being merged, for the second pass",
    )
    parser.add_argument(
        "--files",
        nargs="*",
        default=["verify.sh", "tools/gate/faults.toml"],
        help="the gate files to work on",
    )
    args = parser.parse_args(argv)
    if args.repair is None and args.union is None:
        parser.error("one of --union or --repair is required")

    # The guard is on the resolved path, not the basename: `/etc/verify.sh`
    # and `../other-clone/verify.sh` both end in a gate file's name and
    # neither is this tree's. A resolver pointed outside the repository it is
    # resolving is not a resolver.
    root = Path.cwd().resolve()
    paths = [Path(f) for f in args.files]
    for path in paths:
        if path.name not in GATE_FILES:
            print(f"refusing: {path} is neither verify.sh nor faults.toml", file=sys.stderr)
            return 2
        if not path.resolve().is_relative_to(root):
            print(
                f"refusing: {path} resolves outside {root}, so it is not this "
                f"tree's gate file",
                file=sys.stderr,
            )
            return 2

    if args.union is not None:
        for path in paths:
            if path.is_file() and not union_file(path, *args.union):
                return 1

    if args.repair is not None:
        for path in paths:
            if path.name == "verify.sh" and path.is_file():
                if not repair(path, *args.repair):
                    return 1

    print(
        "merge-gate: now run `./verify.sh --only injections`; this script is "
        "how you get an answer worth checking, not the check"
    )
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except Missing as gone:
        # An operator pointed this at a ref or a file that is not there.
        # Exit 2, the same code every other refusal in this gate uses for
        # "I was asked something I cannot answer", and say what is missing.
        sys.exit(f"merge-gate: {gone}")
