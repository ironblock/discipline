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

# `historical-observation` is a claim about the world, so it has to say which
# part of the world: which capture-side flaw, or why the inputs cannot exist.
# Without one it is a tag, and a tag that costs nothing to apply is the opt-out
# every red result reaches for.
HISTORICAL_REASON = "historical_reason"

# The template is the thing every results directory is copied from, so it is
# counted on its own line and never satisfies the check. It recomputes -- it
# has to, or every copy starts from a script nobody ran -- but a tree whose
# only recomputing directory is the one nobody drew a conclusion from has
# checked no science, and reporting that as a pass is how a gate comes to run
# over nothing while printing a number.
TEMPLATE = "_template"

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


class CannotProbe(Exception):
    """The comparison probe could not be set up on this directory.

    ITS OWN EXCEPTION, and it becomes EXIT_NOTHING rather than a failure,
    because "the probe found the script vacuous" and "the probe could not be
    run" are different facts and this script's exit codes already say so
    everywhere else. Returning `None` for the second was the defect: the
    caller reads `None` as "the probe passed" and counts the directory as
    RECOMPUTED, so a report the probe could not read became a recomputed
    result in the census -- in the gate whose docstring is "a check of
    nothing is not a pass".
    """


# Locating the line to perturb. NOT A SECOND READER: whatever this matches is
# a PROPOSAL, and `tomllib` -- the same reader `front_matter` uses -- confirms
# the perturbation landed before the probe is trusted. The regex it replaced
# was `^(\w+) = (\d+)$`, which is a TOML reader written in a hurry: a trailing
# comment (`dogma_version = 0  # bumped when the dogma changes`) does not
# match it, the probe returned None, and the directory counted as recomputed.
INTEGER_LINE = re.compile(r"^(?P<lead>[ \t]*(?P<key>[A-Za-z0-9_-]+)[ \t]*=[ \t]*)(?P<value>\d+)", re.M)


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
    report = directory / "README.md"
    try:
        text = report.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError) as err:
        raise CannotProbe(f"README.md cannot be read: {err}") from err

    # THE AUTHORISED READER PICKS THE FIELD. `front_matter` is the one place
    # this repository parses a report's front-matter, and it uses `tomllib`;
    # the probe used to run its own regex over the whole file instead, which
    # is the two-readers class inside the vacuity check.
    parsed, why = front_matter(directory)
    if parsed is None:
        raise CannotProbe(f"the front-matter could not be read ({why})")
    integers = sorted(
        key
        for key, value in parsed.items()
        # `bool` is an `int` in Python, and bumping `true` to `2` would make a
        # document TOML still accepts and mean nothing.
        if isinstance(value, int) and not isinstance(value, bool)
    )
    if not integers:
        # NOT a pass. A report stating no number is a report this probe cannot
        # perturb, so it has no verdict to give -- and counting it as
        # recomputed is exactly the vacuous pass the probe exists to prevent.
        raise CannotProbe(
            "the front-matter states no integer, so there is nothing to perturb "
            "and no way to tell a script that compares from one that does not"
        )
    key = integers[0]
    want = parsed[key] + 1

    # Locate the line and rewrite only its digits, so a trailing comment
    # survives. Then RE-PARSE: the regex proposed the edit, `tomllib` decides
    # whether it landed.
    perturbed, count = INTEGER_LINE.subn(
        lambda m: f"{m.group('lead')}{want}" if m.group("key") == key else m.group(0),
        text,
        count=0,
    )
    if count == 0 or perturbed == text:
        raise CannotProbe(f"`{key}` could not be located in README.md to perturb it")
    with tempfile.TemporaryDirectory() as box:
        copy = pathlib.Path(box) / directory.name
        shutil.copytree(directory, copy, symlinks=True)
        (copy / "README.md").write_text(perturbed, encoding="utf-8")
        confirmed, why = front_matter(copy)
        if confirmed is None or confirmed.get(key) != want:
            raise CannotProbe(
                f"the perturbation did not land: `{key}` reads "
                f"{None if confirmed is None else confirmed.get(key)!r} after the edit, "
                f"wanted {want!r}"
                + (f" ({why})" if why else "")
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
            f"{RECOMPUTE} exits 0 with `{key}` in the report changed from "
            f"{parsed[key]!r} to {want!r}, so it does not compare the report to the "
            f"artefacts; a script that cannot fail recomputes nothing"
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
    # Directories whose vacuity probe could not be set up. Their own list, and
    # their own exit code: they are neither proven vacuous nor proven sound,
    # and folding them into either column is the lie this whole check exists
    # to refuse.
    unprobed: list[str] = []
    recomputed = 0
    historical = 0
    undeclared = 0
    templates = 0
    results_seen = 0

    for directory in sorted(p for p in root.iterdir() if p.is_dir()):
        is_template = directory.name == TEMPLATE
        if not is_template:
            results_seen += 1
        front, err = front_matter(directory)
        if front is None:
            undeclared += not is_template
            failures.append(f"{directory}: {err}")
            continue
        kind = front.get("kind")
        if kind not in KINDS:
            undeclared += not is_template
            failures.append(
                f"{directory}: front-matter `kind` is {kind!r}; it must be one of "
                f"{' or '.join(KINDS)}, because a directory that declares nothing is "
                f"neither checked nor knowingly skipped"
            )
            continue
        if is_template and kind != REPRODUCIBLE:
            failures.append(
                f"{directory}: the template declares `{kind}`. Every results "
                f"directory is copied from it, so a template carrying the "
                f"opt-out hands it to every copy before anyone has run anything"
            )
            continue
        if kind == HISTORICAL:
            # Not an unconditional opt-out any more. It has to say what makes
            # the run unreproducible, and it has to have no script -- because
            # "declared historical" and "carries a recompute" is a
            # contradiction, and the way a red result would escape this gate
            # is by acquiring the tag rather than by losing the script.
            reason = front.get(HISTORICAL_REASON)
            if not isinstance(reason, str) or not reason.strip():
                failures.append(
                    f"{directory}: declares `{HISTORICAL}` and states no "
                    f"`{HISTORICAL_REASON}`. Which capture-side flaw, or why the "
                    f"inputs cannot exist -- a tag with no reason behind it is "
                    f"the opt-out every red result reaches for"
                )
                continue
            if (directory / RECOMPUTE).is_file():
                failures.append(
                    f"{directory}: declares `{HISTORICAL}` and carries a "
                    f"{RECOMPUTE}. Declared unreproducible and recomputable is a "
                    f"contradiction; if the script runs, the result is checked, "
                    f"and a script that exits 1 is a red result no tag can skip. "
                    f"Becoming historical means removing the script and saying "
                    f"why, in a change somebody reviews"
                )
                continue
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
        try:
            vacuous = proves_it_compares(directory, script)
        except CannotProbe as err:
            # NOT a failure and NOT a count. The census below reports what was
            # recomputed; a directory whose vacuity could not be established
            # belongs in neither column, and saying so is EXIT_NOTHING.
            unprobed.append(f"{directory}: {err}")
            continue
        if vacuous is not None:
            failures.append(f"{directory}: {vacuous}")
            continue
        if is_template:
            templates += 1
        else:
            recomputed += 1

    for message in failures:
        print(message, file=sys.stderr)

    census = (
        f"check-recompute: template: {templates} · results: {recomputed} recomputed, "
        f"{historical} declared historical, {undeclared} undeclared"
    )
    if failures:
        print(census, file=sys.stderr)
        return EXIT_FAIL
    if unprobed:
        # Before the census is printed as a claim, not after. A directory the
        # probe could not read is one this script has no verdict on, and the
        # census's whole meaning is that the numbers came back out of the
        # artefacts.
        for message in unprobed:
            print(f"{message}", file=sys.stderr)
        print(
            f"{census}\ncheck-recompute: {len(unprobed)} directory(s) could not be "
            f"probed for vacuity, so whether their scripts compare anything is "
            f"unknown; that is not a pass",
            file=sys.stderr,
        )
        return EXIT_NOTHING
    if results_seen == 0:
        # A tree with no results directory in it has nothing to check, and
        # says so. This is the one shape that is not a failure: it is a
        # DECLARED empty rather than a pass, and it stops being available the
        # moment a results directory lands, because the next branch is then
        # the one that runs.
        print(f"{census}\ncheck-recompute: no results directory yet; declared empty")
        return 0
    if recomputed == 0:
        print(
            f"{census}\ncheck-recompute: results are present and none recomputed; a "
            f"check of nothing is not a pass, and the template does not count",
            file=sys.stderr,
        )
        return EXIT_NOTHING
    print(census)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
