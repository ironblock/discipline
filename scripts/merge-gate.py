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

  1. **Union by name.** Inside each conflict hunk, both sides' blocks, ours
     first, the incumbent's copy kept where both carry one. `red_faults` is
     the base plus each side's delta -- never either side's number, which is
     partial by construction. A hunk that is not a list of named blocks is
     refused, and so is a file that is neither of these two: a source file's
     conflict belongs to a person.

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
    merge-gate.py --base-red-faults N            # patch conflict hunks in place
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

HUNK = re.compile(r"^<<<<<<< [^\n]*\n(.*?)^=======\n(.*?)^>>>>>>> [^\n]*\n", re.M | re.S)
CASE = re.compile(r"^  seeded_case .*?\n(?:^    .*\n)+", re.M)
ENTRY = re.compile(r"^\[\[fault\]\]\n(?:^(?!\[\[fault\]\]).*\n)+", re.M)
# The comment above an injection is the reason it exists, and it travels with
# the body: a merge that took one and left the other happened once already.
FUNC = re.compile(r"(?:^#[^\n]*\n)*^(inject_[a-z0-9_]+)\(\) \{\n.*?^\}\n", re.M | re.S)
RED = re.compile(r"^red_faults = (\d+)\n$")
RED_LINE = re.compile(r"^red_faults = (\d+)\n", re.M)
CHECKS_LINE = re.compile(r"^readonly CHECKS=\(([^)]*)\)\n", re.M)

GATE_FILES = ("verify.sh", "faults.toml")


def key_of(pattern):
    if pattern is ENTRY:
        return lambda block: (m := re.search(r'^id = "([^"]+)"', block, re.M)) and m.group(1)
    return lambda block: (m := re.search(r"inject_[a-z0-9_]+", block)) and m.group(0)


def blocks(pattern, side):
    """The named blocks a side is made of, or None if it is made of anything
    else. Blank lines between blocks are not anything else."""
    key = key_of(pattern)
    found, consumed = [], 0
    for m in pattern.finditer(side):
        if side[consumed : m.start()].strip():
            return None
        consumed = m.end()
        name = key(m.group(0))
        if not name:
            return None
        found.append((name, m.group(0)))
    if side[consumed:].strip():
        return None
    return found


def union(pattern, ours, theirs, opener="", closer=""):
    """Both sides' blocks joined, or None if this pattern does not fit.

    `opener` and `closer` are what a hunk may be missing because its boundary
    fell inside a block: the `[[fault]]` header above, the closing `}` below.
    The same repair is applied to both sides and taken back off the result.
    """
    left = blocks(pattern, opener + ours + closer)
    right = blocks(pattern, opener + theirs + closer)
    if left is None or right is None:
        return None
    out, seen = [], set()
    for name, block in left + right:
        if name not in seen:
            seen.add(name)
            out.append(block.rstrip("\n"))
    joined = "\n\n".join(out) + "\n"
    if opener:
        joined = joined[len(opener) :]
    if closer:
        joined = joined[: -len(closer)]
    return joined


def resolve(path: Path, base_red: int) -> int | None:
    text = path.read_text(encoding="utf-8")
    refused = []

    def fix(m):
        ours, theirs = m.group(1), m.group(2)
        # One side empty means the other side added lines here and this one
        # did not -- the union is the side that has something. Safe because
        # these two files are append-only lists: nothing removes a fault or an
        # injection in the ordinary course, so an empty side is never a
        # deletion this would be reverting.
        if not ours.strip():
            return theirs
        if not theirs.strip():
            return ours
        o, t = RED.match(ours), RED.match(theirs)
        if o and t:
            total = base_red + (int(o.group(1)) - base_red) + (int(t.group(1)) - base_red)
            return f"red_faults = {total}\n"
        for pattern, opener, closer in (
            (CASE, "", ""),
            (FUNC, "", ""),
            (FUNC, "", "}\n"),
            (ENTRY, "", ""),
            (ENTRY, "[[fault]]\n", ""),
        ):
            joined = union(pattern, ours, theirs, opener, closer)
            if joined is not None:
                return joined
        refused.append((ours.splitlines()[:2], theirs.splitlines()[:2]))
        return m.group(0)

    out, count = HUNK.subn(fix, text)
    if refused:
        for ours, theirs in refused:
            print(
                f"{path}: a hunk that is not a list of named blocks:\n"
                f"  ours   {ours}\n  theirs {theirs}",
                file=sys.stderr,
            )
        return None
    path.write_text(out, encoding="utf-8")
    return count


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
        raise ValueError("nothing to insert after")
    joined = "".join(block if block.endswith("\n") else block + "\n" for block in additions)
    return text[:end] + joined + text[end:]


