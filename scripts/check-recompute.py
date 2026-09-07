#!/usr/bin/env python3
"""Gate 0: every results directory's recorded numbers re-derive, or says why not.

A results directory is a claim with its evidence attached, and the linter
already requires the report to agree with the record. That is agreement
between two things the same run wrote: a summary is a CLAIM about a run, not a
reading of it, so a summary stating three turns over a record holding two
passes the linter and is wrong.

Re-deriving is the answer, and it is the directory's own job -- what re-derives
a token count is not what re-derives a bootstrap interval. So each directory
carries a ``recompute.sh`` implementing one contract: run it, and it exits 0 if
and only if the numbers the report states come back out of the artefacts
committed beside it. This script runs them and counts.

Not everything can be re-derived. A run against a model that no longer exists,
an observation of a system that has since changed -- those are real results and
they are not reproducible by configuration. A directory says which it is, in
its front-matter, and the census prints all three numbers:

    N recomputed, M declared historical, 0 undeclared

The zero is the point. A directory that declares nothing is neither checked nor
counted as skipped, which is how a gate comes to run over nothing while
reporting success -- so an undeclared directory is a failure, and finding no
recomputable directory at all is exit 2 rather than a pass.

Stdlib only. Exit 0 if every recomputable directory re-derives, 1 if one does
not or a directory is undeclared, 2 if there is nothing to recompute.
"""

from __future__ import annotations

import argparse
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib

FENCE = "+++"
RECOMPUTE = "recompute.sh"

# The two kinds a directory may declare. `reproducible-by-config` is run;
# `historical-observation` is skipped BY DECLARATION and counted, which is a
# different thing from being skipped because nobody said anything.
REPRODUCIBLE = "reproducible-by-config"
HISTORICAL = "historical-observation"
KINDS = (REPRODUCIBLE, HISTORICAL)

EXIT_FAIL = 1
EXIT_NOTHING = 2


def front_matter(directory: pathlib.Path) -> tuple[dict | None, str | None]:
    """A directory's README front-matter, or the reason it could not be read."""
    report = directory / "README.md"
    if not report.is_file():
        return None, "has no README.md"
    try:
        text = report.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError) as err:
        return None, f"README.md cannot be read: {err}"
    if not text.startswith(FENCE + "\n"):
        return None, "README.md does not open with `+++` front-matter"
    parts = text.split(FENCE + "\n", 2)
    if len(parts) < 3:
        return None, "README.md front-matter is not closed"
    try:
        return tomllib.loads(parts[1]), None
    except ValueError as err:
        return None, f"README.md front-matter is not TOML: {err}"


INTEGER = re.compile(r"^(\w+) = (\d+)$", re.M)


def tracked_state(directory: pathlib.Path) -> str | None:
    """What git says about a directory, or None if git cannot say."""
    run = subprocess.run(
        ["git", "status", "--porcelain", "--", str(directory)],
        capture_output=True, text=True,
    )
    return run.stdout if run.returncode == 0 else None


def proves_it_compares(directory: pathlib.Path, script: pathlib.Path) -> str | None:
    """Run the script against a copy whose report has been perturbed.

    Exiting 0 is not evidence of re-derivation. A zero-byte `recompute.sh`
    exits 0, and so does one that prints a cheerful message, and so does one
    that recomputes a number and never compares it to anything -- all three
    count as "1 recomputed" in a census whose entire meaning is that the
    numbers came back out of the artefacts. The only way to know a script
    compares is to give it something that should not match and require it to
    say so. So: one integer in the report is changed, and the script is run
    against that copy. A script that still exits 0 was not reading the report.
    """
    report = (directory / "README.md")
    try:
        text = report.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError) as err:
        return f"README.md cannot be read for the comparison probe: {err}"
    match = INTEGER.search(text)
    if match is None:
        return None  # no number stated; nothing to perturb, nothing to claim
    bumped = f"{match.group(1)} = {int(match.group(2)) + 1}"
    with tempfile.TemporaryDirectory() as box:
        copy = pathlib.Path(box) / directory.name
        shutil.copytree(directory, copy, symlinks=True)
        (copy / "README.md").write_text(
            text[: match.start()] + bumped + text[match.end() :], encoding="utf-8"
        )
        # The COPY's script, not the original's. Every `recompute.sh` opens by
        # cd-ing to its own directory -- that is the contract, so it runs from
        # anywhere -- which means handing it the original path would send it
        # straight back to the unperturbed directory and the probe would
        # always pass. It would then be a check that cannot fail, inside the
        # check for scripts that cannot fail.
        run = subprocess.run(
            ["bash", str((copy / RECOMPUTE).resolve())],
            capture_output=True, text=True, cwd=copy,
        )
    if run.returncode == 0:
        return (
            f"{RECOMPUTE} exits 0 with `{match.group(0)}` in the report changed to "
            f"`{bumped}`, so it does not compare the report to the artefacts; a "
            f"script that cannot fail recomputes nothing"
        )
    return None


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", default="results", help="the results tree to walk")
    args = parser.parse_args(argv)

    root = pathlib.Path(args.root)
    if not root.is_dir():
        print(f"check-recompute: {root} is not a directory", file=sys.stderr)
        return EXIT_NOTHING

    failures: list[str] = []
    recomputed = 0
    historical = 0
    undeclared = 0

    for directory in sorted(p for p in root.iterdir() if p.is_dir()):
        front, err = front_matter(directory)
        if front is None:
            undeclared += 1
            failures.append(f"{directory}: {err}")
            continue
        kind = front.get("kind")
        if kind not in KINDS:
            undeclared += 1
            failures.append(
                f"{directory}: front-matter `kind` is {kind!r}; it must be one of "
                f"{' or '.join(KINDS)}, because a directory that declares nothing is "
                f"neither checked nor knowingly skipped"
            )
            continue
        if kind == HISTORICAL:
            historical += 1
            continue

        script = directory / RECOMPUTE
        if not script.is_file():
            failures.append(
                f"{directory}: declares `{REPRODUCIBLE}` and carries no {RECOMPUTE}"
            )
            continue
        # An empty script exits 0. So does one that is nothing but comments.
        body = "\n".join(
            line for line in script.read_text(encoding="utf-8", errors="replace").splitlines()
            if line.strip() and not line.lstrip().startswith("#")
        ).strip()
        if not body:
            failures.append(
                f"{directory}: {RECOMPUTE} has no executable content, so it exits 0 "
                f"without recomputing anything"
            )
            continue
        before = tracked_state(directory)
        run = subprocess.run(
            ["bash", str(script.resolve())],
            capture_output=True,
            text=True,
            cwd=directory,
        )
        after = tracked_state(directory)
        if before is None or after is None:
            # Not a reason to pass. "git could not answer" and "nothing
            # changed" are the same silence, and only one of them is good
            # news; a tampering check that skips itself when it cannot look
            # is the vacuous pass it exists to prevent.
            failures.append(
                f"{directory}: git could not report whether {RECOMPUTE} modified "
                f"the tree, so whether it tampered is unknown and cannot be assumed"
            )
            continue
        if before != after:
            # Re-derivation READS the artefacts. A script that writes the
            # report to match them has made the comparison true rather than
            # found it true, which is the one outcome gate 0 must never accept.
            failures.append(
                f"{directory}: {RECOMPUTE} modified the working tree. A recompute "
                f"that edits what it is checking is tampering, not recomputation"
            )
            continue
        if run.returncode != 0:
            detail = (run.stderr or run.stdout or "").strip().splitlines()
            failures.append(
                f"{directory}: {RECOMPUTE} exited {run.returncode}"
                + ("\n  " + "\n  ".join(detail) if detail else "")
            )
            continue
        vacuous = proves_it_compares(directory, script)
        if vacuous is not None:
            failures.append(f"{directory}: {vacuous}")
            continue
        recomputed += 1

    for message in failures:
        print(message, file=sys.stderr)

    census = (
        f"check-recompute: {recomputed} recomputed, {historical} declared "
        f"historical, {undeclared} undeclared"
    )
    if failures:
        print(census, file=sys.stderr)
        return EXIT_FAIL
    if recomputed == 0:
        print(
            f"{census}\ncheck-recompute: nothing was recomputed; a check of nothing "
            f"is not a pass",
            file=sys.stderr,
        )
        return EXIT_NOTHING
    print(census)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