def union_file(path: Path, ours_ref: str, theirs_ref: str) -> bool:
    """Rebuild one gate file from the two sides, by name."""
    ours = show(ours_ref, str(path))
    theirs = show(theirs_ref, str(path))
    patterns = (FUNC, CASE) if path.name == "verify.sh" else (ENTRY,)

    # The part no union decides. `red_faults` and the CHECKS array are the two
    # lines that legitimately differ, and both are reconciled below.
    def skeleton(text: str) -> str:
        text = strip_blocks(text, patterns)
        text = RED_LINE.sub("red_faults = N\n", text)
        text = CHECKS_LINE.sub("CHECKS = ...\n", text)
        # Removing a block leaves the blank lines that framed it, and how many
        # depends on where the block sat. Runs of them are not a difference.
        return re.sub(r"\n{2,}", "\n\n", text)

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
        print(
            f"{path}: the two sides differ outside the named blocks; that part "
            f"is not a union and is yours to resolve:",
            file=sys.stderr,
        )
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
            print(f"merge-gate: {path.name}: took {len(added)} block(s) from {theirs_ref}")

    # `red_faults` is COUNTED from the assembled list, not computed from the
    # two sides' deltas. The delta arithmetic is wrong whenever the branches
    # share history the merge base does not -- a criss-cross, which a stack of
    # lanes built on each other produces routinely -- and it double-counts
    # every fault both sides inherited that way. The count follows from the
    # list this union just built, and `check-fault-manifest.py --count-red` is
    # the one reader that knows how to do it.
    if RED_LINE.search(built):
        built = RED_LINE.sub("red_faults = 0\n", built, count=1)
        path.write_text(built, encoding="utf-8")
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

    o, t = CHECKS_LINE.search(ours), CHECKS_LINE.search(theirs)
    if o and t and o.group(0) != t.group(0):
        mine = o.group(1).split()
        joined = mine + [c for c in t.group(1).split() if c not in mine]
        built = CHECKS_LINE.sub(f"readonly CHECKS=({' '.join(joined)})\n", built, count=1)
        print(f"merge-gate: CHECKS unioned to {len(joined)}: {' '.join(joined)}")

    path.write_text(built, encoding="utf-8")
    return True


def show(ref: str, path: str) -> str:
    return subprocess.run(
        ["git", "show", f"{ref}:{path}"], capture_output=True, text=True, check=True
    ).stdout


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
    total = len(FUNC.findall(out))
    print(
        f"merge-gate: {total} injection(s), {len(restored)} restored from a parent"
        + (f" ({', '.join(restored)})" if restored else "")
    )
    return True


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--base-red-faults",
        type=int,
        help="`red_faults` at the merge base, so the union's count is the sum "
        "of both sides' deltas rather than either side's number",
    )
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
    if args.base_red_faults is None and args.repair is None and args.union is None:
        parser.error("one of --union, --base-red-faults or --repair is required")

    paths = [Path(f) for f in args.files]
    for path in paths:
        if path.name not in GATE_FILES:
            print(f"refusing: {path} is neither verify.sh nor faults.toml", file=sys.stderr)
            return 2

    if args.union is not None:
        for path in paths:
            if path.is_file() and not union_file(path, *args.union):
                return 1

    if args.base_red_faults is not None:
        total = 0
        for path in paths:
            if not path.is_file():
                continue
            count = resolve(path, args.base_red_faults)
            if count is None:
                return 1
            total += count
        print(f"merge-gate: {total} hunk(s) resolved by name")

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
    sys.exit(main(sys.argv[1:]))
